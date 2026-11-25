use std::collections::BTreeSet;

use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_codex::ttype::atomic::TAtomic;
use mago_codex::ttype::atomic::object::TObject;
use mago_codex::ttype::union::TUnion;
use mago_span::Span;
use mago_word::{ascii_lowercase_word, word};

use crate::hierarchy::Hierarchy;
use crate::sites::{ClassReference, FirstArgument, Site};
use crate::types::{Classes, Reading, classes_of, literal_strings};

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
        names: Vec<String>,
    },
    New {
        classes: Classes,
    },
    ClassConstant {
        member: String,
        classes: Classes,
    },
    Property {
        member: String,
        receivers: Classes,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub(crate) line: u32,
    pub(crate) using: Option<String>,
    pub(crate) reference: Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FileFacts {
    pub(crate) resolved: Vec<Resolved>,
    pub(crate) names: BTreeSet<String>,
    pub(crate) untyped: Option<String>,
}

impl FileFacts {
    pub(crate) fn new(resolved: Vec<Resolved>, untyped: Option<String>) -> Self {
        let names = resolved
            .iter()
            .flat_map(|entry| return entry.reference.lookup_names())
            .collect();
        return Self {
            resolved,
            names,
            untyped,
        };
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

    fn objects(&self, span: &Span) -> Classes {
        return self.type_of(span).map_or(Classes::Unknown, |union| {
            return classes_of(union, self.this, Reading::Objects);
        });
    }

    fn classes(&self, reference: &ClassReference) -> Classes {
        return match reference {
            ClassReference::Named(name) => Classes::Known(vec![name.clone()]),
            ClassReference::Current => self.this.map_or(Classes::Unknown, |class| {
                return Classes::Known(vec![class.to_owned()]);
            }),
            ClassReference::Parent(parent) => parent.as_ref().map_or(Classes::Unknown, |parent| {
                return Classes::Known(vec![parent.clone()]);
            }),
            ClassReference::Expression(span) => {
                self.type_of(span).map_or(Classes::Unknown, |union| {
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

    fn query_models(&self, span: &Span) -> Classes {
        let Some(union) = self.type_of(span) else {
            return Classes::Unknown;
        };
        if classes_of(union, self.this, Reading::Objects) == Classes::Unknown {
            return Classes::Unknown;
        }
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
                let Classes::Known(queried) = classes_of(&bound, self.this, Reading::Objects)
                else {
                    return Classes::Unknown;
                };
                let concrete: Vec<String> = queried
                    .into_iter()
                    .filter(|name| return !name.eq_ignore_ascii_case(MODEL))
                    .collect();
                if concrete.is_empty() {
                    return Classes::Unknown;
                }
                models.extend(concrete);
            }
        }
        models.sort();
        models.dedup();
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
                names: self.names(first_argument.as_ref()),
            }),
            Site::New { class } => Some(Reference::New {
                classes: self.classes(class),
            }),
            Site::ClassConstant { class, member } => Some(Reference::ClassConstant {
                member: member.clone(),
                classes: self.classes(class),
            }),
            Site::Property { receiver, member } => Some(Reference::Property {
                member: member.clone(),
                receivers: self.objects(receiver),
            }),
            Site::StaticProperty { class, member } => Some(Reference::Property {
                member: member.clone(),
                receivers: self.classes(class),
            }),
        };
    }
}
