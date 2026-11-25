use std::collections::{BTreeMap, BTreeSet};

use mago_names::ResolvedNames;
use mago_syntax::cst::{Program, Statement, TriviaKind};

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
) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
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
            aliases
                .entry(alias)
                .or_insert_with(|| return resolved.clone());
        }
        found.insert(resolved);
    }
    let namespace = namespace(program);
    for trivia in program
        .trivia
        .iter()
        .filter(|trivia| return matches!(trivia.kind, TriviaKind::DocBlockComment))
    {
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
                found.insert(rest.map_or_else(
                    || return imported.clone(),
                    |rest| return format!("{imported}\\{rest}"),
                ));
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
