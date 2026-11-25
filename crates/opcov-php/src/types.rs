use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::atomic::scalar::TScalar;
use mago_codex::ttype::atomic::scalar::class_like_string::TClassLikeString;
use mago_codex::ttype::union::TUnion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Classes {
    Known(Vec<String>),
    Unknown,
}

impl Classes {
    pub(crate) fn as_known(&self) -> Option<&[String]> {
        return match self {
            Self::Known(classes) => Some(classes),
            Self::Unknown => None,
        };
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
                for extra in named
                    .intersection_types
                    .iter()
                    .flat_map(|types| return types.iter())
                {
                    self.atomic(extra, this, reading);
                }
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
    if collected.classes.is_empty() && collected.unknown {
        return Classes::Unknown;
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
