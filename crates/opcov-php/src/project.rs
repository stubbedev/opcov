use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_database::file::File;
use mago_syntax::cst::{
    Array, ArrayElement, BinaryOperator, Expression, LegacyArray, Literal, Return, Statement,
};
use mago_syntax::parser::parse_file;

const COMPOSER_DIR: &str = "vendor/composer";
const PSR4_MAP: &str = "autoload_psr4.php";
const CLASS_MAP: &str = "autoload_classmap.php";
const FILES_MAP: &str = "autoload_files.php";
const VENDOR_DIR: &[u8] = b"$vendorDir";
const BASE_DIR: &[u8] = b"$baseDir";
const ROOT_MANIFEST: &str = "composer.json";
const LOCK_FILE: &str = "composer.lock";
const VENDOR: &str = "vendor";
const AUTOLOAD: &str = "autoload";
const AUTOLOAD_DEV: &str = "autoload-dev";
const SOURCE_EXTENSION: &str = "php";

#[derive(Debug)]
pub enum ProjectError {
    Read { path: PathBuf, error: io::Error },
    Map { path: PathBuf, reason: &'static str },
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Read { path, error } => {
                write!(formatter, "cannot read {}: {error}", path.display())
            }
            Self::Map { path, reason } => write!(
                formatter,
                "cannot read the autoload map {}: {reason}",
                path.display()
            ),
        };
    }
}

impl std::error::Error for ProjectError {}

#[derive(Debug, Clone)]
enum Value {
    Text(String),
    List(Vec<String>),
    Map(Vec<(String, Self)>),
}

struct Evaluator<'a> {
    vendor: &'a str,
    base: &'a str,
}

impl Evaluator<'_> {
    fn text(&self, expression: &Expression<'_>) -> Option<String> {
        return match expression {
            Expression::Literal(Literal::String(string)) => string
                .value
                .map(|value| return String::from_utf8_lossy(value).into_owned()),
            Expression::Variable(mago_syntax::cst::Variable::Direct(variable))
                if variable.name == VENDOR_DIR =>
            {
                Some(self.vendor.to_owned())
            }
            Expression::Variable(mago_syntax::cst::Variable::Direct(variable))
                if variable.name == BASE_DIR =>
            {
                Some(self.base.to_owned())
            }
            Expression::Binary(binary)
                if matches!(binary.operator, BinaryOperator::StringConcat(_)) =>
            {
                let mut joined = self.text(binary.lhs)?;
                joined.push_str(&self.text(binary.rhs)?);
                Some(joined)
            }
            _ => None,
        };
    }

    fn elements<'e>(expression: &'e Expression<'_>) -> Option<&'e [ArrayElement<'e>]> {
        return match expression {
            Expression::Array(Array { elements, .. })
            | Expression::LegacyArray(LegacyArray { elements, .. }) => Some(elements.as_slice()),
            _ => None,
        };
    }

    fn value(&self, expression: &Expression<'_>) -> Option<Value> {
        let Some(elements) = Self::elements(expression) else {
            return self.text(expression).map(Value::Text);
        };
        let keyed = elements
            .iter()
            .any(|element| return matches!(element, ArrayElement::KeyValue(_)));
        if !keyed {
            let values = elements
                .iter()
                .filter_map(|element| {
                    return match element {
                        ArrayElement::Value(value) => self.text(value.value),
                        _ => None,
                    };
                })
                .collect();
            return Some(Value::List(values));
        }
        let entries = elements
            .iter()
            .filter_map(|element| {
                return match element {
                    ArrayElement::KeyValue(pair) => {
                        Some((self.text(pair.key)?, self.value(pair.value)?))
                    }
                    _ => None,
                };
            })
            .collect();
        return Some(Value::Map(entries));
    }
}

fn returned_map(
    path: &Path,
    vendor: &str,
    base: &str,
) -> Result<Vec<(String, Value)>, ProjectError> {
    let content = std::fs::read(path).map_err(|error| {
        return ProjectError::Read {
            path: path.to_path_buf(),
            error,
        };
    })?;
    let name = path.to_string_lossy().into_owned();
    let file = File::ephemeral(Cow::Owned(name.into_bytes()), Cow::Owned(content));
    let arena = LocalArena::new();
    let program = parse_file(&arena, &file);
    let evaluator = Evaluator { vendor, base };
    let returned = program.statements.iter().find_map(|statement| {
        return match statement {
            Statement::Return(Return {
                value: Some(value), ..
            }) => evaluator.value(value),
            _ => None,
        };
    });
    return match returned {
        Some(Value::Map(entries)) => Ok(entries),
        Some(Value::List(values)) if values.is_empty() => Ok(Vec::new()),
        Some(Value::List(_) | Value::Text(_)) => Err(ProjectError::Map {
            path: path.to_path_buf(),
            reason: "it does not return a keyed array",
        }),
        None => Err(ProjectError::Map {
            path: path.to_path_buf(),
            reason: "it returns nothing opcov can read",
        }),
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlaid {
    Absent,
    Contents(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    overlay: BTreeMap<PathBuf, Overlaid>,
    vendor: PathBuf,
    namespaces: HashMap<String, Vec<PathBuf>>,
    classes: HashMap<String, PathBuf>,
    always: Vec<PathBuf>,
    scope: Vec<PathBuf>,
    development: Vec<PathBuf>,
}

fn lower(text: &str) -> String {
    return text.to_ascii_lowercase();
}

impl Project {
    pub fn open(root: &Path) -> Result<Self, ProjectError> {
        let composer = root.join(COMPOSER_DIR);
        let vendor_dir = root.join(VENDOR);
        let vendor = vendor_dir.to_string_lossy().into_owned();
        let base = root.to_string_lossy().into_owned();
        let namespaces: HashMap<String, Vec<PathBuf>> =
            returned_map(&composer.join(PSR4_MAP), &vendor, &base)?
                .into_iter()
                .map(|(prefix, value)| {
                    let directories = match value {
                        Value::List(directories) => {
                            directories.into_iter().map(PathBuf::from).collect()
                        }
                        Value::Text(directory) => vec![PathBuf::from(directory)],
                        Value::Map(_) => Vec::new(),
                    };
                    return (lower(&prefix), directories);
                })
                .collect();
        let classes = returned_map(&composer.join(CLASS_MAP), &vendor, &base)?
            .into_iter()
            .filter_map(|(class, value)| {
                return match value {
                    Value::Text(path) => Some((lower(&class), PathBuf::from(path))),
                    Value::List(_) | Value::Map(_) => None,
                };
            })
            .collect();
        let always = returned_map(&composer.join(FILES_MAP), &vendor, &base)?
            .into_iter()
            .filter_map(|(_, value)| {
                return match value {
                    Value::Text(path) => Some(PathBuf::from(path)),
                    Value::List(_) | Value::Map(_) => None,
                };
            })
            .collect();
        let manifest = Self::manifest(root)?;
        let scope = Self::autoload_scope(root, &manifest, AUTOLOAD);
        let development = Self::autoload_scope(root, &manifest, AUTOLOAD_DEV);
        return Ok(Self {
            root: root.to_path_buf(),
            overlay: BTreeMap::new(),
            vendor: vendor_dir,
            namespaces,
            classes,
            always,
            scope,
            development,
        });
    }

    fn manifest(root: &Path) -> Result<serde_json::Value, ProjectError> {
        let path = root.join(ROOT_MANIFEST);
        let content = std::fs::read(&path).map_err(|error| {
            return ProjectError::Read {
                path: path.clone(),
                error,
            };
        })?;
        return serde_json::from_slice(&content).map_err(|_invalid| {
            return ProjectError::Map {
                path: path.clone(),
                reason: "it is not valid JSON",
            };
        });
    }

    fn autoload_scope(root: &Path, manifest: &serde_json::Value, section: &str) -> Vec<PathBuf> {
        let autoload = manifest.get(section);
        let prefixed = ["psr-4", "psr-0"]
            .into_iter()
            .filter_map(|kind| return autoload?.get(kind)?.as_object())
            .flat_map(|prefixes| return prefixes.values())
            .flat_map(|directories| {
                return match directories {
                    serde_json::Value::String(directory) => vec![directory.as_str()],
                    serde_json::Value::Array(directories) => directories
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect(),
                    _ => Vec::new(),
                };
            });
        let listed = ["classmap", "files"]
            .into_iter()
            .filter_map(|kind| return autoload?.get(kind)?.as_array())
            .flatten()
            .filter_map(serde_json::Value::as_str);
        let mut scope: Vec<PathBuf> = prefixed
            .chain(listed)
            .map(|entry| return root.join(entry))
            .collect();
        scope.sort();
        scope.dedup();
        return scope;
    }

    #[must_use]
    pub fn with_development(mut self) -> Self {
        self.scope.extend(self.development.iter().cloned());
        self.scope.sort();
        self.scope.dedup();
        return self;
    }

    #[must_use]
    pub fn overlaid(mut self, overlay: BTreeMap<PathBuf, Overlaid>) -> Self {
        self.overlay = overlay
            .into_iter()
            .map(|(path, overlaid)| return (self.root.join(path), overlaid))
            .collect();
        return self;
    }

    pub(crate) fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        return match self.overlay.get(path) {
            Some(Overlaid::Contents(contents)) => Ok(contents.clone()),
            Some(Overlaid::Absent) => Err(io::Error::from(io::ErrorKind::NotFound)),
            None => std::fs::read(path),
        };
    }

    fn exists(&self, path: &Path) -> bool {
        return match self.overlay.get(path) {
            Some(Overlaid::Contents(_)) => true,
            Some(Overlaid::Absent) => false,
            None => path.is_file(),
        };
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        return &self.root;
    }

    pub(crate) fn is_vendored(&self, path: &Path) -> bool {
        return path.starts_with(&self.vendor);
    }

    pub(crate) fn lock_file(&self) -> PathBuf {
        return self.root.join(LOCK_FILE);
    }

    #[must_use]
    pub fn always(&self) -> &[PathBuf] {
        return &self.always;
    }

    #[must_use]
    pub fn scope(&self) -> &[PathBuf] {
        return &self.scope;
    }

    #[must_use]
    pub fn class_file(&self, class: &str) -> Option<PathBuf> {
        let class = class.trim_start_matches('\\');
        let lowered = lower(class);
        if let Some(path) = self.classes.get(&lowered) {
            return Some(path.clone());
        }
        let boundaries = lowered
            .match_indices('\\')
            .map(|(index, _)| return index)
            .rev();
        return boundaries
            .filter_map(|boundary| {
                let prefix = lowered.get(..=boundary)?;
                let rest = class.get(boundary.checked_add(1)?..)?;
                return Some((self.namespaces.get(prefix)?, rest));
            })
            .find_map(|(directories, rest)| {
                let relative = format!("{}.{SOURCE_EXTENSION}", rest.replace('\\', "/"));
                return directories
                    .iter()
                    .map(|directory| return directory.join(&relative))
                    .find(|candidate| return self.exists(candidate));
            });
    }

    #[must_use]
    pub fn source_files(&self) -> Vec<PathBuf> {
        return self.files_under(&self.scope);
    }

    #[must_use]
    pub fn all_files(&self) -> Vec<PathBuf> {
        let mut entries = self.scope.clone();
        entries.extend(self.development.iter().cloned());
        return self.files_under(&entries);
    }

    fn files_under(&self, entries: &[PathBuf]) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in entries {
            collect_sources(entry, &mut files);
        }
        if !self.overlay.is_empty() {
            files.retain(|file| return self.overlay.get(file) != Some(&Overlaid::Absent));
            files.extend(
                self.overlay
                    .iter()
                    .filter(|(path, overlaid)| {
                        return matches!(overlaid, Overlaid::Contents(_))
                            && is_source(path)
                            && entries.iter().any(|entry| return path.starts_with(entry));
                    })
                    .map(|(path, _)| return path.clone()),
            );
        }
        files.sort();
        files.dedup();
        return files;
    }
}

fn is_source(path: &Path) -> bool {
    return path
        .extension()
        .is_some_and(|extension| return extension == SOURCE_EXTENSION);
}

fn collect_sources(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if is_source(path) {
            files.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(|entry| return entry.ok().map(|entry| return entry.path()))
        .collect();
    children.sort();
    for child in children {
        collect_sources(&child, files);
    }
}
