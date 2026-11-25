use std::collections::BTreeSet;

use aho_corasick::{AhoCorasick, MatchKind};
use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_database::file::File;
use mago_names::ResolvedNames;
use mago_syntax::cst::Program;
use mago_syntax::walker::Walker;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

use crate::analysis::{Context, with_analysis};
use crate::facts::{FileFacts, Resolved, Resolver};
use crate::hierarchy::Hierarchy;
use crate::index::{Index, Source};
use crate::matching::Matcher;
use crate::members::Lines;
use crate::sites::{Collector, Located, Sites};
use crate::target::Request;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Use {
    pub path: String,
    pub line: u32,
    pub target: String,
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

struct Mentions(Option<AhoCorasick>);

impl Mentions {
    fn new(needles: &BTreeSet<String>) -> Result<Self, aho_corasick::BuildError> {
        if needles.is_empty() {
            return Ok(Self(None));
        }
        return AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .match_kind(MatchKind::Standard)
            .build(needles)
            .map(|automaton| return Self(Some(automaton)));
    }

    fn in_source(&self, source: &[u8]) -> bool {
        let Some(automaton) = &self.0 else {
            return false;
        };
        return automaton.find_overlapping_iter(source).any(|found| {
            let before = found
                .start()
                .checked_sub(1)
                .and_then(|position| return source.get(position));
            let after = source.get(found.end());
            return !before.is_some_and(|byte| return is_name_byte(*byte))
                && !after.is_some_and(|byte| return is_name_byte(*byte));
        });
    }
}

impl Index {
    fn copies<'s>(&'s self, located: &'s Located) -> Vec<(Option<&'s str>, Option<&'s str>)> {
        let class = located.class.as_deref();
        let Some(used) = class.filter(|_| return located.in_trait) else {
            return vec![(class, None)];
        };
        let users = self
            .trait_users
            .get(&used.to_ascii_lowercase())
            .map_or(&[][..], Vec::as_slice);
        if users.is_empty() {
            return vec![(Some(used), None)];
        }
        return users
            .iter()
            .map(|user| return (Some(user.as_str()), Some(user.as_str())))
            .collect();
    }

    fn resolve_sites(
        &self,
        source: &[u8],
        program: &Program<'_>,
        names: &ResolvedNames<'_>,
        artifacts: Option<&AnalysisArtifacts>,
    ) -> Vec<Resolved> {
        let hierarchy = Hierarchy(&self.metadata);
        let mut sites = Sites::default();
        Collector {
            names,
            codebase: &self.metadata,
        }
        .walk_program(program, &mut sites);
        let lines = Lines::of(source);
        let mut resolved = Vec::new();
        for located in &sites.found {
            let line = lines.line(located.offset);
            for (this, using) in self.copies(located) {
                let context = Resolver {
                    hierarchy,
                    artifacts,
                    this,
                };
                if let Some(reference) = context.resolve(&located.site) {
                    resolved.push(Resolved {
                        line,
                        using: using.map(str::to_owned),
                        reference,
                    });
                }
            }
        }
        return resolved;
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
                Ok(artifacts) => FileFacts::new(
                    self.resolve_sites(source, program, names, Some(artifacts)),
                    None,
                ),
                Err(error) => FileFacts::new(
                    self.resolve_sites(source, program, names, None),
                    Some(error.to_string()),
                ),
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

    #[must_use]
    pub fn uses(&self, request: &Request) -> Search {
        let matcher = &Matcher::new(&self.metadata, &self.forwards, &self.hooks, request);
        let needles = matcher.needles();
        let mentions = Mentions::new(&needles);
        let analysed: Vec<(&Source, &FileFacts)> = self
            .sources
            .par_iter()
            .filter(|source| {
                if !request.searches(&source.path) {
                    return false;
                }
                return match (source.facts.get(), &mentions) {
                    (Some(facts), _) => facts.mentions(&needles),
                    (None, Ok(mentions)) => mentions.in_source(source.file.contents.as_ref()),
                    (None, Err(_)) => true,
                };
            })
            .map(|source| return (source, self.facts_of(source)))
            .collect();
        let mut found: Vec<Use> = analysed
            .par_iter()
            .flat_map_iter(|(source, facts)| {
                return facts.named(&needles).flat_map(move |entry| {
                    return matcher
                        .matches(&entry.reference)
                        .into_iter()
                        .map(move |target| {
                            return Use {
                                target: matcher.attributed(target, entry.using.as_deref()),
                                path: source.path.clone(),
                                line: entry.line,
                            };
                        });
                });
            })
            .collect();
        found.sort();
        found.dedup();
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
