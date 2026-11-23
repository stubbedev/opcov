use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread;

use opcov_core::report::{FileCoverage, Header, RawEntry, ReadError, ReportReader, SourcePath};

use crate::failure::Failure;

const READ_BUFFER: usize = 1 << 20;
const IN_FLIGHT_PER_WORKER: usize = 64;

fn open(path: &Path) -> Result<BufReader<File>, Failure> {
    let file = File::open(path).map_err(|error| return Failure::io("open", path, &error))?;
    return Ok(BufReader::with_capacity(READ_BUFFER, file));
}

fn unreadable(path: &Path, error: &ReadError) -> Failure {
    return Failure::failed(format!("cannot read report {}: {error}", path.display()));
}

fn reader(path: &Path) -> Result<ReportReader<BufReader<File>>, Failure> {
    return ReportReader::open(open(path)?).map_err(|error| return unreadable(path, &error));
}

pub(crate) fn header(path: &Path) -> Result<Header, Failure> {
    return reader(path).map(ReportReader::into_header);
}

fn workers() -> usize {
    return thread::available_parallelism().map_or(1, NonZeroUsize::get);
}

enum Message<T> {
    Done(usize, Result<T, Failure>),
    End {
        produced: usize,
        failure: Option<Failure>,
    },
}

struct Producer<'a, W, T> {
    source: &'a Path,
    wants: &'a W,
    tokens: Receiver<()>,
    jobs: SyncSender<(usize, RawEntry)>,
    done: Sender<Message<T>>,
}

impl<W: Fn(&SourcePath) -> bool, T> Producer<'_, W, T> {
    fn run(self, reader: ReportReader<BufReader<File>>) {
        let mut produced = 0_usize;
        let mut failure = None;
        for entry in reader {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    failure = Some(unreadable(self.source, &error));
                    break;
                }
            };
            if !(self.wants)(entry.path()) {
                continue;
            }
            if self.tokens.recv().is_err() || self.jobs.send((produced, entry)).is_err() {
                return;
            }
            produced = produced.saturating_add(1);
        }
        drop(self.jobs);
        self.done
            .send(Message::End { produced, failure })
            .unwrap_or_default();
    }
}

fn work<T, F>(
    header: &Header,
    source: &Path,
    jobs: &Mutex<Receiver<(usize, RawEntry)>>,
    done: &Sender<Message<T>>,
    process: &F,
) where
    F: Fn(usize, SourcePath, FileCoverage) -> Result<T, Failure>,
{
    loop {
        let job = match jobs.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_poisoned) => return,
        };
        let Ok((sequence, entry)) = job else {
            return;
        };
        let result = entry
            .decode(header)
            .map_err(|error| return unreadable(source, &error))
            .and_then(|(path, file)| return process(sequence, path, file));
        if done.send(Message::Done(sequence, result)).is_err() {
            return;
        }
    }
}

fn consume<T>(
    done: &Receiver<Message<T>>,
    tokens: &SyncSender<()>,
    emit: &mut dyn FnMut(T) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let mut pending: BTreeMap<usize, Result<T, Failure>> = BTreeMap::new();
    let mut next = 0_usize;
    let mut end: Option<(usize, Option<Failure>)> = None;
    loop {
        while let Some(result) = pending.remove(&next) {
            emit(result?)?;
            next = next.saturating_add(1);
            tokens.send(()).unwrap_or_default();
        }
        if let Some((produced, failure)) = &mut end
            && *produced == next
        {
            return failure.take().map_or(Ok(()), Err);
        }
        match done.recv() {
            Ok(Message::Done(sequence, result)) => {
                pending.insert(sequence, result);
            }
            Ok(Message::End { produced, failure }) => end = Some((produced, failure)),
            Err(_closed) => {
                return Err(Failure::failed(
                    "the report reader stopped unexpectedly".to_owned(),
                ));
            }
        }
    }
}

pub(crate) fn entries<T, W, F>(
    source: &Path,
    header: &Header,
    wants: &W,
    process: &F,
    emit: &mut dyn FnMut(T) -> Result<(), Failure>,
) -> Result<(), Failure>
where
    T: Send,
    W: Fn(&SourcePath) -> bool + Sync,
    F: Fn(usize, SourcePath, FileCoverage) -> Result<T, Failure> + Sync,
{
    let reader = reader(source)?;
    if reader.header() != header {
        return Err(Failure::failed(format!(
            "cannot read report {}: it changed while it was read",
            source.display()
        )));
    }
    let workers = workers();
    let capacity = workers.saturating_mul(IN_FLIGHT_PER_WORKER);
    let (job_sender, job_receiver) = mpsc::sync_channel(capacity);
    let jobs = Mutex::new(job_receiver);
    return thread::scope(|scope| {
        let (token_sender, token_receiver) = mpsc::sync_channel(capacity);
        for _ in 0..capacity {
            token_sender.send(()).unwrap_or_default();
        }
        let (done_sender, done_receiver) = mpsc::channel();
        let producer = Producer {
            source,
            wants,
            tokens: token_receiver,
            jobs: job_sender,
            done: done_sender.clone(),
        };
        scope.spawn(move || producer.run(reader));
        let jobs = &jobs;
        for _ in 0..workers {
            let done = done_sender.clone();
            scope.spawn(move || work(header, source, jobs, &done, process));
        }
        drop(done_sender);
        let consumed = consume(&done_receiver, &token_sender, emit);
        drop(token_sender);
        drop(done_receiver);
        return consumed;
    });
}

pub(crate) const fn everything(_path: &SourcePath) -> bool {
    return true;
}
