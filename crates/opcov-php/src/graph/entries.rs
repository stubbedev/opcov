use std::collections::BTreeMap;

use mago_allocator::LocalArena;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::visibility::Visibility;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::{Call, Expression, Statement};
use mago_syntax::parser::parse_file;
use serde::Serialize;

use crate::hierarchy::Hierarchy;
use crate::index::Index;
use crate::matching::Framework;
use crate::routes::{Route, Routes};
use crate::target::{Kind, Target};

const TEST_CASE: &str = "PHPUnit\\Framework\\TestCase";
const TEST_ATTRIBUTE: &str = "PHPUnit\\Framework\\Attributes\\Test";
const TEST_PREFIX: &str = "test";
const PEST_FUNCTIONS: &str = "vendor/pestphp/pest/src/Functions.php";
const PEST_TESTS: [&str; 3] = ["test", "it", "describe"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum EntryPoint {
    Route(Route),
    Command {
        name: String,
    },
    Job,
    Listener {
        events: Vec<String>,
    },
    ModelHook {
        model: String,
        instance: Vec<String>,
        query: Vec<String>,
    },
    PhpunitTest {
        classes: Vec<String>,
    },
    PestTest,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(super) struct BrokenRoute {
    pub(super) route: Route,
    pub(super) action: String,
}

#[derive(Debug, Default)]
pub(super) struct RouteTable {
    by_method: BTreeMap<(String, String), Vec<Route>>,
    pub(super) broken: Vec<BrokenRoute>,
}

impl RouteTable {
    pub(super) fn new(routes: &Routes, head: &Index) -> Self {
        let mut table = Self::default();
        for (route, action) in &routes.routes {
            let declaring = head
                .metadata
                .get_declaring_method_class(action.class.as_bytes(), action.method.as_bytes());
            let Some(declaring) = declaring else {
                let indexed = head
                    .metadata
                    .get_class_like(action.class.as_bytes())
                    .is_some();
                if !indexed && !action.in_sources {
                    continue;
                }
                table.broken.push(BrokenRoute {
                    route: route.clone(),
                    action: format!("{}@{}", action.class, action.method),
                });
                continue;
            };
            table
                .by_method
                .entry((
                    declaring.to_string().to_ascii_lowercase(),
                    action.method.to_ascii_lowercase(),
                ))
                .or_default()
                .push(route.clone());
        }
        table.broken.sort();
        return table;
    }

    fn routes(&self, class: &str, method: &str) -> &[Route] {
        return self
            .by_method
            .get(&(class.to_ascii_lowercase(), method.to_ascii_lowercase()))
            .map_or(&[], Vec::as_slice);
    }
}

fn original(codebase: &CodebaseMetadata, class: &[u8]) -> Option<String> {
    return codebase
        .get_class_like(class)
        .map(|metadata| return metadata.original_name.to_string());
}

fn test_classes(codebase: &CodebaseMetadata, class: &str, method: &str) -> Vec<String> {
    if !Hierarchy(codebase).is(class, TEST_CASE) {
        return Vec::new();
    }
    let Some(metadata) = codebase.get_method(class.as_bytes(), method.as_bytes()) else {
        return Vec::new();
    };
    let public = metadata
        .method_metadata
        .as_ref()
        .is_some_and(|method| return method.visibility == Visibility::Public);
    let marked = method.starts_with(TEST_PREFIX)
        || metadata.attributes.iter().any(|attribute| {
            return attribute
                .name
                .as_bytes()
                .eq_ignore_ascii_case(TEST_ATTRIBUTE.as_bytes());
        });
    if !public || !marked {
        return Vec::new();
    }
    let descendants = codebase
        .get_class_like(class.as_bytes())
        .and_then(|metadata| return codebase.all_class_like_descendants.get(&metadata.name))
        .into_iter()
        .flatten()
        .map(|descendant| return descendant.as_bytes().to_vec());
    let mut classes: Vec<String> = std::iter::once(class.as_bytes().to_vec())
        .chain(descendants)
        .filter(|candidate| {
            let concrete = codebase
                .get_class_like(candidate)
                .is_some_and(|metadata| return !metadata.flags.is_abstract());
            let runs_this = codebase
                .get_declaring_method_class(candidate, method.as_bytes())
                .is_some_and(|declaring| {
                    return declaring.as_bytes().eq_ignore_ascii_case(class.as_bytes());
                });
            return concrete && runs_this;
        })
        .filter_map(|candidate| return original(codebase, &candidate))
        .collect();
    classes.sort();
    classes.dedup();
    return classes;
}

const fn innermost_call<'a>(mut expression: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
    loop {
        match expression {
            Expression::Call(Call::Method(call)) => expression = call.object,
            Expression::Call(Call::NullSafeMethod(call)) => expression = call.object,
            Expression::Call(Call::Function(_)) => return Some(expression),
            _ => return None,
        }
    }
}

pub(super) fn is_pest_file(index: &Index, path: &str) -> bool {
    let declared_by_pest = |name: &str| {
        return index
            .metadata
            .get_function(name.as_bytes())
            .is_some_and(|function| {
                return function.span.file_id == FileId::new(PEST_FUNCTIONS.as_bytes());
            });
    };
    let Some(source) = index
        .positions
        .get(path)
        .and_then(|position| return index.sources.get(*position))
    else {
        return false;
    };
    let arena = LocalArena::new();
    let program = parse_file(&arena, &source.file);
    let names = NameResolver::new(&arena).resolve(program);
    return program.statements.iter().any(|statement| {
        let Statement::Expression(statement) = statement else {
            return false;
        };
        let Some(Expression::Call(Call::Function(call))) = innermost_call(statement.expression)
        else {
            return false;
        };
        let Expression::Identifier(identifier) = call.function else {
            return false;
        };
        let resolved = String::from_utf8_lossy(names.get(identifier)).into_owned();
        let local = String::from_utf8_lossy(identifier.value()).into_owned();
        return [resolved, local].iter().any(|name| {
            return PEST_TESTS
                .iter()
                .any(|test| return name.eq_ignore_ascii_case(test))
                && declared_by_pest(name);
        });
    });
}

pub(super) fn of_member(index: &Index, routes: &RouteTable, target: &Target) -> Vec<EntryPoint> {
    let (Kind::Method, Some(class)) = (target.kind, target.class.as_deref()) else {
        return Vec::new();
    };
    let method = target.member.as_str();
    let mut entries: Vec<EntryPoint> = routes
        .routes(class, method)
        .iter()
        .cloned()
        .map(EntryPoint::Route)
        .collect();
    let invocations = Framework::new(&index.metadata, &index.hooks).invocations(class, method);
    entries.extend(
        invocations
            .command
            .map(|name| return EntryPoint::Command { name }),
    );
    if invocations.queued {
        entries.push(EntryPoint::Job);
    }
    if !invocations.events.is_empty() {
        entries.push(EntryPoint::Listener {
            events: invocations.events,
        });
    }
    entries.extend(invocations.model_events.into_iter().map(|event| {
        return EntryPoint::ModelHook {
            model: event.model,
            instance: event.instance,
            query: event.query,
        };
    }));
    let classes = test_classes(&index.metadata, class, method);
    if !classes.is_empty() {
        entries.push(EntryPoint::PhpunitTest { classes });
    }
    entries.sort();
    entries.dedup();
    return entries;
}
