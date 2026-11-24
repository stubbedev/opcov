use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};

use opcov_core::cfg::Line;
use opcov_core::report::{PathBase, SourcePath};

mod diff;
mod list;

pub(crate) use diff::parse_diff;
pub(crate) use list::parse_list;

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

    pub(crate) fn files(&self) -> impl Iterator<Item = (&Path, &FileChange)> {
        return self
            .0
            .iter()
            .map(|(path, change)| return (path.as_path(), change));
    }
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

fn normalize(path: &Path) -> PathBuf {
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
