use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, OnceLock};

use aho_corasick::{AhoCorasick, MatchKind};
use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_database::file::File;
use mago_names::ResolvedNames;
use mago_syntax::cst::Program;
use mago_syntax::walker::Walker;
use rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::analysis::{Context, with_analysis};
use crate::facts::{FileFacts, Reference, Resolved, Resolver, TraitCopy};
use crate::hierarchy::Hierarchy;
use crate::index::{Index, Source};
use crate::matching::Matcher;
use crate::members::Lines;
use crate::sites::{Collector, Sites};
use crate::target::{Request, Resolution};
use crate::types::USING_CLASS;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Use {
    pub path: String,
    pub line: u32,
    pub target: String,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Untyped {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Search {
    pub uses: Vec<Use>,
    pub analysed: usize,
    pub untyped: Vec<Untyped>,
}

const fn is_name_byte(byte: u8) -> bool {
    return byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80;
}

fn is_variable(preceding: &[u8]) -> bool {
    let Some(sigil) = preceding.strip_suffix(b"$") else {
        return false;
    };
    return !sigil.trim_ascii_end().ends_with(b"::");
}

pub(crate) struct Mentions(Option<AhoCorasick>);

impl Mentions {
    pub(crate) fn new(needles: &BTreeSet<String>) -> Result<Self, aho_corasick::BuildError> {
        if needles.is_empty() {
            return Ok(Self(None));
        }
        return AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .match_kind(MatchKind::Standard)
            .build(needles)
            .map(|automaton| return Self(Some(automaton)));
    }

    pub(crate) fn in_source(&self, source: &[u8]) -> bool {
        let Some(automaton) = &self.0 else {
            return false;
        };
        return automaton.find_overlapping_iter(source).any(|found| {
            let preceding = source.get(..found.start()).unwrap_or_default();
            let after = source.get(found.end());
            return !preceding
                .last()
                .is_some_and(|byte| return is_name_byte(*byte))
                && !after.is_some_and(|byte| return is_name_byte(*byte))
                && !is_variable(preceding);
        });
    }
}

impl Index {
    fn copies_for(&self, used: &str) -> Vec<TraitCopy> {
        let users = self
            .trait_users
            .get(&used.to_ascii_lowercase())
            .map_or(&[][..], Vec::as_slice);
        return users
            .iter()
            .map(|user| {
                let parent = self
                    .metadata
                    .get_class_like(user.as_bytes())
                    .and_then(|class| return class.direct_parent_class)
                    .and_then(|parent| return self.metadata.get_class_like(parent.as_bytes()))
                    .map(|parent| return parent.original_name.to_string());
                return TraitCopy {
                    user: user.clone(),
                    parent,
                };
            })
            .collect();
    }

    fn resolve_sites(
        &self,
        source: &[u8],
        program: &Program<'_>,
        names: &ResolvedNames<'_>,
        artifacts: Option<&AnalysisArtifacts>,
    ) -> (Vec<Resolved>, Vec<Vec<TraitCopy>>) {
        let hierarchy = Hierarchy(&self.metadata);
        let mut sites = Sites::default();
        Collector {
            names,
            codebase: &self.metadata,
        }
        .walk_program(program, &mut sites);
        let lines = Lines::of(source);
        let mut resolved = Vec::new();
        let mut copies: Vec<Vec<TraitCopy>> = Vec::new();
        let mut slots: BTreeMap<String, Option<usize>> = BTreeMap::new();
        for located in &sites.found {
            let line = lines.line(located.offset);
            let class = located.class.as_deref();
            let used = class.filter(|_| return located.in_trait);
            let slot = used.and_then(|used| {
                return *slots.entry(used.to_owned()).or_insert_with(|| {
                    let users = self.copies_for(used);
                    if users.is_empty() {
                        return None;
                    }
                    copies.push(users);
                    return Some(copies.len().saturating_sub(1));
                });
            });
            let this = if slot.is_some() {
                Some(USING_CLASS)
            } else {
                class
            };
            let context = Resolver {
                hierarchy,
                artifacts,
                this,
            };
            if let Some(reference) = context.resolve(&located.site) {
                resolved.push(Resolved {
                    line,
                    copies: slot,
                    reference,
                });
            }
        }
        return (resolved, copies);
    }

    fn analyse(&self, file: &File) -> FileFacts {
        let context = Context {
            metadata: &self.metadata,
            registry: &self.registry,
            settings: &self.settings,
        };
        let source = file.contents.as_ref();
        return with_analysis(&context, file, |program, names, analysed| {
            return match analysed {
                Ok(artifacts) => {
                    let (resolved, copies) =
                        self.resolve_sites(source, program, names, Some(artifacts));
                    FileFacts::new(resolved, copies, None)
                }
                Err(error) => {
                    let (resolved, copies) = self.resolve_sites(source, program, names, None);
                    FileFacts::new(resolved, copies, Some(error.to_string()))
                }
            };
        });
    }

    fn facts_of<'s>(&self, source: &'s Source) -> &'s FileFacts {
        return source
            .facts
            .get_or_init(|| return self.analyse(&source.file));
    }

    pub fn warm(&self) -> usize {
        self.sources.par_iter().for_each(|source| {
            self.facts_of(source);
        });
        return self.sources.len();
    }

    pub(crate) fn analysed(&self) -> usize {
        return self
            .sources
            .iter()
            .filter(|source| return source.facts.get().is_some())
            .count();
    }

    fn matcher<'s>(&'s self, request: &'s Request) -> Matcher<'s> {
        return Matcher::new(&self.metadata, &self.forwards, &self.hooks, request);
    }

    fn wanted<'s>(&'s self, request: &Request, needles: &BTreeSet<String>) -> Vec<&'s Source> {
        let mentions = Mentions::new(needles);
        return self
            .sources
            .par_iter()
            .filter(|source| {
                if !request.searches(&source.path) {
                    return false;
                }
                return match (source.facts.get(), &mentions) {
                    (Some(facts), _) => facts.mentions(needles),
                    (None, Ok(mentions)) => mentions.in_source(source.file.contents.as_ref()),
                    (None, Err(_)) => true,
                };
            })
            .collect();
    }

    pub(crate) fn analyse_wanted(indexes: &[&Self], request: &Request) {
        let mut claimed: HashSet<*const OnceLock<FileFacts>> = HashSet::new();
        let mut pending: Vec<(&Self, &Source)> = Vec::new();
        for index in indexes {
            let needles = index.matcher(request).needles();
            for source in index.wanted(request, &needles) {
                if source.facts.get().is_none() && claimed.insert(Arc::as_ptr(&source.facts)) {
                    pending.push((index, source));
                }
            }
        }
        pending.par_iter().for_each(|(index, source)| {
            index.facts_of(source);
        });
    }

    #[must_use]
    pub fn uses(&self, request: &Request) -> Search {
        let matcher = &self.matcher(request);
        let needles = matcher.needles();
        let analysed: Vec<(&Source, &FileFacts)> = self
            .wanted(request, &needles)
            .into_par_iter()
            .map(|source| return (source, self.facts_of(source)))
            .collect();
        let mut found: Vec<Use> = analysed
            .par_iter()
            .flat_map_iter(|(source, facts)| {
                return facts.named(&needles).flat_map(move |entry| {
                    let copies: Vec<(Option<&str>, Cow<'_, Reference>)> =
                        facts.copies_of(entry).map_or_else(
                            || return vec![(None, Cow::Borrowed(&entry.reference))],
                            |copies| {
                                return copies
                                    .iter()
                                    .map(|copy| {
                                        return (
                                            Some(copy.user.as_str()),
                                            Cow::Owned(
                                                entry
                                                    .reference
                                                    .for_copy(&copy.user, copy.parent.as_deref()),
                                            ),
                                        );
                                    })
                                    .collect();
                            },
                        );
                    return copies.into_iter().flat_map(move |(using, reference)| {
                        return matcher.matches(&reference).into_iter().map(move |found| {
                            return Use {
                                target: matcher.attributed(found.target, using),
                                path: source.path.clone(),
                                line: entry.line,
                                resolution: found.resolution,
                            };
                        });
                    });
                });
            })
            .collect();
        found.sort();
        found.dedup_by(|later, kept| {
            return (&later.path, later.line, &later.target)
                == (&kept.path, kept.line, &kept.target);
        });
        let mut untyped: Vec<Untyped> = self
            .untyped
            .iter()
            .cloned()
            .chain(analysed.iter().filter_map(|(source, facts)| {
                return facts.untyped.as_ref().map(|error| {
                    return Untyped {
                        path: source.path.clone(),
                        error: error.clone(),
                    };
                });
            }))
            .collect();
        untyped.sort();
        untyped.dedup();
        return Search {
            uses: found,
            analysed: analysed.len(),
            untyped,
        };
    }
}
