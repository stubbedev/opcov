use std::collections::{BTreeMap, BTreeSet};

use opcov_core::cfg::{Function, Line, LoopSite, LoopTest, OpIndex, Plan};
use opcov_core::kind::{EdgeKind, LoopKind};

use super::construct::{Construct, ConstructKind, Detail, Location, Unmatched};
use super::position::{LineRange, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum SiteClass {
    Condition,
    ShortCircuit,
    Coalesce,
    ShortTernary,
    Nullsafe,
    LoopEntry,
    LoopTest,
    Catch,
    Finally,
    FinallyEntry,
    Arms,
}

const fn class_of(kind: EdgeKind) -> Option<SiteClass> {
    return match kind {
        EdgeKind::IfTrue | EdgeKind::IfFalse => Some(SiteClass::Condition),
        EdgeKind::ShortCircuit | EdgeKind::Evaluate => Some(SiteClass::ShortCircuit),
        EdgeKind::CoalesceValue | EdgeKind::CoalesceFallback => Some(SiteClass::Coalesce),
        EdgeKind::TernaryValue | EdgeKind::TernaryFallback => Some(SiteClass::ShortTernary),
        EdgeKind::NullsafeNull | EdgeKind::NullsafeValue => Some(SiteClass::Nullsafe),
        EdgeKind::MatchArm(_)
        | EdgeKind::MatchNoArm
        | EdgeKind::SwitchCase(_)
        | EdgeKind::SwitchDefault => Some(SiteClass::Arms),
        EdgeKind::LoopEnter | EdgeKind::LoopSkip => Some(SiteClass::LoopEntry),
        EdgeKind::LoopBack | EdgeKind::LoopExit => Some(SiteClass::LoopTest),
        EdgeKind::Catch => Some(SiteClass::Catch),
        EdgeKind::Finally => Some(SiteClass::Finally),
        EdgeKind::FinallyException => Some(SiteClass::FinallyEntry),
        EdgeKind::Flow | EdgeKind::Engine | EdgeKind::Jump => None,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ConstructId<S> {
    pub(super) span: S,
    pub(super) kind: ConstructKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Site<S, R> {
    pub(super) class: SiteClass,
    pub(super) claim: Vec<R>,
    pub(super) construct: ConstructId<S>,
    pub(super) detail: Detail,
}

impl<S, R> Site<S, R> {
    pub(super) fn try_map<T, U, E>(
        self,
        mut span: impl FnMut(S) -> Result<T, E>,
        lines: impl FnMut(R) -> Result<U, E>,
    ) -> Result<Site<T, U>, E> {
        return Ok(Site {
            class: self.class,
            claim: self
                .claim
                .into_iter()
                .map(lines)
                .collect::<Result<_, _>>()?,
            construct: ConstructId {
                span: span(self.construct.span)?,
                kind: self.construct.kind,
            },
            detail: self.detail,
        });
    }
}

impl Site<Span, LineRange> {
    fn claims(&self, class: SiteClass, line: Line) -> bool {
        return self.class == class && self.claim.iter().any(|range| return range.contains(line));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutcomeRef {
    pub site: OpIndex,
    pub kind: EdgeKind,
    pub line: Line,
}

#[must_use]
pub fn outcome_refs(function: &Function) -> Vec<OutcomeRef> {
    return Plan::of(function)
        .outcomes()
        .iter()
        .map(|outcome| {
            return OutcomeRef {
                site: outcome.site,
                kind: outcome.kind,
                line: outcome.line,
            };
        })
        .collect();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchedOutcome {
    pub outcome: OutcomeRef,
    pub location: Result<Location, Unmatched>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matching {
    outcomes: Vec<MatchedOutcome>,
}

type Assignments = BTreeMap<(SiteClass, Line, OpIndex), Result<usize, Unmatched>>;

fn candidates(sites: &[Site<Span, LineRange>], class: SiteClass, line: Line) -> Vec<usize> {
    return sites
        .iter()
        .enumerate()
        .filter(|(_, site)| return site.claims(class, line))
        .map(|(index, _)| return index)
        .collect();
}

const fn shortfall(sources: usize, found: usize) -> Unmatched {
    if found == 0 {
        return Unmatched::NoConstruct;
    }
    return Unmatched::Ambiguous {
        sources,
        candidates: found,
    };
}

fn assign_ordinals(sites: &[Site<Span, LineRange>], outcomes: &[OutcomeRef]) -> Assignments {
    let mut groups: BTreeMap<(SiteClass, Line), BTreeSet<OpIndex>> = BTreeMap::new();
    for outcome in outcomes {
        if let Some(class) = class_of(outcome.kind).filter(|&class| return !claims_whole(class)) {
            groups
                .entry((class, outcome.line))
                .or_default()
                .insert(outcome.site);
        }
    }
    let mut assigned = Assignments::new();
    for ((class, line), sources) in groups {
        let found = candidates(sites, class, line);
        let results: Vec<Result<usize, Unmatched>> = if found.len() == sources.len() {
            found.into_iter().map(Ok).collect()
        } else {
            vec![Err(shortfall(sources.len(), found.len())); sources.len()]
        };
        for (source, result) in sources.into_iter().zip(results) {
            assigned.insert((class, line, source), result);
        }
    }
    let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
    for site in assigned.values().filter_map(|result| return result.ok()) {
        let count = uses.entry(site).or_default();
        *count = count.saturating_add(1);
    }
    for result in assigned.values_mut() {
        if result.is_ok_and(|site| return uses.get(&site).is_some_and(|&count| return count > 1)) {
            *result = Err(Unmatched::ClaimedFromSeveralLines);
        }
    }
    return assigned;
}

const fn claims_whole(class: SiteClass) -> bool {
    return matches!(class, SiteClass::Arms);
}

fn assign_whole(
    sites: &[Site<Span, LineRange>],
    class: SiteClass,
    outcome: &OutcomeRef,
) -> Result<usize, Unmatched> {
    return match candidates(sites, class, outcome.line).as_slice() {
        &[only] => Ok(only),
        found => Err(shortfall(1, found.len())),
    };
}

impl Matching {
    pub(super) fn new(
        constructs: &[Construct],
        sites: &[Site<Span, LineRange>],
        outcomes: &[OutcomeRef],
    ) -> Self {
        let assigned = assign_ordinals(sites, outcomes);
        let matched = outcomes
            .iter()
            .map(|&outcome| {
                let site = match class_of(outcome.kind) {
                    None => Err(Unmatched::NotFromSource),
                    Some(class) if claims_whole(class) => assign_whole(sites, class, &outcome),
                    Some(class) => assigned
                        .get(&(class, outcome.line, outcome.site))
                        .copied()
                        .unwrap_or(Err(Unmatched::NoConstruct)),
                };
                let location = site.and_then(|index| {
                    let site = sites.get(index).ok_or(Unmatched::NoConstruct)?;
                    let wanted = (site.construct.span, site.construct.kind);
                    let position = constructs
                        .binary_search_by_key(&wanted, |construct| {
                            return (construct.span(), construct.kind());
                        })
                        .ok()
                        .ok_or(Unmatched::NoConstruct)?;
                    let construct = constructs.get(position).ok_or(Unmatched::NoConstruct)?;
                    return construct.resolve(site.detail, outcome.kind);
                });
                return MatchedOutcome { outcome, location };
            })
            .collect();
        return Self { outcomes: matched };
    }

    #[must_use]
    pub fn outcomes(&self) -> &[MatchedOutcome] {
        return &self.outcomes;
    }

    pub fn unmatched(&self) -> impl Iterator<Item = (&OutcomeRef, Unmatched)> {
        return self.outcomes.iter().filter_map(|matched| {
            return matched
                .location
                .err()
                .map(|reason| return (&matched.outcome, reason));
        });
    }

    #[must_use]
    pub fn location(&self, site: OpIndex, kind: EdgeKind) -> Option<Result<Location, Unmatched>> {
        return self
            .outcomes
            .iter()
            .find(|matched| return matched.outcome.site == site && matched.outcome.kind == kind)
            .map(|matched| return matched.location);
    }

    #[must_use]
    pub fn loop_kind(&self, plan: &Plan, planned: &LoopSite) -> Option<LoopKind> {
        let back = match planned.test {
            LoopTest::Foreach { back, .. }
            | LoopTest::PreTest { back, .. }
            | LoopTest::PostTest { back, .. } => back,
        };
        let site = plan.outcome(back)?.site;
        let location = self.location(site, EdgeKind::LoopBack)?.ok()?;
        return match location.construct {
            ConstructKind::Loop(kind) => Some(kind),
            _ => None,
        };
    }
}
