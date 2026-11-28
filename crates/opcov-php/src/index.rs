use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_analyzer::plugin::PluginRegistry;
use mago_analyzer::settings::Settings;
use mago_codex::metadata::{CodebaseEntryKeys, CodebaseMetadata};
use mago_codex::populator::populate_codebase;
use mago_codex::reference::SymbolReferences;
use mago_codex::scanner::scan_program;
use mago_codex::signature_builder::build_file_signature;
use mago_database::file::{File, HasFileId};
use mago_names::resolver::NameResolver;
use mago_prelude::Prelude;
use mago_syntax::parser::parse_file;
use mago_syntax::walker::Walker;
use mago_word::Word;
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

use crate::cache::{
    CacheError, Digest, Key, ScanCache, Stamp, decode_scanned, digest, encode_scanned,
};
use crate::consumers::Untyped;
use crate::facts::FileFacts;
use crate::hierarchy::{TraitUsers, trait_users};
use crate::laravel;
use crate::laravel::Hooks;
use crate::project::Project;
use crate::references::{Reach, referenced_classes};

const PRELUDE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/prelude.bin"));

#[derive(Debug)]
pub enum IndexError {
    Prelude(String),
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    Cache {
        name: String,
        error: CacheError,
    },
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return match self {
            Self::Read { path, error } => {
                write!(formatter, "cannot read {}: {error}", path.display())
            }
            Self::Prelude(error) => write!(
                formatter,
                "cannot load the PHP prelude built into opcov: {error}"
            ),
            Self::Cache { name, error } => {
                write!(formatter, "cannot cache the scan of {name}: {error}")
            }
        };
    }
}

impl std::error::Error for IndexError {}

impl IndexError {
    fn path(&self) -> Option<&Path> {
        return match self {
            Self::Read { path, .. } => Some(path),
            Self::Prelude(_) | Self::Cache { .. } => None,
        };
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexStats {
    pub scanned: usize,
    pub sources: usize,
    pub models: usize,
    pub facades: usize,
}

pub struct Index {
    pub(crate) metadata: CodebaseMetadata,
    pub(crate) registry: PluginRegistry,
    pub(crate) settings: Settings,
    pub(crate) sources: Vec<Source>,
    pub(crate) forwards: laravel::Forwards,
    pub(crate) hooks: Hooks,
    pub(crate) symbol_references: SymbolReferences,
    pub(crate) trait_users: TraitUsers,
    pub(crate) positions: HashMap<String, usize>,
    pub(crate) untyped: Vec<Untyped>,
    root: PathBuf,
    stats: IndexStats,
}

pub(crate) type Signature = Vec<(Word, u64, Vec<(Word, u64)>)>;

pub(crate) fn signature(metadata: &CodebaseMetadata) -> Signature {
    let mut nodes: Signature = metadata
        .file_signatures
        .values()
        .flat_map(|file| return file.ast_nodes.iter())
        .map(|node| {
            return (
                node.name,
                node.signature_hash,
                node.children
                    .iter()
                    .map(|child| return (child.name, child.signature_hash))
                    .collect(),
            );
        })
        .collect();
    nodes.sort();
    return nodes;
}

pub(crate) struct Record {
    pub(crate) keys: CodebaseEntryKeys,
    pub(crate) signature: Signature,
    pub(crate) references: BTreeSet<String>,
    pub(crate) hooks: Hooks,
}

pub(crate) struct Source {
    pub(crate) file: File,
    pub(crate) path: String,
    pub(crate) facts: Arc<OnceLock<FileFacts>>,
    pub(crate) record: Record,
}

impl Source {
    pub(crate) fn new(file: File, record: Record) -> Self {
        let path = String::from_utf8_lossy(file.name.as_ref()).into_owned();
        return Self {
            file,
            path,
            facts: Arc::new(OnceLock::new()),
            record,
        };
    }
}

impl std::fmt::Debug for Index {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return formatter
            .debug_struct("Index")
            .field("root", &self.root)
            .field("stats", &self.stats)
            .finish_non_exhaustive();
    }
}

fn logical_name(root: &Path, path: &Path) -> String {
    return path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
}

pub(crate) fn load(project: &Project, path: &Path) -> Result<File, IndexError> {
    let content = project.read(path).map_err(|error| {
        return IndexError::Read {
            path: path.to_path_buf(),
            error,
        };
    })?;
    return Ok(File::ephemeral(
        Cow::Owned(logical_name(project.root(), path).into_bytes()),
        Cow::Owned(content),
    ));
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Scanned {
    pub(crate) metadata: CodebaseMetadata,
    pub(crate) references: BTreeSet<String>,
    pub(crate) hooks: Hooks,
}

fn claim<T: Eq + std::hash::Hash>(seen: &Mutex<HashSet<T>>, item: T) -> bool {
    return seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(item);
}

#[derive(Debug)]
pub struct Build {
    pub index: Index,
    pub cache: ScanCache,
    pub parsed: usize,
    pub changed: bool,
}

struct Caching {
    previous: Mutex<BTreeMap<Key, Vec<u8>>>,
    vendor: bool,
}

impl Caching {
    fn new(previous: ScanCache, lock: Option<Digest>) -> Self {
        let vendor_valid = lock.is_some() && previous.vendor == lock;
        let entries = previous
            .entries
            .into_iter()
            .filter(|(key, _)| return vendor_valid || key.stamp != Stamp::Vendor)
            .collect();
        return Self {
            previous: Mutex::new(entries),
            vendor: lock.is_some(),
        };
    }

    fn reuse(&self, key: &Key) -> Option<Obtained> {
        let bytes = self
            .previous
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key)?;
        let scanned = decode_scanned(&bytes)?;
        return Some(Obtained {
            scanned,
            fresh: Fresh::Reused,
            entry: Some((key.clone(), bytes)),
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fresh {
    Parsed,
    Reused,
}

struct Obtained {
    scanned: Scanned,
    fresh: Fresh,
    entry: Option<(Key, Vec<u8>)>,
}

impl Obtained {
    fn encoded(key: Key, scanned: Scanned) -> Result<Self, IndexError> {
        let bytes = encode_scanned(&scanned).map_err(|error| {
            return IndexError::Cache {
                name: key.name.clone(),
                error,
            };
        })?;
        return Ok(Self {
            scanned,
            fresh: Fresh::Parsed,
            entry: Some((key, bytes)),
        });
    }
}

enum Visit {
    Source(File),
    Loaded(File),
    Path(PathBuf),
}

fn file_name(file: &File) -> String {
    return String::from_utf8_lossy(file.name.as_ref()).into_owned();
}

struct Walk<'a> {
    project: &'a Project,
    root: &'a Path,
    settings: &'a Settings,
    caching: Option<&'a Caching>,
    seen: &'a Mutex<HashSet<PathBuf>>,
    looked_up: &'a Mutex<HashSet<String>>,
    scanned: &'a Mutex<Vec<Part>>,
    failures: &'a Mutex<Vec<IndexError>>,
}

impl Walk<'_> {
    fn obtain_file(&self, file: &File, reach: Reach) -> Result<Obtained, IndexError> {
        let Some(caching) = self.caching else {
            return Ok(Obtained {
                scanned: scan(file, self.settings, reach),
                fresh: Fresh::Parsed,
                entry: None,
            });
        };
        let key = Key {
            name: file_name(file),
            reach,
            stamp: Stamp::Content(digest(file.contents.as_ref())),
        };
        if let Some(obtained) = caching.reuse(&key) {
            return Ok(obtained);
        }
        return Obtained::encoded(key, scan(file, self.settings, reach));
    }

    fn obtain_path(&self, path: &Path) -> Result<Obtained, IndexError> {
        let reach = Reach::Declarations;
        if let Some(caching) = self.caching
            && caching.vendor
            && self.project.is_vendored(path)
        {
            let key = Key {
                name: logical_name(self.root, path),
                reach,
                stamp: Stamp::Vendor,
            };
            if let Some(obtained) = caching.reuse(&key) {
                return Ok(obtained);
            }
            let file = load(self.project, path)?;
            return Obtained::encoded(key, scan(&file, self.settings, reach));
        }
        return self.obtain_file(&load(self.project, path)?, reach);
    }

    fn visit<'scope>(&'scope self, scope: &rayon::Scope<'scope>, visit: Visit) {
        let obtained = match &visit {
            Visit::Source(file) => self.obtain_file(file, Reach::Everything),
            Visit::Loaded(file) => self.obtain_file(file, Reach::Declarations),
            Visit::Path(path) => self.obtain_path(path),
        };
        let Obtained {
            scanned:
                Scanned {
                    metadata,
                    references,
                    hooks,
                },
            fresh,
            entry,
        } = match obtained {
            Ok(obtained) => obtained,
            Err(error) => {
                self.failures
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(error);
                return;
            }
        };
        let unseen: Vec<String> = references
            .iter()
            .filter(|class| return claim(self.looked_up, class.to_ascii_lowercase()))
            .cloned()
            .collect();
        let (name, origin) = match visit {
            Visit::Source(file) => (file.name.to_vec(), Origin::Source(file, references)),
            Visit::Loaded(file) => (file.name.to_vec(), Origin::Declarations),
            Visit::Path(path) => (
                logical_name(self.root, &path).into_bytes(),
                Origin::Declarations,
            ),
        };
        self.scanned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Part {
                name,
                metadata,
                hooks,
                origin,
                fresh,
                entry,
            });
        let unseen = unseen.into_iter();
        for path in unseen.filter_map(|class| return self.project.class_file(&class)) {
            if claim(self.seen, path.clone()) {
                scope.spawn(move |scope| self.visit(scope, Visit::Path(path)));
            }
        }
    }
}

pub(crate) fn scan(file: &File, settings: &Settings, reach: Reach) -> Scanned {
    let arena = LocalArena::new();
    let program = parse_file(&arena, file);
    let names = NameResolver::new(&arena).resolve(program);
    let references = referenced_classes(file.contents.as_ref(), program, &names, reach);
    let mut collecting = laravel::HookCollecting::default();
    if reach == Reach::Everything {
        laravel::HookCollector { names: &names }.walk_program(program, &mut collecting);
    }
    let mut metadata = scan_program(&arena, file, program, &names, settings.version);
    if reach == Reach::Everything {
        metadata.set_file_signature(file.file_id(), build_file_signature(file, program, &names));
    }
    return Scanned {
        metadata,
        references,
        hooks: collecting.hooks,
    };
}

enum Origin {
    Source(File, BTreeSet<String>),
    Declarations,
}

struct Part {
    name: Vec<u8>,
    metadata: CodebaseMetadata,
    hooks: Hooks,
    origin: Origin,
    fresh: Fresh,
    entry: Option<(Key, Vec<u8>)>,
}

fn scan_reachable(
    project: &Project,
    settings: &Settings,
    caching: Option<&Caching>,
) -> Result<Vec<Part>, IndexError> {
    let root = project.root();
    let sources: Vec<File> = project
        .source_files()
        .par_iter()
        .map(|path| return load(project, path))
        .collect::<Result<_, _>>()?;
    let seen: Mutex<HashSet<PathBuf>> = Mutex::new(project.source_files().into_iter().collect());
    let looked_up: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
    let always: Vec<PathBuf> = project
        .always()
        .iter()
        .filter(|path| return claim(&seen, (*path).clone()))
        .cloned()
        .collect();
    let failures: Mutex<Vec<IndexError>> = Mutex::new(Vec::new());
    let scanned: Mutex<Vec<Part>> = Mutex::new(Vec::new());
    let walk = Walk {
        project,
        root,
        settings,
        caching,
        seen: &seen,
        looked_up: &looked_up,
        scanned: &scanned,
        failures: &failures,
    };
    rayon::scope(|scope| {
        for file in sources {
            scope.spawn(|scope| walk.visit(scope, Visit::Source(file)));
        }
        for path in always {
            scope.spawn(|scope| walk.visit(scope, Visit::Path(path)));
        }
        scope.spawn(|scope| walk.visit(scope, Visit::Loaded(laravel::aliases_stub(root))));
    });
    if let Some(failure) = failures
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .into_iter()
        .min_by(|left, right| return left.path().cmp(&right.path()))
    {
        return Err(failure);
    }
    let mut parts = scanned
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    parts.sort_by(|left, right| return left.name.cmp(&right.name));
    return Ok(parts);
}

pub(crate) fn merged_hooks(sources: &[Source]) -> Hooks {
    let mut hooks = Hooks::default();
    for source in sources {
        hooks.extend(source.record.hooks.clone());
    }
    return hooks;
}

fn lock_digest(project: &Project) -> Result<Option<Digest>, IndexError> {
    let path = project.lock_file();
    return match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(digest(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(IndexError::Read { path, error }),
    };
}

impl Index {
    pub fn build(project: &Project) -> Result<Self, IndexError> {
        let settings = Settings::default();
        let parts = scan_reachable(project, &settings, None)?;
        return Self::assemble(project, settings, parts);
    }

    pub fn build_cached(project: &Project, previous: ScanCache) -> Result<Build, IndexError> {
        let settings = Settings::default();
        let lock = lock_digest(project)?;
        let mut cache = previous.emptied(lock);
        let (previous_entries, previous_vendor) = (previous.entries.len(), previous.vendor);
        let caching = Caching::new(previous, lock);
        let mut parts = scan_reachable(project, &settings, Some(&caching))?;
        let mut parsed = 0_usize;
        for part in &mut parts {
            if part.fresh == Fresh::Parsed {
                parsed = parsed.saturating_add(1);
            }
            if let Some((key, bytes)) = part.entry.take() {
                cache.entries.insert(key, bytes);
            }
        }
        let index = Self::assemble(project, settings, parts)?;
        let changed = parsed > 0
            || cache.entries.len() != previous_entries
            || cache.vendor != previous_vendor;
        return Ok(Build {
            index,
            cache,
            parsed,
            changed,
        });
    }

    fn assemble(
        project: &Project,
        settings: Settings,
        parts: Vec<Part>,
    ) -> Result<Self, IndexError> {
        let root = project.root().to_path_buf();
        let scanned = parts.len();
        let Prelude {
            mut metadata,
            mut symbol_references,
            ..
        } = Prelude::decode(PRELUDE)
            .map_err(|error| return IndexError::Prelude(error.to_string()))?;
        let mut owned: Vec<(File, CodebaseMetadata, BTreeSet<String>, Hooks)> = Vec::new();
        for part in parts {
            match part.origin {
                Origin::Source(file, references) => {
                    metadata.extend_ref(&part.metadata);
                    owned.push((file, part.metadata, references, part.hooks));
                }
                Origin::Declarations => metadata.extend(part.metadata),
            }
        }
        let sources: Vec<Source> = owned
            .into_iter()
            .map(|(file, part, references, hooks)| {
                let record = Record {
                    keys: part.extract_owned_keys(&metadata),
                    signature: signature(&part),
                    references,
                    hooks,
                };
                return Source::new(file, record);
            })
            .collect();
        populate_codebase(
            &mut metadata,
            &mut symbol_references,
            mago_word::WordSet::default(),
            HashSet::default(),
        );
        let mut registry = PluginRegistry::with_library_providers();
        let files: Vec<&File> = sources.iter().map(|source| return &source.file).collect();
        let facts = laravel::apply(&mut metadata, &registry, &settings, &files);
        registry.register_method_provider(facts.provider);
        let forwards = facts.forwards;
        let trait_users = trait_users(&metadata);
        let hooks = merged_hooks(&sources);
        let positions = sources
            .iter()
            .enumerate()
            .map(|(position, source)| return (source.path.clone(), position))
            .collect();
        let stats = IndexStats {
            scanned,
            sources: sources.len(),
            models: facts.models,
            facades: facts.facades,
        };
        return Ok(Self {
            metadata,
            registry,
            settings,
            sources,
            forwards,
            hooks,
            symbol_references,
            trait_users,
            untyped: facts.untyped,
            positions,
            root,
            stats,
        });
    }

    pub const fn stats(&self) -> IndexStats {
        return self.stats;
    }

    pub fn root(&self) -> &Path {
        return &self.root;
    }
}
