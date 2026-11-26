use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use opcov_php::{Answer, Index, Project, Query, Refresh};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::failure::Failure;
use crate::output::write_stderr;
use crate::{index, local};

macro_rules! log {
    ($($argument:tt)*) => {
        drop(write_stderr(|out| return writeln!(out, "opcov serve: {}", format_args!($($argument)*))))
    };
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPLY_TIMEOUT: Duration = Duration::from_mins(15);
const QUIET: Duration = Duration::from_millis(50);
const IDLE_CHECKS: u32 = 4;
const SPAWNED_IDLE: Duration = Duration::from_hours(1);
const STARTUP: Duration = Duration::from_secs(10);
const STARTUP_POLL: Duration = Duration::from_millis(20);
const SOCKET_NAME: &str = "serve.sock";
const RUNTIME_DIRECTORY: &str = "XDG_RUNTIME_DIR";
#[cfg(target_os = "macos")]
const MAX_SOCKET_PATH: usize = 103;
#[cfg(not(target_os = "macos"))]
const MAX_SOCKET_PATH: usize = 107;
const DIGEST_BYTES: usize = 8;
const REBUILD_FILES: [&str; 3] = ["composer.json", "composer.lock", "config/app.php"];
const AUTOLOAD_DIRECTORY: &str = "vendor/composer";
const SOURCE_EXTENSION: &str = "php";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "message")]
enum Message {
    Query { version: String, query: Query },
    Stop { version: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "reply")]
enum Reply {
    Answer { answer: Answer },
    Stopped,
    Error { error: String },
}

type Fingerprint = Option<[u8; 32]>;

struct Live {
    project: Project,
    index: Index,
    inputs: BTreeMap<PathBuf, Fingerprint>,
}

type Shared = Arc<RwLock<Live>>;
type Pending = Arc<Mutex<BTreeSet<PathBuf>>>;

pub(crate) fn socket_path(root: &Path) -> Result<PathBuf, Failure> {
    let root =
        fs::canonicalize(root).map_err(|error| return Failure::io("resolve", root, &error))?;
    let path = std::env::var_os(RUNTIME_DIRECTORY)
        .filter(|directory| return !directory.is_empty())
        .map_or_else(
            || return local::path(&root, SOCKET_NAME),
            |directory| {
                let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
                let name: String = digest
                    .iter()
                    .take(DIGEST_BYTES)
                    .map(|byte| return format!("{byte:02x}"))
                    .collect();
                return PathBuf::from(directory).join(format!("opcov-{name}.sock"));
            },
        );
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(Failure::failed(format!(
            "the socket path {} is longer than a Unix socket allows; set {RUNTIME_DIRECTORY} to a short private directory",
            path.display()
        )));
    }
    return Ok(path);
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    return mutex.lock().unwrap_or_else(PoisonError::into_inner);
}

fn read(shared: &Shared) -> std::sync::RwLockReadGuard<'_, Live> {
    return shared.read().unwrap_or_else(PoisonError::into_inner);
}

fn write(shared: &Shared) -> std::sync::RwLockWriteGuard<'_, Live> {
    return shared.write().unwrap_or_else(PoisonError::into_inner);
}

fn digest(path: &Path) -> Result<Fingerprint, Failure> {
    return match fs::read(path) {
        Ok(bytes) => Ok(Some(Sha256::digest(bytes).into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Failure::io("read", path, &error)),
    };
}

fn is_input(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    return REBUILD_FILES
        .iter()
        .any(|file| return relative == Path::new(file))
        || relative.starts_with(AUTOLOAD_DIRECTORY);
}

fn inputs(root: &Path) -> Result<BTreeMap<PathBuf, Fingerprint>, Failure> {
    let mut found = BTreeMap::new();
    for file in REBUILD_FILES {
        let path = root.join(file);
        found.insert(path.clone(), digest(&path)?);
    }
    let autoload = root.join(AUTOLOAD_DIRECTORY);
    let entries = match fs::read_dir(&autoload) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(Failure::io("list", &autoload, &error)),
    };
    for entry in entries {
        let path = entry
            .map_err(|error| return Failure::io("list", &autoload, &error))?
            .path();
        if !path.is_dir() {
            found.insert(path.clone(), digest(&path)?);
        }
    }
    return Ok(found);
}

fn build(root: &Path) -> Result<Live, Failure> {
    let inputs = inputs(root)?;
    let project = Project::open(root).map_err(|error| return Failure::failed(error.to_string()))?;
    let index = index::build(&project)?;
    return Ok(Live {
        project,
        index,
        inputs,
    });
}

fn changed_input(live: &Live, paths: &BTreeSet<PathBuf>) -> Result<Option<String>, Failure> {
    for path in paths
        .iter()
        .filter(|path| return is_input(live.project.root(), path))
    {
        if live.inputs.get(path).copied().flatten() != digest(path)? {
            let relative = path.strip_prefix(live.project.root()).unwrap_or(path);
            return Ok(Some(format!("{} changed", relative.display())));
        }
    }
    return Ok(None);
}

fn warm(shared: &Shared) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        let started = Instant::now();
        let analysed = read(&shared).index.warm();
        log!("analysed {analysed} file(s) in {:.1?}", started.elapsed());
    });
}

fn relevant(root: &Path, path: &Path) -> bool {
    return path
        .extension()
        .is_some_and(|extension| return extension == SOURCE_EXTENSION)
        || is_input(root, path);
}

fn sync(shared: &Shared, pending: &Pending, extra: Vec<PathBuf>) -> Result<(), Failure> {
    let root = read(shared).project.root().to_path_buf();
    let mut paths: BTreeSet<PathBuf> = std::mem::take(&mut *lock(pending));
    paths.extend(extra);
    paths.retain(|path| return relevant(&root, path));
    if paths.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    let mut live = write(shared);
    let refresh = if let Some(reason) = changed_input(&live, &paths)? {
        Refresh::Rebuild(reason)
    } else {
        let changed: Vec<&Path> = paths
            .iter()
            .filter(|path| return !is_input(&root, path))
            .map(PathBuf::as_path)
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        let Live { project, index, .. } = &mut *live;
        index
            .refresh(project, &changed)
            .map_err(|error| return Failure::failed(error.to_string()))?
    };
    match refresh {
        Refresh::Unchanged => {}
        Refresh::Updated(files) => {
            log!("updated {} in {:.1?}", files.join(", "), started.elapsed());
        }
        Refresh::Rebuild(reason) => {
            *live = build(&root)?;
            log!("rebuilt the index in {:.1?}: {reason}", started.elapsed());
            drop(live);
            warm(shared);
        }
    }
    return Ok(());
}

const fn changes_content(kind: EventKind) -> bool {
    return !matches!(
        kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
    );
}

fn watch(
    project: &Project,
    pending: &Pending,
) -> Result<(RecommendedWatcher, Receiver<()>), Failure> {
    let (sender, receiver) = mpsc::channel();
    let queue = Arc::clone(pending);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                log!("the file watcher failed: {error}");
                return;
            }
        };
        if !changes_content(event.kind) {
            return;
        }
        lock(&queue).extend(event.paths);
        if sender.send(()).is_err() {
            log!("the refresher stopped; changes are no longer applied");
        }
    })
    .map_err(|error| return Failure::failed(format!("cannot watch the project: {error}")))?;
    let root = project.root();
    let mut targets: Vec<(PathBuf, RecursiveMode)> = project
        .scope()
        .iter()
        .map(|path| return (path.clone(), RecursiveMode::Recursive))
        .collect();
    targets.push((root.to_path_buf(), RecursiveMode::NonRecursive));
    for directory in ["config", AUTOLOAD_DIRECTORY] {
        targets.push((root.join(directory), RecursiveMode::NonRecursive));
    }
    for (path, mode) in targets.into_iter().filter(|(path, _)| return path.exists()) {
        watcher.watch(&path, mode).map_err(|error| {
            return Failure::failed(format!("cannot watch {}: {error}", path.display()));
        })?;
    }
    return Ok((watcher, receiver));
}

fn refresher(shared: Shared, pending: Pending, events: Receiver<()>) {
    thread::spawn(move || {
        while events.recv().is_ok() {
            loop {
                match events.recv_timeout(QUIET) {
                    Ok(()) => {}
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if let Err(error) = sync(&shared, &pending, Vec::new()) {
                log!("{error}");
            }
        }
    });
}

fn respond(stream: &UnixStream, reply: &Reply) {
    let mut writer = stream;
    if serde_json::to_writer(&mut writer, reply).is_ok() {
        drop(writer.write_all(b"\n"));
    }
}

fn handle(
    stream: &UnixStream,
    shared: &Shared,
    pending: &Pending,
    stopping: &AtomicBool,
    socket: &Path,
) {
    let mut line = String::new();
    if let Err(error) = BufReader::new(stream).read_line(&mut line) {
        respond(
            stream,
            &Reply::Error {
                error: format!("cannot read the request: {error}"),
            },
        );
        return;
    }
    let message = match serde_json::from_str::<Message>(&line) {
        Ok(message) => message,
        Err(error) => {
            respond(
                stream,
                &Reply::Error {
                    error: format!("invalid request: {error}"),
                },
            );
            return;
        }
    };
    if let Message::Query { version, .. } = &message
        && version != VERSION
    {
        respond(
            stream,
            &Reply::Error {
                error: format!(
                    "opcov serve {VERSION} cannot answer opcov {version}: restart opcov serve"
                ),
            },
        );
        return;
    }
    match message {
        Message::Stop { .. } => {
            stopping.store(true, Ordering::SeqCst);
            respond(stream, &Reply::Stopped);
            if let Err(error) = UnixStream::connect(socket) {
                log!("cannot wake the listener to stop: {error}");
            }
        }
        Message::Query { query, .. } => {
            let root = read(shared).project.root().to_path_buf();
            let extra = query
                .changes
                .iter()
                .map(|change| return root.join(&change.path))
                .collect();
            let reply = match sync(shared, pending, extra) {
                Ok(()) => Reply::Answer {
                    answer: read(shared).index.answer(&query),
                },
                Err(error) => Reply::Error {
                    error: error.to_string(),
                },
            };
            respond(stream, &reply);
        }
    }
}

fn bind(path: &Path) -> Result<UnixListener, Failure> {
    if UnixStream::connect(path).is_ok() {
        return Err(Failure::failed(format!(
            "opcov serve is already running on {}",
            path.display()
        )));
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Failure::io("remove the stale socket", path, &error)),
    }
    local::create_parent(path)?;
    return UnixListener::bind(path).map_err(|error| return Failure::io("listen on", path, &error));
}

fn watchdog(idle: Duration, last: &Arc<Mutex<Instant>>, stopping: &Arc<AtomicBool>, socket: &Path) {
    let (last, stopping, socket) = (Arc::clone(last), Arc::clone(stopping), socket.to_path_buf());
    let interval = idle
        .checked_div(IDLE_CHECKS)
        .unwrap_or(idle)
        .clamp(Duration::from_secs(1), Duration::from_mins(1));
    thread::spawn(move || {
        loop {
            thread::sleep(interval);
            if lock(&last).elapsed() < idle {
                continue;
            }
            log!("stopping after {:.0?} without a query", idle);
            stopping.store(true, Ordering::SeqCst);
            if let Err(error) = UnixStream::connect(&socket) {
                log!("cannot wake the listener to stop: {error}");
            }
            return;
        }
    });
}

pub(crate) fn serve(root: &Path, idle: Option<Duration>) -> Result<(), Failure> {
    let root =
        fs::canonicalize(root).map_err(|error| return Failure::io("resolve", root, &error))?;
    let socket = socket_path(&root)?;
    let listener = bind(&socket)?;
    let started = Instant::now();
    let live = build(&root)?;
    log!(
        "indexed {} file(s) in {:.1?}, listening on {}",
        live.index.stats().scanned,
        started.elapsed(),
        socket.display()
    );
    let pending: Pending = Arc::new(Mutex::new(BTreeSet::new()));
    let (_watcher, events) = watch(&live.project, &pending)?;
    let shared: Shared = Arc::new(RwLock::new(live));
    warm(&shared);
    refresher(Arc::clone(&shared), Arc::clone(&pending), events);
    let stopping = Arc::new(AtomicBool::new(false));
    let last = Arc::new(Mutex::new(Instant::now()));
    if let Some(idle) = idle {
        watchdog(idle, &last, &stopping, &socket);
    }
    for stream in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        *lock(&last) = Instant::now();
        let stream = stream.map_err(|error| return Failure::io("accept on", &socket, &error))?;
        let (shared, pending, stopping, socket) = (
            Arc::clone(&shared),
            Arc::clone(&pending),
            Arc::clone(&stopping),
            socket.clone(),
        );
        thread::spawn(move || handle(&stream, &shared, &pending, &stopping, &socket));
    }
    return fs::remove_file(&socket).map_err(|error| return Failure::io("remove", &socket, &error));
}

fn exchange(stream: &mut UnixStream, message: &Message) -> Result<Reply, Failure> {
    stream
        .set_read_timeout(Some(REPLY_TIMEOUT))
        .and_then(|()| return stream.set_write_timeout(Some(REPLY_TIMEOUT)))
        .map_err(|error| {
            return Failure::failed(format!(
                "cannot configure the opcov serve connection: {error}"
            ));
        })?;
    serde_json::to_writer(&mut *stream, message)
        .map_err(|error| return Failure::failed(format!("cannot send to opcov serve: {error}")))?;
    stream
        .write_all(b"\n")
        .map_err(|error| return Failure::failed(format!("cannot send to opcov serve: {error}")))?;
    let mut line = String::new();
    BufReader::new(&*stream)
        .read_line(&mut line)
        .map_err(|error| return Failure::failed(format!("no reply from opcov serve: {error}")))?;
    return serde_json::from_str(&line).map_err(|error| {
        return Failure::failed(format!("invalid reply from opcov serve: {error}"));
    });
}

pub(crate) fn ensure(root: &Path) -> Result<(), Failure> {
    let socket = socket_path(root)?;
    if UnixStream::connect(&socket).is_ok() {
        return Ok(());
    }
    let program = std::env::current_exe().map_err(|error| {
        return Failure::failed(format!(
            "cannot find the opcov binary to start opcov serve: {error}"
        ));
    })?;
    std::process::Command::new(program)
        .arg("serve")
        .arg("--root")
        .arg(root)
        .arg("--idle-timeout")
        .arg(SPAWNED_IDLE.as_secs().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| return Failure::failed(format!("cannot start opcov serve: {error}")))?;
    let started = Instant::now();
    while UnixStream::connect(&socket).is_err() {
        if started.elapsed() > STARTUP {
            return Err(Failure::failed(format!(
                "opcov serve did not listen on {} within {:.0?}",
                socket.display(),
                STARTUP
            )));
        }
        thread::sleep(STARTUP_POLL);
    }
    return Ok(());
}

pub(crate) fn ask(root: &Path, query: &Query) -> Result<Option<Answer>, Failure> {
    let Ok(mut stream) = UnixStream::connect(socket_path(root)?) else {
        return Ok(None);
    };
    let message = Message::Query {
        version: VERSION.to_owned(),
        query: query.clone(),
    };
    return match exchange(&mut stream, &message)? {
        Reply::Answer { answer } => Ok(Some(answer)),
        Reply::Error { error } => Err(Failure::failed(format!("opcov serve: {error}"))),
        Reply::Stopped => Err(Failure::failed(
            "opcov serve stopped instead of answering".to_owned(),
        )),
    };
}

pub(crate) fn stop(root: &Path) -> Result<bool, Failure> {
    let Ok(mut stream) = UnixStream::connect(socket_path(root)?) else {
        return Ok(false);
    };
    return match exchange(
        &mut stream,
        &Message::Stop {
            version: VERSION.to_owned(),
        },
    )? {
        Reply::Stopped => Ok(true),
        Reply::Error { error } => Err(Failure::failed(format!("opcov serve: {error}"))),
        Reply::Answer { .. } => Err(Failure::failed(
            "opcov serve answered instead of stopping".to_owned(),
        )),
    };
}
