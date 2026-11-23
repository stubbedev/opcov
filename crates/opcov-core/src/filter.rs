#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

mod error;
mod name;
mod path_set;

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

pub use error::FilterError;
pub use name::{eval_host, stream_scheme};
use path_set::PathSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathSetting {
    Collect,
    Exclude,
}

impl PathSetting {
    #[must_use]
    pub const fn ini_name(self) -> &'static str {
        return match self {
            Self::Collect => "opcov.collect_paths",
            Self::Exclude => "opcov.exclude_paths",
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseSensitivity {
    Sensitive,
    Insensitive,
}

impl CaseSensitivity {
    pub const HOST: Self = if cfg!(target_os = "macos") {
        Self::Insensitive
    } else {
        Self::Sensitive
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionConfig;

impl SelectionConfig {
    pub fn parse(
        collect: &str,
        exclude: &str,
        mut read_listfile: impl FnMut(&Path) -> Result<String, io::Error>,
    ) -> Result<UnresolvedSelection, FilterError> {
        let collect = parse_setting(PathSetting::Collect, collect, &mut read_listfile)?;
        let exclude = parse_setting(PathSetting::Exclude, exclude, &mut read_listfile)?;
        return Ok(UnresolvedSelection { collect, exclude });
    }
}

fn parse_setting(
    setting: PathSetting,
    value: &str,
    read_listfile: &mut impl FnMut(&Path) -> Result<String, io::Error>,
) -> Result<Vec<String>, FilterError> {
    let value = value.trim();
    let Some(listfile) = value.strip_prefix('@') else {
        return Ok(split_entries(value, ':'));
    };
    let listfile = Path::new(listfile.trim());
    if listfile.as_os_str().is_empty() {
        return Err(FilterError::MissingListfileName { setting });
    }
    let contents = match read_listfile(listfile) {
        Ok(contents) => contents,
        Err(source) => {
            return Err(FilterError::ListfileUnreadable {
                setting,
                listfile: listfile.to_path_buf(),
                source,
            });
        }
    };
    let entries = split_entries(&contents, '\n');
    if entries.is_empty() {
        return Err(FilterError::EmptyListfile {
            setting,
            listfile: listfile.to_path_buf(),
        });
    }
    return Ok(entries);
}

fn split_entries(text: &str, separator: char) -> Vec<String> {
    return text
        .split(separator)
        .map(str::trim)
        .filter(|entry| return !entry.is_empty())
        .map(str::to_owned)
        .collect();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedSelection {
    collect: Vec<String>,
    exclude: Vec<String>,
}

impl UnresolvedSelection {
    #[must_use]
    pub fn from_entries(
        collect: impl IntoIterator<Item = String>,
        exclude: impl IntoIterator<Item = String>,
    ) -> Self {
        return Self {
            collect: collect.into_iter().collect(),
            exclude: exclude.into_iter().collect(),
        };
    }

    pub fn entries(&self) -> impl Iterator<Item = &Path> {
        return self.collect.iter().chain(&self.exclude).map(Path::new);
    }

    #[must_use]
    pub fn collect_entries(&self) -> &[String] {
        return &self.collect;
    }

    #[must_use]
    pub fn exclude_entries(&self) -> &[String] {
        return &self.exclude;
    }

    pub fn resolve(
        self,
        mut realpath: impl FnMut(&Path) -> Result<PathBuf, io::Error>,
        case: CaseSensitivity,
    ) -> Result<Selection, FilterError> {
        let collect = resolve_setting(PathSetting::Collect, self.collect, &mut realpath, case)?;
        let exclude = resolve_setting(PathSetting::Exclude, self.exclude, &mut realpath, case)?;
        return Ok(Selection { collect, exclude });
    }
}

fn resolve_setting(
    setting: PathSetting,
    entries: Vec<String>,
    realpath: &mut impl FnMut(&Path) -> Result<PathBuf, io::Error>,
    case: CaseSensitivity,
) -> Result<PathSet, FilterError> {
    let mut set = PathSet::new(case);
    for entry in entries.into_iter().map(PathBuf::from) {
        let resolved = match realpath(&entry) {
            Ok(resolved) => resolved,
            Err(source) => {
                return Err(FilterError::EntryUnresolvable {
                    setting,
                    entry,
                    source,
                });
            }
        };
        if !is_canonical(&resolved) {
            return Err(FilterError::NotCanonical {
                setting,
                entry,
                resolved,
            });
        }
        set.insert(resolved);
    }
    return Ok(set);
}

fn is_canonical(path: &Path) -> bool {
    return path.is_absolute()
        && path.components().all(|component| {
            return !matches!(component, Component::ParentDir | Component::CurDir);
        });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    collect: PathSet,
    exclude: PathSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledFile<'a> {
    Resolved(&'a Path),
    StreamWrapper(&'a str),
    Unresolvable(&'a str),
    Eval { host: &'a Path },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Selected,
    Skipped(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    NotInCollectPaths,
    ExcludedByPath(PathBuf),
    ExcludedByPattern,
    StreamWrapper,
    Unresolvable,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::NotInCollectPaths => {
                write!(f, "not in {}", PathSetting::Collect.ini_name())
            }
            Self::ExcludedByPath(entry) => write!(
                f,
                "excluded by {} entry {}",
                PathSetting::Exclude.ini_name(),
                entry.display()
            ),
            Self::ExcludedByPattern => f.write_str("matched by opcov.exclude_pattern"),
            Self::StreamWrapper => {
                f.write_str("loaded through a stream wrapper, which is not selectable by path")
            }
            Self::Unresolvable => {
                f.write_str("its path cannot be resolved to an existing UTF-8 file name")
            }
        };
    }
}

impl Selection {
    #[must_use]
    pub const fn is_empty_collect(&self) -> bool {
        return self.collect.is_empty();
    }

    #[must_use]
    pub fn decide(&self, file: CompiledFile<'_>, pattern_excluded: bool) -> Decision {
        let path = match file {
            CompiledFile::Resolved(path) | CompiledFile::Eval { host: path } => path,
            CompiledFile::StreamWrapper(_) => return Decision::Skipped(SkipReason::StreamWrapper),
            CompiledFile::Unresolvable(_) => return Decision::Skipped(SkipReason::Unresolvable),
        };
        if self.collect.first_containing(path).is_none() {
            return Decision::Skipped(SkipReason::NotInCollectPaths);
        }
        if let Some(entry) = self.exclude.first_containing(path) {
            return Decision::Skipped(SkipReason::ExcludedByPath(entry.to_path_buf()));
        }
        if pattern_excluded {
            return Decision::Skipped(SkipReason::ExcludedByPattern);
        }
        return Decision::Selected;
    }
}
