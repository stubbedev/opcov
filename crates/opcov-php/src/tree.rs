use std::collections::{BTreeMap, BTreeSet};

use mago_allocator::LocalArena;
use mago_names::resolver::NameResolver;
use mago_syntax::parser::parse_file;
use mago_syntax::walker::Walker;
use serde::{Deserialize, Serialize};

use crate::consumers::Untyped;
use crate::index::{Index, IndexStats};
use crate::members::{Collector, Lines, Member, Members, innermost_callable};
use crate::target::{Request, Target};

const VIA: &str = " via ";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Changed {
    Whole,
    Parts {
        ranges: Vec<(u32, u32)>,
        deletions: Vec<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub changed: Changed,
}

impl FileChange {
    fn touches(&self, member: &Member) -> bool {
        return match &self.changed {
            Changed::Whole => true,
            Changed::Parts { ranges, deletions } => {
                ranges
                    .iter()
                    .any(|(first, last)| return *first <= member.last && member.first <= *last)
                    || deletions
                        .iter()
                        .any(|before| return member.first < *before && *before <= member.last)
            }
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consumer {
    pub depth: u32,
    pub path: String,
    pub first: u32,
    pub last: u32,
    pub member: Option<String>,
    pub uses: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    pub changed: Vec<Member>,
    pub consumers: Vec<Consumer>,
    pub analysed: usize,
    pub untyped: Vec<Untyped>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub changes: Vec<FileChange>,
    pub request: Request,
    pub depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    #[serde(flatten)]
    pub tree: Tree,
    pub index: IndexStats,
}

impl Index {
    #[must_use]
    pub fn answer(&self, query: &Query) -> Answer {
        let changed = self.changed_members(&query.changes);
        return Answer {
            tree: self.consumers(
                changed,
                &query.request.targets,
                &query.request.within,
                query.depth,
            ),
            index: self.stats(),
        };
    }

    fn members_of(&self, path: &str) -> Vec<Member> {
        let Some(file) = self
            .positions
            .get(path)
            .and_then(|position| return self.sources.get(*position))
            .map(|source| return &source.file)
        else {
            return Vec::new();
        };
        let arena = LocalArena::new();
        let program = parse_file(&arena, file);
        let names = NameResolver::new(&arena).resolve(program);
        let lines = Lines::of(file.contents.as_ref());
        let mut members = Members::default();
        Collector {
            names: &names,
            lines: &lines,
            path,
        }
        .walk_program(program, &mut members);
        return members.found;
    }

    pub(crate) fn changed_members(&self, changes: &[FileChange]) -> Vec<Member> {
        let mut touched: Vec<Member> = changes
            .iter()
            .flat_map(|change| {
                return self
                    .members_of(&change.path)
                    .into_iter()
                    .filter(move |member| return change.touches(member));
            })
            .collect();
        touched.sort();
        touched.dedup();
        return touched;
    }

    pub(crate) fn consumers(
        &self,
        changed: Vec<Member>,
        extra: &[Target],
        within: &[String],
        depth: u32,
    ) -> Tree {
        let mut known: BTreeSet<String> = changed
            .iter()
            .map(|member| return member.target.to_string())
            .chain(extra.iter().map(ToString::to_string))
            .collect();
        let mut frontier: Vec<Target> = changed
            .iter()
            .map(|member| return member.target.clone())
            .chain(extra.iter().cloned())
            .collect();
        let mut members: BTreeMap<String, Vec<Member>> = BTreeMap::new();
        let mut consumers: BTreeMap<(String, String), Consumer> = BTreeMap::new();
        let mut untyped: BTreeSet<Untyped> = BTreeSet::new();
        for level in 1..=depth {
            if frontier.is_empty() {
                break;
            }
            let search = self.uses(&Request {
                targets: std::mem::take(&mut frontier),
                within: within.to_vec(),
            });
            untyped.extend(search.untyped);
            for found in search.uses {
                let in_file = members
                    .entry(found.path.clone())
                    .or_insert_with(|| return self.members_of(&found.path));
                let via = found
                    .target
                    .split_once(VIA)
                    .map(|(_, class)| return class.to_owned());
                let owner = innermost_callable(in_file, found.line)
                    .cloned()
                    .map(|mut member| {
                        if let Some(class) = &via
                            && member.target.class.is_some()
                        {
                            member.target.class = Some(class.clone());
                        }
                        return member;
                    });
                let key = owner.as_ref().map_or_else(
                    || return format!("{}:{}", found.path, found.line),
                    |member| return member.target.to_string(),
                );
                if owner.as_ref().is_some_and(|member| {
                    return known.contains(&member.target.to_string())
                        && !consumers.contains_key(&(found.path.clone(), key.clone()));
                }) {
                    continue;
                }
                let consumer = consumers
                    .entry((found.path.clone(), key))
                    .or_insert_with(|| {
                        return Consumer {
                            depth: level,
                            path: found.path.clone(),
                            first: owner
                                .as_ref()
                                .map_or(found.line, |member| return member.first),
                            last: owner
                                .as_ref()
                                .map_or(found.line, |member| return member.last),
                            member: owner
                                .as_ref()
                                .map(|member| return member.target.to_string()),
                            uses: BTreeSet::new(),
                        };
                    });
                consumer.uses.insert(found.target.clone());
                if let Some(member) = owner.filter(|member| return member.is_callable())
                    && known.insert(member.target.to_string())
                {
                    frontier.push(member.target);
                }
            }
        }
        let mut consumers: Vec<Consumer> = consumers.into_values().collect();
        consumers.sort_by(|left, right| {
            return (left.depth, &left.path, left.first).cmp(&(
                right.depth,
                &right.path,
                right.first,
            ));
        });
        return Tree {
            changed,
            consumers,
            analysed: self.analysed(),
            untyped: untyped.into_iter().collect(),
        };
    }
}
