use std::borrow::Cow;
use std::collections::BTreeSet;

use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::atomic::object::named::TNamedObject;
use mago_codex::ttype::union::TUnion;
use mago_span::Span;
use mago_word::{ascii_lowercase_word, word};

use crate::hierarchy::Hierarchy;
use crate::sites::{ClassReference, FirstArgument, Receiver, Site};
use crate::types::{
    Classes, Dispatch, Reading, USING_CLASS, USING_PARENT, classes_of, literal_strings,
};

const BUILDER: &str = "Illuminate\\Database\\Eloquent\\Builder";
const RELATION: &str = "Illuminate\\Database\\Eloquent\\Relations\\Relation";
const MODEL: &str = "Illuminate\\Database\\Eloquent\\Model";
const BUILDER_TEMPLATE: &str = "TModel";
const RELATION_TEMPLATE: &str = "TRelatedModel";
pub(crate) const NEW: &str = "new";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference {
    Function {
        candidates: Vec<String>,
    },
    Constant {
        candidates: Vec<String>,
    },
    Method {
        member: String,
        receivers: Classes,
        query: Classes,
        names: Vec<String>,
    },
    Static {
        member: String,
        receivers: Classes,
        dispatch: Dispatch,
        names: Vec<String>,
    },
    New {
        classes: Classes,
        dispatch: Dispatch,
    },
    ClassConstant {
        member: String,
        classes: Classes,
        dispatch: Dispatch,
    },
    Property {
        member: String,
        receivers: Classes,
        dispatch: Dispatch,
    },
}

pub(crate) fn short_name(name: &str) -> &str {
    return name.rsplit('\\').next().unwrap_or(name);
}

impl Reference {
    fn lookup_names(&self) -> Vec<String> {
        return match self {
            Self::Function { candidates } | Self::Constant { candidates } => candidates
                .iter()
                .map(|candidate| return short_name(candidate).to_ascii_lowercase())
                .collect(),
            Self::Method { member, .. }
            | Self::Static { member, .. }
            | Self::ClassConstant { member, .. }
            | Self::Property { member, .. } => vec![member.to_ascii_lowercase()],
            Self::New { .. } => vec![NEW.to_owned()],
        };
    }
}

impl Reference {
    pub(crate) fn for_copy(&self, user: &str, parent: Option<&str>) -> Self {
        let copy = |classes: &Classes| return classes.for_copy(user, parent);
        return match self {
            Self::Function { .. } | Self::Constant { .. } => self.clone(),
            Self::Method {
                member,
                receivers,
                query,
                names,
            } => Self::Method {
                member: member.clone(),
                receivers: copy(receivers),
                query: copy(query),
                names: names.clone(),
            },
            Self::Static {
                member,
                receivers,
                dispatch,
                names,
            } => Self::Static {
                member: member.clone(),
                receivers: copy(receivers),
                dispatch: *dispatch,
                names: names.clone(),
            },
            Self::New { classes, dispatch } => Self::New {
                classes: copy(classes),
                dispatch: *dispatch,
            },
            Self::ClassConstant {
                member,
                classes,
                dispatch,
            } => Self::ClassConstant {
                member: member.clone(),
                classes: copy(classes),
                dispatch: *dispatch,
            },
            Self::Property {
                member,
                receivers,
                dispatch,
            } => Self::Property {
                member: member.clone(),
                receivers: copy(receivers),
                dispatch: *dispatch,
            },
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraitCopy {
    pub(crate) user: String,
    pub(crate) parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub(crate) line: u32,
    pub(crate) copies: Option<usize>,
    pub(crate) reference: Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FileFacts {
    pub(crate) resolved: Vec<Resolved>,
    pub(crate) copies: Vec<Vec<TraitCopy>>,
    pub(crate) names: BTreeSet<String>,
    pub(crate) untyped: Option<String>,
}

impl FileFacts {
    pub(crate) fn new(
        resolved: Vec<Resolved>,
        copies: Vec<Vec<TraitCopy>>,
        untyped: Option<String>,
    ) -> Self {
        let names = resolved
            .iter()
            .flat_map(|entry| return entry.reference.lookup_names())
            .collect();
        return Self {
            resolved,
            copies,
            names,
            untyped,
        };
    }

    pub(crate) fn copies_of(&self, entry: &Resolved) -> Option<&[TraitCopy]> {
        return entry
            .copies
            .and_then(|index| return self.copies.get(index))
            .map(Vec::as_slice);
    }

    pub(crate) fn mentions(&self, needles: &BTreeSet<String>) -> bool {
        return needles
            .iter()
            .any(|needle| return self.names.contains(needle));
    }

    pub(crate) fn named<'f>(
        &'f self,
        needles: &'f BTreeSet<String>,
    ) -> impl Iterator<Item = &'f Resolved> {
        return self.resolved.iter().filter(|entry| {
            return entry
                .reference
                .lookup_names()
                .iter()
                .any(|name| return needles.contains(name));
        });
    }
}

pub(crate) struct Resolver<'a> {
    pub(crate) hierarchy: Hierarchy<'a>,
    pub(crate) artifacts: Option<&'a AnalysisArtifacts>,
    pub(crate) this: Option<&'a str>,
}

impl Resolver<'_> {
    fn type_of(&self, span: &Span) -> Option<&TUnion> {
        return self.artifacts?.get_expression_type(span);
    }

    fn receiver_type(&self, receiver: &Receiver) -> Option<Cow<'_, TUnion>> {
        if let Some(recorded) = self.type_of(&receiver.span) {
            return Some(Cow::Borrowed(recorded));
        }
        let this = self.this.filter(|_| return receiver.is_this)?;
        let object = TNamedObject::new_this(word(this.as_bytes()));
        return Some(Cow::Owned(TUnion::from_atomic(TAtomic::Object(
            TObject::Named(object),
        ))));
    }

    fn objects(&self, receiver: &Receiver) -> Classes {
        return self
            .receiver_type(receiver)
            .map_or(Classes::unknown(), |union| {
                return classes_of(&union, self.this, Reading::Objects);
            });
    }

    fn users_parent(&self) -> Classes {
        if self.this == Some(USING_CLASS) {
            return Classes::Known(vec![USING_PARENT.to_owned()]);
        }
        let codebase = self.hierarchy.codebase();
        let parent = self
            .this
            .and_then(|user| return codebase.get_class_like(user.as_bytes()))
            .and_then(|user| return user.direct_parent_class)
            .and_then(|parent| return codebase.get_class_like(parent.as_bytes()));
        return parent.map_or(Classes::unknown(), |parent| {
            return Classes::Known(vec![parent.original_name.to_string()]);
        });
    }

    fn classes(&self, reference: &ClassReference) -> Classes {
        return match reference {
            ClassReference::Named(name) => Classes::Known(vec![name.clone()]),
            ClassReference::Own | ClassReference::Late => {
                self.this.map_or(Classes::unknown(), |class| {
                    return Classes::Known(vec![class.to_owned()]);
                })
            }
            ClassReference::Parent(parent) => {
                parent.as_ref().map_or(Classes::unknown(), |parent| {
                    return Classes::Known(vec![parent.clone()]);
                })
            }
            ClassReference::UsersParent => self.users_parent(),
            ClassReference::Expression(span) => {
                self.type_of(span).map_or(Classes::unknown(), |union| {
                    return classes_of(union, self.this, Reading::ClassNames);
                })
            }
        };
    }

    fn names(&self, argument: Option<&FirstArgument>) -> Vec<String> {
        let Some(argument) = argument else {
            return Vec::new();
        };
        let mut names = self
            .type_of(&argument.span)
            .map_or_else(Vec::new, literal_strings);
        names.extend(argument.literal.clone());
        names.sort();
        names.dedup();
        return names;
    }

    fn template_argument(
        &self,
        class: &str,
        parameters: Option<&[TUnion]>,
        ancestor: &str,
        template: &str,
    ) -> Option<TUnion> {
        if !self.hierarchy.is(class, ancestor) {
            return None;
        }
        let metadata = self.hierarchy.codebase().get_class_like(class.as_bytes())?;
        let own = |name: &str| {
            return metadata
                .template_types
                .get_index_of(&word(name.as_bytes()))
                .and_then(|position| return parameters?.get(position).cloned());
        };
        if let Some(bound) = own(template) {
            return Some(bound);
        }
        let resolved = metadata
            .template_extended_parameters
            .get(&ascii_lowercase_word(ancestor.as_bytes()))?
            .get(&word(template.as_bytes()))?;
        let forwarded = resolved.types.iter().find_map(|atomic| {
            return match atomic {
                TAtomic::GenericParameter(parameter) => own(&parameter.parameter_name.to_string()),
                _ => None,
            };
        });
        return Some(forwarded.unwrap_or_else(|| return resolved.clone()));
    }

    fn query_models(&self, receiver: &Receiver) -> Classes {
        let Some(union) = self.receiver_type(receiver) else {
            return Classes::unknown();
        };
        let mut open = classes_of(&union, self.this, Reading::Objects).is_open();
        let mut models = Vec::new();
        for atomic in union.types.iter() {
            let TAtomic::Object(TObject::Named(named)) = atomic else {
                continue;
            };
            for (ancestor, template) in [(BUILDER, BUILDER_TEMPLATE), (RELATION, RELATION_TEMPLATE)]
            {
                let Some(bound) = self.template_argument(
                    &named.name.to_string(),
                    named.get_type_parameters(),
                    ancestor,
                    template,
                ) else {
                    continue;
                };
                let queried = classes_of(&bound, self.this, Reading::Objects);
                let concrete: Vec<String> = queried
                    .known()
                    .iter()
                    .filter(|name| return !name.eq_ignore_ascii_case(MODEL))
                    .cloned()
                    .collect();
                open |= queried.is_open() || concrete.is_empty();
                models.extend(concrete);
            }
        }
        models.sort();
        models.dedup();
        if open {
            return Classes::Open(models);
        }
        return Classes::Known(models);
    }

    pub(crate) fn resolve(&self, site: &Site) -> Option<Reference> {
        return match site {
            Site::Function { candidates } => Some(Reference::Function {
                candidates: candidates.clone(),
            }),
            Site::Constant { candidates } => Some(Reference::Constant {
                candidates: candidates.clone(),
            }),
            Site::Method { member: None, .. } | Site::Static { member: None, .. } => None,
            Site::Method {
                receiver,
                member: Some(member),
                first_argument,
            } => Some(Reference::Method {
                member: member.clone(),
                receivers: self.objects(receiver),
                query: self.query_models(receiver),
                names: self.names(first_argument.as_ref()),
            }),
            Site::Static {
                class,
                member: Some(member),
                first_argument,
            } => Some(Reference::Static {
                member: member.clone(),
                receivers: self.classes(class),
                dispatch: class.dispatch(),
                names: self.names(first_argument.as_ref()),
            }),
            Site::New { class } => Some(Reference::New {
                classes: self.classes(class),
                dispatch: class.dispatch(),
            }),
            Site::ClassConstant { class, member } => Some(Reference::ClassConstant {
                member: member.clone(),
                classes: self.classes(class),
                dispatch: class.dispatch(),
            }),
            Site::Property { receiver, member } => Some(Reference::Property {
                member: member.clone(),
                receivers: self.objects(receiver),
                dispatch: Dispatch::Late,
            }),
            Site::StaticProperty { class, member } => Some(Reference::Property {
                member: member.clone(),
                receivers: self.classes(class),
                dispatch: class.dispatch(),
            }),
        };
    }
}
