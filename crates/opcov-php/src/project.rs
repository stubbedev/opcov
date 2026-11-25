use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
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
const SOURCE_EXTENSION: &str = "php";

#[derive(Debug)]
pub enum ProjectError {
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    Map {
        path: PathBuf,
        reason: &'static str,
    },
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

#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    namespaces: Vec<(String, Vec<PathBuf>)>,
    classes: BTreeMap<String, PathBuf>,
    always: Vec<PathBuf>,
    scope: Vec<PathBuf>,
}

fn lower(text: &str) -> String {
    return text.to_ascii_lowercase();
}

impl Project {
    pub fn open(root: &Path) -> Result<Self, ProjectError> {
        let composer = root.join(COMPOSER_DIR);
        let vendor = root.join("vendor").to_string_lossy().into_owned();
        let base = root.to_string_lossy().into_owned();
        let mut namespaces: Vec<(String, Vec<PathBuf>)> =
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
                    return (prefix, directories);
                })
                .collect();
        namespaces.sort_by(|left, right| {
            return right
                .0
                .len()
                .cmp(&left.0.len())
                .then_with(|| return left.0.cmp(&right.0));
        });
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
        let scope = Self::autoload_scope(root)?;
        return Ok(Self {
            root: root.to_path_buf(),
            namespaces,
            classes,
            always,
            scope,
        });
    }

    fn autoload_scope(root: &Path) -> Result<Vec<PathBuf>, ProjectError> {
        let path = root.join(ROOT_MANIFEST);
        let content = std::fs::read(&path).map_err(|error| {
            return ProjectError::Read {
                path: path.clone(),
                error,
            };
        })?;
        let manifest: serde_json::Value = serde_json::from_slice(&content).map_err(|_invalid| {
            return ProjectError::Map {
                path: path.clone(),
                reason: "it is not valid JSON",
            };
        })?;
        let autoload = manifest.get("autoload");
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
        return Ok(scope);
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        return &self.root;
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
        if let Some(path) = self.classes.get(&lower(class)) {
            return Some(path.clone());
        }
        return self.namespaces.iter().find_map(|(prefix, directories)| {
            let rest = class.get(prefix.len()..).filter(|_| {
                return class.len() > prefix.len()
                    && class
                        .get(..prefix.len())
                        .is_some_and(|head| return head.eq_ignore_ascii_case(prefix));
            })?;
            let relative = format!("{}.{SOURCE_EXTENSION}", rest.replace('\\', "/"));
            return directories
                .iter()
                .map(|directory| return directory.join(&relative))
                .find(|candidate| return candidate.is_file());
        });
    }

    #[must_use]
    pub fn source_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in &self.scope {
            collect_sources(entry, &mut files);
        }
        files.sort();
        files.dedup();
        return files;
    }
}

fn collect_sources(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path
            .extension()
            .is_some_and(|extension| return extension == SOURCE_EXTENSION)
        {
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
