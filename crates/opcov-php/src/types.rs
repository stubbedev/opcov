use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::atomic::scalar::TScalar;
use mago_codex::ttype::atomic::scalar::class_like_string::TClassLikeString;
use mago_codex::ttype::union::TUnion;

use crate::target::Resolution;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dispatch {
    Exact,
    Late,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Classes {
    Known(Vec<String>),
    Open(Vec<String>),
}

pub(crate) const USING_CLASS: &str = "\0using";
pub(crate) const USING_PARENT: &str = "\0parent";

impl Classes {
    pub(crate) fn for_copy(&self, user: &str, parent: Option<&str>) -> Self {
        let classes = self.known();
        if !classes
            .iter()
            .any(|class| return class == USING_CLASS || class == USING_PARENT)
        {
            return self.clone();
        }
        let mut substituted = Vec::with_capacity(classes.len());
        for class in classes {
            if class == USING_CLASS {
                substituted.push(user.to_owned());
            } else if class == USING_PARENT {
                let Some(parent) = parent else {
                    return Self::unknown();
                };
                substituted.push(parent.to_owned());
            } else {
                substituted.push(class.clone());
            }
        }
        substituted.sort();
        substituted.dedup();
        return self.with(substituted);
    }

    pub(crate) const fn unknown() -> Self {
        return Self::Open(Vec::new());
    }

    pub(crate) fn known(&self) -> &[String] {
        return match self {
            Self::Known(classes) | Self::Open(classes) => classes,
        };
    }

    pub(crate) const fn is_open(&self) -> bool {
        return matches!(self, Self::Open(_));
    }

    pub(crate) const fn with(&self, classes: Vec<String>) -> Self {
        return match self {
            Self::Known(_) => Self::Known(classes),
            Self::Open(_) => Self::Open(classes),
        };
    }

    pub(crate) fn matching(&self, reaches: impl Fn(&str) -> bool) -> Option<Resolution> {
        if self.known().iter().any(|class| return reaches(class)) {
            return Some(Resolution::Typed);
        }
        return self.is_open().then_some(Resolution::ByName);
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Reading {
    Objects,
    ClassNames,
}

struct Collected {
    classes: Vec<String>,
    unknown: bool,
}

impl Collected {
    fn atomic(&mut self, atomic: &TAtomic, this: Option<&str>, reading: Reading) {
        match (atomic, reading) {
            (TAtomic::Object(TObject::Named(named)), _) => {
                let name = match (named.is_static || named.is_this, this) {
                    (true, Some(this)) => this.to_owned(),
                    _ => named.name.to_string(),
                };
                self.classes.push(name);
                let mut narrowing = Self {
                    classes: Vec::new(),
                    unknown: false,
                };
                for extra in named
                    .intersection_types
                    .iter()
                    .flat_map(|types| return types.iter())
                {
                    narrowing.atomic(extra, this, reading);
                }
                self.classes.extend(narrowing.classes);
            }
            (TAtomic::Object(TObject::Enum(enumeration)), _) => {
                self.classes.push(enumeration.name.to_string());
            }
            (TAtomic::GenericParameter(parameter), _) => {
                for inner in parameter.constraint.types.iter() {
                    self.atomic(inner, this, reading);
                }
            }
            (TAtomic::Scalar(TScalar::ClassLikeString(class_string)), Reading::ClassNames) => {
                match class_string {
                    TClassLikeString::Literal { value } => self.classes.push(value.to_string()),
                    TClassLikeString::OfType { constraint, .. }
                    | TClassLikeString::Generic { constraint, .. } => {
                        self.atomic(constraint, this, Reading::Objects);
                    }
                    TClassLikeString::Any { .. } => self.unknown = true,
                }
            }
            (TAtomic::Scalar(TScalar::String(string)), Reading::ClassNames) => {
                match string.get_known_literal_value() {
                    Some(value) => self
                        .classes
                        .push(String::from_utf8_lossy(value).into_owned()),
                    None => self.unknown = true,
                }
            }
            (TAtomic::Scalar(TScalar::Generic | TScalar::ArrayKey), Reading::ClassNames) => {
                self.unknown = true;
            }
            (
                TAtomic::Null
                | TAtomic::Void
                | TAtomic::Never
                | TAtomic::Scalar(_)
                | TAtomic::Array(_)
                | TAtomic::Resource(_),
                _,
            ) => {}
            _ => self.unknown = true,
        }
    }
}

pub(crate) fn classes_of(union: &TUnion, this: Option<&str>, reading: Reading) -> Classes {
    let mut collected = Collected {
        classes: Vec::new(),
        unknown: false,
    };
    for atomic in union.types.iter() {
        collected.atomic(atomic, this, reading);
    }
    collected.classes.sort();
    collected.classes.dedup();
    if collected.unknown {
        return Classes::Open(collected.classes);
    }
    return Classes::Known(collected.classes);
}

pub(crate) fn literal_strings(union: &TUnion) -> Vec<String> {
    return union
        .types
        .iter()
        .filter_map(|atomic| {
            return match atomic {
                TAtomic::Scalar(TScalar::ClassLikeString(TClassLikeString::Literal { value })) => {
                    Some(value.to_string())
                }
                TAtomic::Scalar(TScalar::String(string)) => string
                    .get_known_literal_value()
                    .map(|value| return String::from_utf8_lossy(value).into_owned()),
                _ => None,
            };
        })
        .collect();
}
