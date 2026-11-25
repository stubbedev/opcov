use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_analyzer::plugin::PluginRegistry;
use mago_analyzer::settings::Settings;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::populator::populate_codebase;
use mago_codex::scanner::scan_program;
use mago_database::file::File;
use mago_names::resolver::NameResolver;
use mago_prelude::Prelude;
use mago_syntax::parser::parse_file;
use mago_syntax::walker::Walker;
use std::sync::{Mutex, OnceLock};

use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

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
        };
    }
}

impl std::error::Error for IndexError {}

impl IndexError {
    fn path(&self) -> Option<&Path> {
        return match self {
            Self::Read { path, .. } => Some(path),
            Self::Prelude(_) => None,
        };
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
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
    pub(crate) trait_users: TraitUsers,
    pub(crate) positions: HashMap<String, usize>,
    pub(crate) untyped: Vec<Untyped>,
    root: PathBuf,
    stats: IndexStats,
}

pub(crate) struct Source {
    pub(crate) file: File,
    pub(crate) path: String,
    pub(crate) facts: OnceLock<FileFacts>,
}

impl Source {
    fn new(file: File) -> Self {
        let path = String::from_utf8_lossy(file.name.as_ref()).into_owned();
        return Self {
            file,
            path,
            facts: OnceLock::new(),
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

pub(crate) fn load(root: &Path, path: &Path) -> Result<File, IndexError> {
    let content = std::fs::read(path).map_err(|error| {
        return IndexError::Read {
            path: path.to_path_buf(),
            error,
        };
    })?;
    let name = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    return Ok(File::ephemeral(
        Cow::Owned(name.into_bytes()),
        Cow::Owned(content),
    ));
}

struct Scanned {
    metadata: CodebaseMetadata,
    references: BTreeSet<String>,
    hooks: Hooks,
}

fn claim<T: Eq + std::hash::Hash>(seen: &Mutex<HashSet<T>>, item: T) -> bool {
    return seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(item);
}

struct Walk<'a> {
    project: &'a Project,
    root: &'a Path,
    settings: &'a Settings,
    seen: &'a Mutex<HashSet<PathBuf>>,
    looked_up: &'a Mutex<HashSet<String>>,
    scanned: &'a Mutex<Vec<Part>>,
    failures: &'a Mutex<Vec<IndexError>>,
}

impl Walk<'_> {
    fn visit<'scope>(&'scope self, scope: &rayon::Scope<'scope>, file: &File, reach: Reach) {
        let Scanned {
            metadata,
            references,
            hooks,
        } = scan(file, self.settings, reach);
        self.scanned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((file.name.to_vec(), metadata, hooks));
        let unseen = references
            .into_iter()
            .filter(|class| return claim(self.looked_up, class.to_ascii_lowercase()));
        for path in unseen.filter_map(|class| return self.project.class_file(&class)) {
            if claim(self.seen, path.clone()) {
                scope.spawn(move |scope| match load(self.root, &path) {
                    Ok(file) => self.visit(scope, &file, Reach::Declarations),
                    Err(error) => self
                        .failures
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(error),
                });
            }
        }
    }
}

fn scan(file: &File, settings: &Settings, reach: Reach) -> Scanned {
    let arena = LocalArena::new();
    let program = parse_file(&arena, file);
    let names = NameResolver::new(&arena).resolve(program);
    let references = referenced_classes(file.contents.as_ref(), program, &names, reach);
    let mut collecting = laravel::HookCollecting::default();
    if reach == Reach::Everything {
        laravel::HookCollector { names: &names }.walk_program(program, &mut collecting);
    }
    let metadata = scan_program(&arena, file, program, &names, settings.version);
    return Scanned {
        metadata,
        references,
        hooks: collecting.hooks,
    };
}

type Part = (Vec<u8>, CodebaseMetadata, Hooks);

fn scan_reachable(
    project: &Project,
    root: &Path,
    settings: &Settings,
    sources: &[File],
) -> Result<Vec<Part>, IndexError> {
    let seen: Mutex<HashSet<PathBuf>> = Mutex::new(project.source_files().into_iter().collect());
    let looked_up: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
    let extra: Vec<File> = project
        .always()
        .iter()
        .filter(|path| return claim(&seen, (*path).clone()))
        .map(|path| return load(root, path))
        .chain(std::iter::once(Ok(laravel::aliases_stub(root))))
        .collect::<Result<_, _>>()?;
    let failures: Mutex<Vec<IndexError>> = Mutex::new(Vec::new());
    let scanned: Mutex<Vec<Part>> = Mutex::new(Vec::new());
    let walk = Walk {
        project,
        root,
        settings,
        seen: &seen,
        looked_up: &looked_up,
        scanned: &scanned,
        failures: &failures,
    };
    rayon::scope(|scope| {
        for file in sources {
            scope.spawn(|scope| walk.visit(scope, file, Reach::Everything));
        }
        for file in &extra {
            scope.spawn(|scope| walk.visit(scope, file, Reach::Declarations));
        }
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
    parts.sort_by(|left, right| return left.0.cmp(&right.0));
    return Ok(parts);
}

impl Index {
    pub fn build(project: &Project) -> Result<Self, IndexError> {
        let settings = Settings::default();
        let root = project.root().to_path_buf();
        let sources: Vec<File> = project
            .source_files()
            .par_iter()
            .map(|path| return load(&root, path))
            .collect::<Result<_, _>>()?;
        let parts = scan_reachable(project, &root, &settings, &sources)?;
        let scanned = parts.len();
        let Prelude {
            mut metadata,
            mut symbol_references,
            ..
        } = Prelude::decode(PRELUDE)
            .map_err(|error| return IndexError::Prelude(error.to_string()))?;
        let mut hooks = Hooks::default();
        for (_, part, found) in parts {
            metadata.extend(part);
            hooks.extend(found);
        }
        populate_codebase(
            &mut metadata,
            &mut symbol_references,
            mago_word::WordSet::default(),
            HashSet::default(),
        );
        let mut registry = PluginRegistry::with_library_providers();
        let facts = laravel::apply(&mut metadata, &registry, &settings, &sources);
        registry.register_method_provider(facts.provider);
        let forwards = facts.forwards;
        let trait_users = trait_users(&metadata);
        let sources: Vec<Source> = sources.into_iter().map(Source::new).collect();
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
