use std::collections::{BTreeMap, BTreeSet};

use mago_names::ResolvedNames;
use mago_span::{HasSpan, Span};
use mago_syntax::cst::{
    ArrowFunction, Closure, Function, Method, MethodBody, Program, PropertyHook, PropertyHookBody,
    Statement, TriviaKind,
};
use mago_syntax::walker::Walker;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    Everything,
    Declarations,
}

struct Bodies;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<Span>> for Bodies {
    fn walk_in_function(&self, function: &'ast Function<'arena>, bodies: &mut Vec<Span>) {
        bodies.push(function.body.span());
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, bodies: &mut Vec<Span>) {
        if let MethodBody::Concrete(block) = &method.body {
            bodies.push(block.span());
        }
    }

    fn walk_in_closure(&self, closure: &'ast Closure<'arena>, bodies: &mut Vec<Span>) {
        bodies.push(closure.body.span());
    }

    fn walk_in_arrow_function(&self, arrow: &'ast ArrowFunction<'arena>, bodies: &mut Vec<Span>) {
        bodies.push(arrow.expression.span());
    }

    fn walk_in_property_hook(&self, hook: &'ast PropertyHook<'arena>, bodies: &mut Vec<Span>) {
        if let PropertyHookBody::Concrete(body) = &hook.body {
            bodies.push(body.span());
        }
    }
}

fn bodies(program: &Program<'_>) -> Vec<(u32, u32)> {
    let mut spans = Vec::new();
    Bodies.walk_program(program, &mut spans);
    spans.sort_by_key(|span| return (span.start.offset, std::cmp::Reverse(span.end.offset)));
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start.offset < last.1 => last.1 = last.1.max(span.end.offset),
            _ => merged.push((span.start.offset, span.end.offset)),
        }
    }
    return merged;
}

fn in_body(bodies: &[(u32, u32)], offset: u32) -> bool {
    let after = bodies.partition_point(|(start, _)| return *start <= offset);
    return after
        .checked_sub(1)
        .and_then(|index| return bodies.get(index))
        .is_some_and(|(_, end)| return offset < *end);
}

fn text(bytes: &[u8]) -> String {
    return String::from_utf8_lossy(bytes).into_owned();
}

fn last_segment(name: &str) -> &str {
    return name.rsplit('\\').next().unwrap_or(name);
}

fn namespace(program: &Program<'_>) -> Option<String> {
    return program.statements.iter().find_map(|statement| {
        return match statement {
            Statement::Namespace(namespace) => namespace
                .name
                .as_ref()
                .map(|name| return text(name.value())),
            _ => None,
        };
    });
}

const fn is_name_byte(byte: u8) -> bool {
    return byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\\';
}

fn docblock_words(comment: &[u8]) -> impl Iterator<Item = &[u8]> {
    return comment
        .split(|byte| return !is_name_byte(*byte))
        .filter(|word| {
            return word
                .first()
                .is_some_and(|first| return first.is_ascii_uppercase() || *first == b'\\');
        });
}

pub(crate) fn referenced_classes(
    source: &[u8],
    program: &Program<'_>,
    names: &ResolvedNames<'_>,
    reach: Reach,
) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut aliases: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let bodies = match reach {
        Reach::Everything => Vec::new(),
        Reach::Declarations => bodies(program),
    };
    for (start, end, resolved, _) in names.iter() {
        let resolved = text(resolved);
        let written = source
            .get(
                usize::try_from(start).unwrap_or(usize::MAX)
                    ..usize::try_from(end).unwrap_or(usize::MAX),
            )
            .map_or_else(String::new, text);
        let alias = last_segment(written.trim_start_matches('\\')).to_ascii_lowercase();
        if !alias.is_empty() {
            aliases.entry(alias).or_default().insert(resolved.clone());
        }
        if !in_body(&bodies, start) {
            found.insert(resolved);
        }
    }
    let namespace = namespace(program);
    for trivia in program.trivia.iter().filter(|trivia| {
        return matches!(trivia.kind, TriviaKind::DocBlockComment)
            && !in_body(&bodies, trivia.span.start.offset);
    }) {
        for word in docblock_words(trivia.value) {
            let word = text(word);
            if let Some(absolute) = word.strip_prefix('\\') {
                found.insert(absolute.to_owned());
                continue;
            }
            let (head, rest) =
                word.split_once('\\')
                    .map_or((word.as_str(), None), |(head, rest)| {
                        return (head, Some(rest));
                    });
            if let Some(imported) = aliases.get(&head.to_ascii_lowercase()) {
                found.extend(imported.iter().map(|imported| {
                    return rest.map_or_else(
                        || return imported.clone(),
                        |rest| return format!("{imported}\\{rest}"),
                    );
                }));
                continue;
            }
            if let Some(namespace) = &namespace {
                found.insert(format!("{namespace}\\{word}"));
            }
            found.insert(word);
        }
    }
    return found;
}
