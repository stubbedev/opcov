use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::{panic, thread};

use opcov_php::{FileChange, Graph, Overlaid, Project, Revision};

use crate::changes::{ChangeSet, Content, FileChange as Lines, FilePatch, Patch, rebuild_base};
use crate::consumers;
use crate::failure::Failure;
use crate::gate::PHP_EXTENSION;
use crate::index;

fn is_php(path: &Path) -> bool {
    return path
        .extension()
        .is_some_and(|extension| return extension == PHP_EXTENSION);
}

fn overlay(root: &Path, patch: &Patch) -> Result<BTreeMap<PathBuf, Overlaid>, Failure> {
    let mut overlay = BTreeMap::new();
    for file in patch.files() {
        let (old, new) = (file.old.as_deref(), file.new.as_deref());
        if !old.into_iter().chain(new).any(is_php) {
            continue;
        }
        let Content::Text(hunks) = &file.content else {
            let shown = new.or(old).unwrap_or_else(|| return Path::new(""));
            return Err(Failure::failed(format!(
                "cannot rebuild the base version of {}: the diff shows a binary change",
                shown.display()
            )));
        };
        if let Some(new) = new.filter(|new| return is_php(new) && Some(*new) != old) {
            overlay.insert(new.to_path_buf(), Overlaid::Absent);
        }
        let Some(old) = old.filter(|old| return is_php(old)) else {
            continue;
        };
        let current = match new {
            Some(new) => {
                let disk = root.join(new);
                fs::read(&disk).map_err(|error| return Failure::io("read", &disk, &error))?
            }
            None => Vec::new(),
        };
        let base = rebuild_base(&old.display(), &current, hunks)?;
        overlay.insert(old.to_path_buf(), Overlaid::Contents(base));
    }
    return Ok(overlay);
}

fn failed(error: &impl ToString) -> Failure {
    return Failure::failed(error.to_string());
}

fn side_changes(
    mut set: ChangeSet,
    patch: &Patch,
    side: fn(&FilePatch) -> Option<&PathBuf>,
) -> Vec<FileChange> {
    for file in patch.files() {
        if let (Content::Text(_), Some(path)) = (&file.content, side(file)) {
            set.add(path.clone(), Lines::Parts(BTreeSet::new()));
        }
    }
    return consumers::changes(&set);
}

pub(crate) fn build(
    root: &Path,
    patch: &Patch,
    within: &[String],
    depth: u32,
) -> Result<Graph, Failure> {
    let project = Project::open(root).map_err(|error| return failed(&error))?;
    let base_project = project.clone().overlaid(overlay(root, patch)?);
    let (head_index, base_index) = thread::scope(|scope| {
        let base = scope.spawn(|| return index::build_unpersisted(&base_project));
        let head = index::build(&project);
        let base = match base.join() {
            Ok(built) => built,
            Err(payload) => panic::resume_unwind(payload),
        };
        return (head, base);
    });
    let (head_index, mut base_index) = (head_index?, base_index?);
    let head_changes = side_changes(ChangeSet::of_patch(patch), patch, |file| {
        return file.new.as_ref();
    });
    let base_changes = side_changes(ChangeSet::of_base_patch(patch), patch, |file| {
        return file.old.as_ref();
    });
    let diff: Vec<&str> = base_changes
        .iter()
        .chain(&head_changes)
        .map(|change| return change.path.as_str())
        .collect();
    base_index.share_unaffected(&head_index, &diff);
    let walked = Graph::walk(
        Revision {
            index: &base_index,
            changes: &base_changes,
        },
        Revision {
            index: &head_index,
            changes: &head_changes,
        },
        within,
        depth,
    )
    .map_err(|error| return failed(&error));
    thread::scope(|scope| {
        scope.spawn(move || return drop(base_index));
        drop(head_index);
    });
    return walked;
}

pub(crate) fn render_json(out: &mut dyn Write, graph: &Graph) -> io::Result<()> {
    serde_json::to_writer(&mut *out, graph)?;
    return writeln!(out);
}
