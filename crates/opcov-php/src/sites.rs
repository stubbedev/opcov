use mago_codex::metadata::CodebaseMetadata;
use mago_names::ResolvedNames;
use mago_span::{HasSpan, Span};
use mago_syntax::cst::{
    AnonymousClass, Argument, ArgumentList, Class, ClassConstantAccess, ClassLikeConstantSelector,
    ClassLikeMemberSelector, ConstantAccess, Enum, Expression, FunctionCall, Identifier,
    Instantiation, LocalIdentifier, MethodCall, NullSafeMethodCall, NullSafePropertyAccess,
    PropertyAccess, StaticMethodCall, StaticPropertyAccess, Trait, Variable,
};
use mago_syntax::walker::Walker;

use crate::syntax::class_or_string;
use crate::types::Dispatch;

const CLASS_CONSTANT: &[u8] = b"class";
const THIS: &[u8] = b"$this";

#[derive(Debug, Clone)]
pub(crate) enum ClassReference {
    Named(String),
    Own,
    Late,
    Parent(Option<String>),
    UsersParent,
    Expression(Span),
}

impl ClassReference {
    pub(crate) const fn dispatch(&self) -> Dispatch {
        return match self {
            Self::Named(_) | Self::Own | Self::Parent(_) | Self::UsersParent => Dispatch::Exact,
            Self::Late | Self::Expression(_) => Dispatch::Late,
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Receiver {
    pub(crate) span: Span,
    pub(crate) is_this: bool,
}

impl Receiver {
    fn of(object: &Expression<'_>) -> Self {
        return Self {
            span: object.span(),
            is_this: matches!(object, Expression::Variable(Variable::Direct(variable)) if variable.name == THIS),
        };
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FirstArgument {
    pub(crate) span: Span,
    pub(crate) literal: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum Site {
    Function {
        candidates: Vec<String>,
    },
    Constant {
        candidates: Vec<String>,
    },
    Method {
        receiver: Receiver,
        member: Option<String>,
        first_argument: Option<FirstArgument>,
    },
    Static {
        class: ClassReference,
        member: Option<String>,
        first_argument: Option<FirstArgument>,
    },
    New {
        class: ClassReference,
    },
    ClassConstant {
        class: ClassReference,
        member: String,
    },
    Property {
        receiver: Receiver,
        member: String,
    },
    StaticProperty {
        class: ClassReference,
        member: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Located {
    pub(crate) site: Site,
    pub(crate) offset: u32,
    pub(crate) class: Option<String>,
    pub(crate) in_trait: bool,
}

#[derive(Clone, Default)]
struct Frame {
    class: Option<String>,
    parent: Option<String>,
    in_trait: bool,
}

#[derive(Default)]
pub(crate) struct Sites {
    frames: Vec<Frame>,
    pub(crate) found: Vec<Located>,
}

impl Sites {
    fn frame(&self) -> Frame {
        return self.frames.last().cloned().unwrap_or_default();
    }
}

pub(crate) struct Collector<'n, 'arena> {
    pub(crate) names: &'n ResolvedNames<'arena>,
    pub(crate) codebase: &'n CodebaseMetadata,
}

fn text(bytes: &[u8]) -> String {
    return String::from_utf8_lossy(bytes).into_owned();
}

fn selector(selector: &ClassLikeMemberSelector<'_>) -> Option<String> {
    return match selector {
        ClassLikeMemberSelector::Identifier(identifier) => Some(text(identifier.value)),
        ClassLikeMemberSelector::Variable(_)
        | ClassLikeMemberSelector::Expression(_)
        | ClassLikeMemberSelector::Missing(_) => None,
    };
}

impl Collector<'_, '_> {
    fn resolved(&self, identifier: &Identifier<'_>) -> String {
        return text(self.names.get(identifier));
    }

    fn first_argument(&self, arguments: &ArgumentList<'_>) -> Option<FirstArgument> {
        let value = arguments.arguments.first().map(|argument| {
            return match argument {
                Argument::Positional(positional) => positional.value,
                Argument::Named(named) => named.value,
            };
        })?;
        return Some(FirstArgument {
            span: value.span(),
            literal: class_or_string(self.names, value),
        });
    }

    fn class_reference(&self, expression: &Expression<'_>, sites: &Sites) -> ClassReference {
        return match expression {
            Expression::Identifier(identifier) => ClassReference::Named(self.resolved(identifier)),
            Expression::Self_(_) => ClassReference::Own,
            Expression::Static(_) => ClassReference::Late,
            Expression::Parent(_) => {
                let frame = sites.frame();
                if frame.in_trait {
                    ClassReference::UsersParent
                } else {
                    ClassReference::Parent(frame.parent)
                }
            }
            other => ClassReference::Expression(other.span()),
        };
    }

    fn candidates(&self, identifier: &Identifier<'_>) -> Vec<String> {
        let mut candidates = vec![self.resolved(identifier)];
        if let Identifier::Local(local) = identifier {
            candidates.push(text(local.value));
        }
        return candidates;
    }

    fn original(&self, class: &str) -> String {
        return self.codebase.get_class_like(class.as_bytes()).map_or_else(
            || return class.to_owned(),
            |metadata| return metadata.original_name.to_string(),
        );
    }

    fn enter(&self, name: &LocalIdentifier<'_>, sites: &mut Sites, in_trait: bool) {
        let resolved = text(self.names.resolve(name).unwrap_or(name.value));
        let parent = self
            .codebase
            .get_class_like(resolved.as_bytes())
            .and_then(|metadata| return metadata.direct_parent_class)
            .map(|parent| return self.original(&parent.to_string()));
        sites.frames.push(Frame {
            class: Some(self.original(&resolved)),
            parent,
            in_trait,
        });
    }

    fn push(sites: &mut Sites, site: Site, span: Span) {
        let frame = sites.frame();
        sites.found.push(Located {
            site,
            offset: span.start.offset,
            class: frame.class,
            in_trait: frame.in_trait,
        });
    }
}

impl<'ast, 'arena> Walker<'ast, 'arena, Sites> for Collector<'_, 'arena> {
    fn walk_in_class(&self, class: &'ast Class<'arena>, sites: &mut Sites) {
        self.enter(&class.name, sites, false);
    }

    fn walk_out_class(&self, _: &'ast Class<'arena>, sites: &mut Sites) {
        sites.frames.pop();
    }

    fn walk_in_enum(&self, enumeration: &'ast Enum<'arena>, sites: &mut Sites) {
        self.enter(&enumeration.name, sites, false);
    }

    fn walk_out_enum(&self, _: &'ast Enum<'arena>, sites: &mut Sites) {
        sites.frames.pop();
    }

    fn walk_in_trait(&self, used: &'ast Trait<'arena>, sites: &mut Sites) {
        self.enter(&used.name, sites, true);
    }

    fn walk_out_trait(&self, _: &'ast Trait<'arena>, sites: &mut Sites) {
        sites.frames.pop();
    }

    fn walk_in_anonymous_class(&self, class: &'ast AnonymousClass<'arena>, sites: &mut Sites) {
        let parent = class
            .extends
            .as_ref()
            .and_then(|extends| return extends.types.first())
            .map(|identifier| return self.resolved(identifier));
        sites.frames.push(Frame {
            class: parent.clone(),
            parent,
            in_trait: false,
        });
    }

    fn walk_out_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, sites: &mut Sites) {
        sites.frames.pop();
    }

    fn walk_in_function_call(&self, call: &'ast FunctionCall<'arena>, sites: &mut Sites) {
        if let Expression::Identifier(identifier) = call.function {
            Self::push(
                sites,
                Site::Function {
                    candidates: self.candidates(identifier),
                },
                call.span(),
            );
        }
    }

    fn walk_in_constant_access(&self, access: &'ast ConstantAccess<'arena>, sites: &mut Sites) {
        Self::push(
            sites,
            Site::Constant {
                candidates: self.candidates(&access.name),
            },
            access.span(),
        );
    }

    fn walk_in_method_call(&self, call: &'ast MethodCall<'arena>, sites: &mut Sites) {
        let site = Site::Method {
            receiver: Receiver::of(call.object),
            member: selector(&call.method),
            first_argument: self.first_argument(&call.argument_list),
        };
        Self::push(sites, site, call.span());
    }

    fn walk_in_null_safe_method_call(
        &self,
        call: &'ast NullSafeMethodCall<'arena>,
        sites: &mut Sites,
    ) {
        let site = Site::Method {
            receiver: Receiver::of(call.object),
            member: selector(&call.method),
            first_argument: self.first_argument(&call.argument_list),
        };
        Self::push(sites, site, call.span());
    }

    fn walk_in_static_method_call(&self, call: &'ast StaticMethodCall<'arena>, sites: &mut Sites) {
        let site = Site::Static {
            class: self.class_reference(call.class, sites),
            member: selector(&call.method),
            first_argument: self.first_argument(&call.argument_list),
        };
        Self::push(sites, site, call.span());
    }

    fn walk_in_instantiation(&self, new: &'ast Instantiation<'arena>, sites: &mut Sites) {
        let site = Site::New {
            class: self.class_reference(new.class, sites),
        };
        Self::push(sites, site, new.span());
    }

    fn walk_in_class_constant_access(
        &self,
        access: &'ast ClassConstantAccess<'arena>,
        sites: &mut Sites,
    ) {
        let ClassLikeConstantSelector::Identifier(identifier) = &access.constant else {
            return;
        };
        if identifier.value.eq_ignore_ascii_case(CLASS_CONSTANT) {
            return;
        }
        let site = Site::ClassConstant {
            class: self.class_reference(access.class, sites),
            member: text(identifier.value),
        };
        Self::push(sites, site, access.span());
    }

    fn walk_in_property_access(&self, access: &'ast PropertyAccess<'arena>, sites: &mut Sites) {
        if let Some(member) = selector(&access.property) {
            Self::push(
                sites,
                Site::Property {
                    receiver: Receiver::of(access.object),
                    member,
                },
                access.span(),
            );
        }
    }

    fn walk_in_null_safe_property_access(
        &self,
        access: &'ast NullSafePropertyAccess<'arena>,
        sites: &mut Sites,
    ) {
        if let Some(member) = selector(&access.property) {
            Self::push(
                sites,
                Site::Property {
                    receiver: Receiver::of(access.object),
                    member,
                },
                access.span(),
            );
        }
    }

    fn walk_in_static_property_access(
        &self,
        access: &'ast StaticPropertyAccess<'arena>,
        sites: &mut Sites,
    ) {
        if let Variable::Direct(direct) = &access.property {
            let member = text(direct.name).trim_start_matches('$').to_owned();
            let site = Site::StaticProperty {
                class: self.class_reference(access.class, sites),
                member,
            };
            Self::push(sites, site, access.span());
        }
    }
}
