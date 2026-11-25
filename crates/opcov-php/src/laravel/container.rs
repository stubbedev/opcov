use std::collections::BTreeMap;

use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_codex::metadata::CodebaseMetadata;
use mago_names::ResolvedNames;
use mago_span::{HasSpan, Span};
use mago_syntax::cst::{
    Argument, ArgumentList, ArrowFunction, Class, ClassLikeMemberSelector, Closure, Expression,
    FunctionLikeReturnTypeHint, Hint, Method, MethodCall, Return, StaticMethodCall,
};
use mago_syntax::walker::Walker;
use mago_word::Word;

use super::members::key;
use crate::syntax::{class_constant, string_literal};
use crate::types::{Classes, Reading, classes_of};

pub(super) const FACADE: &[u8] = b"Illuminate\\Support\\Facades\\Facade";
pub(super) const SERVICE_PROVIDER: &[u8] = b"Illuminate\\Support\\ServiceProvider";
const ACCESSOR: &[u8] = b"getFacadeAccessor";
const BINDERS: [&[u8]; 6] = [
    b"bind",
    b"singleton",
    b"scoped",
    b"instance",
    b"bindIf",
    b"singletonIf",
];

#[derive(Debug, Clone)]
enum Name {
    Key(String),
    Class(String),
}

impl Name {
    fn lookup(&self) -> String {
        return match self {
            Self::Key(name) | Self::Class(name) => name.to_ascii_lowercase(),
        };
    }
}

#[derive(Debug, Clone, Copy)]
enum Bound {
    Closure(Span),
    Expression(Span),
}

#[derive(Debug, Clone)]
enum Binding {
    Class(String),
    Deferred(Bound),
}

#[derive(Default)]
pub(super) struct Found {
    accessors: Vec<(String, Name)>,
    bindings: Vec<(Name, Binding)>,
    returns: BTreeMap<(u32, u32), Vec<Span>>,
    hinted: BTreeMap<(u32, u32), String>,
    classes: Vec<String>,
    closures: Vec<Option<(u32, u32)>>,
    in_accessor: bool,
}

pub(super) struct Collector<'n, 'arena> {
    pub(super) names: &'n ResolvedNames<'arena>,
}

const fn range(span: Span) -> (u32, u32) {
    return (span.start.offset, span.end.offset);
}

const fn argument_value<'a>(argument: &'a Argument<'a>) -> &'a Expression<'a> {
    return match argument {
        Argument::Positional(positional) => positional.value,
        Argument::Named(named) => named.value,
    };
}

impl Collector<'_, '_> {
    fn name(&self, expression: &Expression<'_>) -> Option<Name> {
        return class_constant(self.names, expression)
            .map(Name::Class)
            .or_else(|| return string_literal(expression).map(Name::Key));
    }

    fn binding(&self, arguments: &ArgumentList<'_>, found: &mut Found) {
        let mut values = arguments.arguments.iter().map(argument_value);
        let Some(name) = values.next().and_then(|first| return self.name(first)) else {
            return;
        };
        let binding = match (values.next(), &name) {
            (None, Name::Class(class)) => Binding::Class(class.clone()),
            (None, Name::Key(_)) => return,
            (Some(Expression::Closure(closure)), _) => {
                Binding::Deferred(Bound::Closure(closure.span()))
            }
            (Some(Expression::ArrowFunction(arrow)), _) => {
                Binding::Deferred(Bound::Closure(arrow.span()))
            }
            (Some(other), _) => class_constant(self.names, other).map_or_else(
                || return Binding::Deferred(Bound::Expression(other.span())),
                Binding::Class,
            ),
        };
        found.bindings.push((name, binding));
    }

    fn hint(&self, hint: Option<&FunctionLikeReturnTypeHint<'_>>) -> Option<String> {
        return match &hint?.hint {
            Hint::Identifier(identifier) => {
                Some(String::from_utf8_lossy(self.names.get(identifier)).into_owned())
            }
            _ => None,
        };
    }
}

fn binds(selector: &ClassLikeMemberSelector<'_>) -> bool {
    return matches!(selector, ClassLikeMemberSelector::Identifier(identifier) if BINDERS.iter().any(|binder| return identifier.value.eq_ignore_ascii_case(binder)));
}

impl<'ast, 'arena> Walker<'ast, 'arena, Found> for Collector<'_, 'arena> {
    fn walk_in_class(&self, class: &'ast Class<'arena>, found: &mut Found) {
        found.classes.push(
            String::from_utf8_lossy(self.names.resolve(&class.name).unwrap_or(class.name.value))
                .into_owned(),
        );
    }

    fn walk_out_class(&self, _: &'ast Class<'arena>, found: &mut Found) {
        found.classes.pop();
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, found: &mut Found) {
        found.in_accessor = method.name.value.eq_ignore_ascii_case(ACCESSOR);
        found.closures.push(None);
    }

    fn walk_out_method(&self, _: &'ast Method<'arena>, found: &mut Found) {
        found.in_accessor = false;
        found.closures.pop();
    }

    fn walk_in_closure(&self, closure: &'ast Closure<'arena>, found: &mut Found) {
        let span = range(closure.span());
        if let Some(hint) = self.hint(closure.return_type_hint.as_ref()) {
            found.hinted.insert(span, hint);
        }
        found.closures.push(Some(span));
    }

    fn walk_out_closure(&self, _: &'ast Closure<'arena>, found: &mut Found) {
        found.closures.pop();
    }

    fn walk_in_arrow_function(&self, arrow: &'ast ArrowFunction<'arena>, found: &mut Found) {
        let span = range(arrow.span());
        if let Some(hint) = self.hint(arrow.return_type_hint.as_ref()) {
            found.hinted.insert(span, hint);
        }
        found
            .returns
            .entry(span)
            .or_default()
            .push(arrow.expression.span());
    }

    fn walk_in_return(&self, statement: &'ast Return<'arena>, found: &mut Found) {
        let Some(value) = statement.value else {
            return;
        };
        if let Some(closure) = found.closures.last().copied().flatten() {
            found.returns.entry(closure).or_default().push(value.span());
            return;
        }
        if let (true, Some(class), Some(name)) = (
            found.in_accessor,
            found.classes.last().cloned(),
            self.name(value),
        ) {
            found.accessors.push((class, name));
        }
    }

    fn walk_in_method_call(&self, call: &'ast MethodCall<'arena>, found: &mut Found) {
        if binds(&call.method) {
            self.binding(&call.argument_list, found);
        }
    }

    fn walk_in_static_method_call(&self, call: &'ast StaticMethodCall<'arena>, found: &mut Found) {
        if binds(&call.method) {
            self.binding(&call.argument_list, found);
        }
    }
}

fn single_class(artifacts: &AnalysisArtifacts, span: &Span) -> Option<String> {
    let Classes::Known(classes) =
        classes_of(artifacts.get_expression_type(span)?, None, Reading::Objects)
    else {
        return None;
    };
    let mut classes = classes.into_iter();
    let class = classes.next()?;
    return classes.next().is_none().then_some(class);
}

#[derive(Default)]
pub(super) struct Bindings {
    keys: BTreeMap<String, String>,
    accessors: Vec<(String, Name)>,
}

impl Bindings {
    pub(super) fn add(&mut self, found: Found, artifacts: &AnalysisArtifacts) {
        let returned = |span: Span| -> Option<String> {
            let closure = range(span);
            if let Some(hint) = found.hinted.get(&closure) {
                return Some(hint.clone());
            }
            let mut classes = found
                .returns
                .get(&closure)?
                .iter()
                .map(|value| return single_class(artifacts, value));
            let first = classes.next()??;
            return classes
                .all(|class| return class.as_ref() == Some(&first))
                .then_some(first);
        };
        for (name, binding) in &found.bindings {
            let class = match binding {
                Binding::Class(class) => Some(class.clone()),
                Binding::Deferred(Bound::Closure(span)) => returned(*span),
                Binding::Deferred(Bound::Expression(span)) => single_class(artifacts, span),
            };
            if let Some(class) = class {
                self.keys.entry(name.lookup()).or_insert(class);
            }
        }
        self.accessors.extend(found.accessors);
    }

    pub(super) fn facades(&self, metadata: &CodebaseMetadata) -> BTreeMap<Word, Word> {
        let mut facades = BTreeMap::new();
        for (facade, name) in &self.accessors {
            if !metadata.is_instance_of(facade.as_bytes(), FACADE) {
                continue;
            }
            let target = match name {
                Name::Class(class) => self.keys.get(&name.lookup()).unwrap_or(class),
                Name::Key(_) => match self.keys.get(&name.lookup()) {
                    Some(class) => class,
                    None => continue,
                },
            };
            let (Some(facade), Some(target)) = (
                metadata.class_likes.get(&key(facade.as_bytes())),
                metadata.class_likes.get(&key(target.as_bytes())),
            ) else {
                continue;
            };
            facades.insert(facade.original_name, target.original_name);
        }
        return facades;
    }
}
