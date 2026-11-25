use std::borrow::Cow;
use std::collections::BTreeSet;
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
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

use crate::laravel;
use crate::project::Project;
use crate::references::referenced_classes;

#[derive(Debug, Clone, Copy, Default)]
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
    pub(crate) sources: Vec<File>,
    pub(crate) forwards: laravel::Forwards,
    root: PathBuf,
    stats: IndexStats,
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

pub(crate) fn load(root: &Path, path: &Path) -> Option<File> {
    let content = std::fs::read(path).ok()?;
    let name = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    return Some(File::ephemeral(
        Cow::Owned(name.into_bytes()),
        Cow::Owned(content),
    ));
}

struct Scanned {
    metadata: CodebaseMetadata,
    references: BTreeSet<String>,
}

fn scan(file: &File, settings: &Settings) -> Scanned {
    let arena = LocalArena::new();
    let program = parse_file(&arena, file);
    let names = NameResolver::new(&arena).resolve(program);
    let references = referenced_classes(file.contents.as_ref(), program, &names);
    let metadata = scan_program(&arena, file, program, &names, settings.version);
    return Scanned {
        metadata,
        references,
    };
}

impl Index {
    #[must_use]
    pub fn build(project: &Project) -> Self {
        let settings = Settings::default();
        let root = project.root().to_path_buf();
        let sources: Vec<File> = project
            .source_files()
            .par_iter()
            .filter_map(|path| return load(&root, path))
            .collect();
        let mut seen: BTreeSet<PathBuf> = project.source_files().into_iter().collect();
        let mut extra: Vec<File> = project
            .always()
            .iter()
            .filter(|path| return seen.insert((*path).clone()))
            .filter_map(|path| return load(&root, path))
            .collect();
        extra.push(laravel::aliases_stub(&root));
        let Prelude {
            mut metadata,
            mut symbol_references,
            ..
        } = Prelude::build();
        let mut results: Vec<Scanned> = sources
            .par_iter()
            .chain(extra.par_iter())
            .map(|file| return scan(file, &settings))
            .collect();
        let mut scanned = 0_usize;
        while !results.is_empty() {
            scanned = scanned.saturating_add(results.len());
            let mut next = Vec::new();
            for result in results {
                metadata.extend(result.metadata);
                next.extend(
                    result
                        .references
                        .iter()
                        .filter_map(|class| return project.class_file(class))
                        .filter(|path| return seen.insert(path.clone())),
                );
            }
            let wave: Vec<File> = next
                .par_iter()
                .filter_map(|path| return load(&root, path))
                .collect();
            results = wave
                .par_iter()
                .map(|file| return scan(file, &settings))
                .collect();
        }
        populate_codebase(
            &mut metadata,
            &mut symbol_references,
            mago_word::WordSet::default(),
            std::collections::HashSet::default(),
        );
        let mut registry = PluginRegistry::with_library_providers();
        let facts = laravel::apply(&mut metadata, &registry, &settings, &sources);
        registry.register_method_provider(facts.provider);
        let forwards = facts.forwards;
        let stats = IndexStats {
            scanned,
            sources: sources.len(),
            models: facts.models,
            facades: facts.facades,
        };
        return Self {
            metadata,
            registry,
            settings,
            sources,
            forwards,
            root,
            stats,
        };
    }

    pub const fn stats(&self) -> IndexStats {
        return self.stats;
    }

    pub fn root(&self) -> &Path {
        return &self.root;
    }
}
