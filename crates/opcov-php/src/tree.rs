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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Hunks {
    ranges: Vec<(u32, u32)>,
    removed_before: Vec<u32>,
}

impl Hunks {
    pub(crate) const fn is_empty(&self) -> bool {
        return self.ranges.is_empty() && self.removed_before.is_empty();
    }
}

impl FileChange {
    pub(crate) fn within(&self, member: &Member) -> Hunks {
        return match &self.changed {
            Changed::Whole => Hunks {
                ranges: vec![(member.first, member.last)],
                removed_before: Vec::new(),
            },
            Changed::Parts { ranges, deletions } => Hunks {
                ranges: ranges
                    .iter()
                    .filter(|(first, last)| return *first <= member.last && member.first <= *last)
                    .map(|(first, last)| {
                        return ((*first).max(member.first), (*last).min(member.last));
                    })
                    .collect(),
                removed_before: deletions
                    .iter()
                    .copied()
                    .filter(|before| return member.first < *before && *before <= member.last)
                    .collect(),
            },
        };
    }

    pub(crate) fn outside(&self, callables: &[&Member], lines: u32) -> Hunks {
        let inside = |line: u32| return callables.iter().any(|member| return member.contains(line));
        let (ranges, deletions): (Vec<(u32, u32)>, &[u32]) = match &self.changed {
            Changed::Whole => (vec![(1, lines)], &[]),
            Changed::Parts { ranges, deletions } => (ranges.clone(), deletions),
        };
        let mut outside = Vec::new();
        for (first, last) in ranges {
            let mut start = None;
            for line in first..=last {
                match (inside(line), start) {
                    (false, None) => start = Some(line),
                    (true, Some(open)) => {
                        outside.push((open, line.saturating_sub(1)));
                        start = None;
                    }
                    (true, None) | (false, Some(_)) => {}
                }
            }
            if let Some(open) = start {
                outside.push((open, last));
            }
        }
        return Hunks {
            ranges: outside,
            removed_before: deletions
                .iter()
                .copied()
                .filter(|before| {
                    return !callables
                        .iter()
                        .any(|member| return member.first < *before && *before <= member.last);
                })
                .collect(),
        };
    }

    pub(crate) fn touches(&self, member: &Member) -> bool {
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

pub(crate) fn split_via(used: &str) -> (&str, Option<&str>) {
    return used
        .split_once(VIA)
        .map_or((used, None), |(target, class)| return (target, Some(class)));
}

pub(crate) fn attributed(member: &Member, via: Option<&str>) -> Member {
    let mut member = member.clone();
    if let Some(class) = via
        && member.target.class.is_some()
    {
        member.target.class = Some(class.to_owned());
    }
    return member;
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

    pub(crate) fn line_count(&self, path: &str) -> Option<u32> {
        let source = self
            .positions
            .get(path)
            .and_then(|position| return self.sources.get(*position))?;
        let contents: &[u8] = source.file.contents.as_ref();
        let newlines = contents
            .iter()
            .filter(|byte| return **byte == b'\n')
            .count();
        let unterminated = usize::from(!contents.is_empty() && !contents.ends_with(b"\n"));
        return u32::try_from(newlines.saturating_add(unterminated)).ok();
    }

    pub(crate) fn members_of(&self, path: &str) -> Vec<Member> {
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
                let via = split_via(&found.target).1;
                let owner = innermost_callable(in_file, found.line)
                    .map(|member| return attributed(member, via));
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
