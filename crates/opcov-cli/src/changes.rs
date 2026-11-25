use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};

use opcov_core::cfg::Line;
use opcov_core::report::{PathBase, SourcePath};

mod diff;
mod list;
mod revert;

pub(crate) use diff::{Content, FilePatch, Hunk, LineKind, Patch, parse_diff};
pub(crate) use list::parse_list;
pub(crate) use revert::original;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LineRange {
    first: Line,
    last: Line,
}

impl LineRange {
    pub(crate) fn new(first: Line, last: Line) -> Option<Self> {
        return (first <= last).then_some(Self { first, last });
    }

    pub(crate) const fn single(line: Line) -> Self {
        return Self {
            first: line,
            last: line,
        };
    }

    pub(crate) fn lines(self) -> impl Iterator<Item = Line> {
        return (self.first.get()..=self.last.get()).filter_map(Line::new);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Change {
    Lines(LineRange),
    Deletion { before: Line },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileChange {
    Whole,
    Parts(BTreeSet<Change>),
}

impl FileChange {
    fn absorb(&mut self, other: Self) {
        match (self, other) {
            (Self::Whole, _) => {}
            (this @ Self::Parts(_), Self::Whole) => *this = Self::Whole,
            (Self::Parts(mine), Self::Parts(theirs)) => mine.extend(theirs),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChangeSet(BTreeMap<PathBuf, FileChange>);

impl ChangeSet {
    pub(crate) fn add(&mut self, path: PathBuf, change: FileChange) {
        match self.0.get_mut(&path) {
            Some(existing) => existing.absorb(change),
            None => {
                self.0.insert(path, change);
            }
        }
    }

    pub(crate) fn merge(&mut self, other: Self) {
        for (path, change) in other.0 {
            self.add(path, change);
        }
    }

    pub(crate) fn of_patch(diff: &Patch) -> Self {
        let mut set = Self::default();
        for file in diff.files() {
            let (Some(path), Content::Text(hunks)) = (&file.new, &file.content) else {
                continue;
            };
            let changes: BTreeSet<Change> = hunks.iter().flat_map(hunk_changes).collect();
            if !changes.is_empty() {
                set.add(path.clone(), FileChange::Parts(changes));
            }
        }
        return set;
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = (&Path, &FileChange)> {
        return self
            .0
            .iter()
            .map(|(path, change)| return (path.as_path(), change));
    }
}

#[derive(Debug, Clone, Copy)]
enum Block {
    Unchanged,
    Removed { before: Line },
    Added,
}

fn hunk_changes(hunk: &Hunk) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut next = hunk.new_first;
    let mut block = Block::Unchanged;
    let close = |block: Block, changes: &mut Vec<Change>| {
        if let Block::Removed { before } = block {
            changes.push(Change::Deletion { before });
        }
    };
    for line in &hunk.lines {
        match line.kind {
            LineKind::Added => {
                changes.push(Change::Lines(LineRange::single(next)));
                block = Block::Added;
                next = Line::new(next.get().saturating_add(1)).unwrap_or(next);
            }
            LineKind::Removed => {
                if matches!(block, Block::Unchanged) {
                    block = Block::Removed { before: next };
                }
            }
            LineKind::Context => {
                close(block, &mut changes);
                block = Block::Unchanged;
                next = Line::new(next.get().saturating_add(1)).unwrap_or(next);
            }
        }
    }
    close(block, &mut changes);
    return changes;
}

pub(crate) fn base_change(hunks: &[Hunk]) -> FileChange {
    let mut changes = BTreeSet::new();
    for hunk in hunks {
        let mut old = hunk.old_first;
        let mut replacing = false;
        for line in &hunk.lines {
            match line.kind {
                LineKind::Removed => {
                    changes.insert(Change::Lines(LineRange::single(old)));
                    replacing = true;
                    old = Line::new(old.get().saturating_add(1)).unwrap_or(old);
                }
                LineKind::Added => {
                    if !replacing {
                        changes.insert(Change::Deletion { before: old });
                    }
                }
                LineKind::Context => {
                    replacing = false;
                    old = Line::new(old.get().saturating_add(1)).unwrap_or(old);
                }
            }
        }
    }
    return FileChange::Parts(changes);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeError {
    pub(crate) source: String,
    pub(crate) line: usize,
    pub(crate) message: String,
}

impl fmt::Display for ChangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "{} line {}: {}",
            self.source, self.line, self.message
        );
    }
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other @ (Component::Prefix(_) | Component::RootDir | Component::Normal(_)) => {
                normal.push(other);
            }
        }
    }
    return normal;
}

pub(crate) fn resolve(paths: &PathBase, base: &Path, changed: &Path) -> Option<SourcePath> {
    let absolute = normalize(&base.join(changed));
    let relative = match paths {
        PathBase::Absolute => absolute.as_path(),
        PathBase::Relative(root) => absolute.strip_prefix(root.to_path_buf()).ok()?,
    };
    return SourcePath::from_bytes(relative.as_os_str().as_bytes()).ok();
}
