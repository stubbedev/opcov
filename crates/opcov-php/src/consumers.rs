use aho_corasick::AhoCorasick;
use mago_database::file::File;
use mago_syntax::walker::Walker;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

use crate::analysis::{Context, with_analysis};
use crate::index::Index;
use crate::matching::Matcher;
use crate::sites::{Collector, Located, Sites};
use crate::target::Request;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Use {
    pub path: String,
    pub line: u32,
    pub target: String,
}

fn line_of(source: &[u8], offset: u32) -> u32 {
    let end = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    let newlines = source.get(..end).map_or(0, |before| {
        return before.iter().filter(|byte| return **byte == b'\n').count();
    });
    return u32::try_from(newlines)
        .unwrap_or(u32::MAX)
        .saturating_add(1);
}

fn copies<'m>(
    matcher: &'m Matcher<'_>,
    located: &'m Located,
) -> Vec<(Option<&'m str>, Option<&'m str>)> {
    let class = located.class.as_deref();
    let Some(used) = class.filter(|_| return located.in_trait) else {
        return vec![(class, None)];
    };
    let users = matcher.trait_users(used);
    if users.is_empty() {
        return vec![(Some(used), None)];
    }
    return users
        .iter()
        .map(|user| return (Some(user.as_str()), Some(user.as_str())))
        .collect();
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Search {
    pub uses: Vec<Use>,
    pub analysed: usize,
}

impl Index {
    #[must_use]
    pub fn uses(&self, request: &Request) -> Search {
        let matcher = Matcher::new(&self.metadata, &self.forwards, request);
        let mentions = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(matcher.needles())
            .ok();
        let context = Context {
            metadata: &self.metadata,
            registry: &self.registry,
            settings: &self.settings,
        };
        let candidates: Vec<&File> = self
            .sources
            .par_iter()
            .filter(|file| {
                return request.searches(&String::from_utf8_lossy(file.name.as_ref()))
                    && mentions
                        .as_ref()
                        .is_none_or(|mentions| return mentions.is_match(file.contents.as_ref()));
            })
            .collect();
        let mut found: Vec<Use> = candidates
            .par_iter()
            .flat_map_iter(|file| {
                let path = String::from_utf8_lossy(file.name.as_ref()).into_owned();
                let source = file.contents.as_ref();
                let uses = with_analysis(&context, file, |program, names, artifacts| {
                    let mut sites = Sites::default();
                    Collector {
                        names,
                        codebase: &self.metadata,
                    }
                    .walk_program(program, &mut sites);
                    let mut uses = Vec::new();
                    for located in &sites.found {
                        let line = line_of(source, located.offset);
                        for (this, using) in copies(&matcher, located) {
                            for target in matcher.evaluate(&located.site, artifacts, this) {
                                uses.push(Use {
                                    target: matcher.attributed(target, using),
                                    path: path.clone(),
                                    line,
                                });
                            }
                        }
                    }
                    return uses;
                });
                return uses.unwrap_or_default();
            })
            .collect();
        found.sort();
        found.dedup();
        return Search {
            uses: found,
            analysed: candidates.len(),
        };
    }
}
