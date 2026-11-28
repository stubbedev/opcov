use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use mago_codex::populator::populate_codebase_after_body_edit;
use mago_codex::symbol::SymbolIdentifier;
use mago_database::file::{File, FileId};
use mago_word::{WordSet, empty_word};

use crate::index::{
    Index, IndexError, Record, Scanned, Source, load, merged_hooks, scan, signature,
};
use crate::laravel;
use crate::project::Project;
use crate::references::Reach;
use crate::unaffected::declares_in;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refresh {
    Unchanged,
    Updated(Vec<String>),
    Rebuild(String),
}

enum Edit {
    Same,
    Body(usize, File, Box<Scanned>),
}

impl Index {
    fn reads(&self, project: &Project, relative: &str) -> bool {
        let absolute = project.root().join(relative);
        return project
            .scope()
            .iter()
            .any(|directory| return absolute.starts_with(directory))
            || declares_in(
                &self.metadata,
                &BTreeSet::from([FileId::new(relative.as_bytes())]),
            );
    }

    fn classify(
        &self,
        project: &Project,
        relative: &str,
    ) -> Result<Result<Edit, String>, IndexError> {
        let Some(position) = self.positions.get(relative).copied() else {
            if !self.reads(project, relative) {
                return Ok(Ok(Edit::Same));
            }
            return Ok(Err(format!("{relative} is not an indexed project source")));
        };
        let Some(source) = self.sources.get(position) else {
            return Ok(Err(format!("{relative} is not an indexed project source")));
        };
        let file = match load(project, &project.root().join(relative)) {
            Ok(file) => file,
            Err(IndexError::Read { error, .. }) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Err(format!("{relative} was deleted")));
            }
            Err(error) => return Err(error),
        };
        if file.contents == source.file.contents {
            return Ok(Ok(Edit::Same));
        }
        let scanned = scan(&file, &self.settings, Reach::Everything);
        if signature(&scanned.metadata) != source.record.signature {
            return Ok(Err(format!("{relative} changed a declaration")));
        }
        let declared = source
            .record
            .keys
            .class_like_names
            .iter()
            .map(ToString::to_string);
        if let Some(class) = declared
            .clone()
            .find(|class| return laravel::derives_facts_from_body(&self.metadata, class))
        {
            return Ok(Err(format!(
                "{relative} declares {class}, whose body the framework facts read"
            )));
        }
        let reaches_new = scanned.references.iter().any(|class| {
            return !source.record.references.contains(class)
                && !self.metadata.class_like_exists(class.as_bytes())
                && project.class_file(class).is_some();
        });
        if reaches_new {
            return Ok(Err(format!(
                "{relative} reaches a class the index has not scanned"
            )));
        }
        return Ok(Ok(Edit::Body(position, file, Box::new(scanned))));
    }

    fn apply_body_edit(&mut self, position: usize, file: File, scanned: Box<Scanned>) {
        let Some(source) = self.sources.get(position) else {
            return;
        };
        let classes: Vec<String> = scanned
            .metadata
            .class_likes
            .values()
            .map(|class| return class.original_name.to_string())
            .collect();
        self.metadata.remove_entries_by_keys(&source.record.keys);
        self.metadata.extend_ref(&scanned.metadata);
        let keys = scanned.metadata.extract_owned_keys(&self.metadata);
        let dirty: HashSet<SymbolIdentifier, _> = keys
            .function_like_keys
            .iter()
            .copied()
            .chain(
                keys.class_like_names
                    .iter()
                    .map(|name| return (*name, empty_word())),
            )
            .chain(
                keys.constant_names
                    .iter()
                    .map(|name| return (*name, empty_word())),
            )
            .collect();
        let edited: BTreeSet<_> = keys.class_like_names.iter().copied().collect();
        let safe: WordSet = self
            .metadata
            .class_likes
            .keys()
            .filter(|name| return !edited.contains(*name))
            .copied()
            .collect();
        populate_codebase_after_body_edit(
            &mut self.metadata,
            &mut self.symbol_references,
            safe,
            dirty,
        );
        self.forwards
            .extend(laravel::reinstall_models(&mut self.metadata, &classes));
        let record = Record {
            keys,
            signature: signature(&scanned.metadata),
            references: scanned.references,
            hooks: scanned.hooks,
        };
        if let Some(slot) = self.sources.get_mut(position) {
            *slot = Source::new(file, record);
        }
    }

    pub fn refresh(&mut self, project: &Project, changed: &[&Path]) -> Result<Refresh, IndexError> {
        let mut edits = Vec::new();
        for path in changed {
            let relative = path
                .strip_prefix(project.root())
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            match self.classify(project, &relative)? {
                Ok(Edit::Same) => {}
                Ok(Edit::Body(position, file, scanned)) => {
                    edits.push((relative, position, file, scanned));
                }
                Err(reason) => return Ok(Refresh::Rebuild(reason)),
            }
        }
        if edits.is_empty() {
            return Ok(Refresh::Unchanged);
        }
        let mut updated = Vec::new();
        for (relative, position, file, scanned) in edits {
            self.apply_body_edit(position, file, scanned);
            updated.push(relative);
        }
        self.hooks = merged_hooks(&self.sources);
        updated.sort();
        return Ok(Refresh::Updated(updated));
    }
}
