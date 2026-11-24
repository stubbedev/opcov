use opcov_core::kind::{EdgeKind, LoopKind};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use super::position::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BranchKind {
    If,
    ElseIf,
    Ternary,
    ShortTernary,
    Coalesce,
    CoalesceAssign,
    NullsafeProperty,
    NullsafeMethodCall,
    And,
    Or,
    LogicalAnd,
    LogicalOr,
    Isset,
}

impl BranchKind {
    const fn name(self) -> &'static str {
        return match self {
            Self::If => "If",
            Self::ElseIf => "ElseIf",
            Self::Ternary => "Ternary",
            Self::ShortTernary => "ShortTernary",
            Self::Coalesce => "Coalesce",
            Self::CoalesceAssign => "CoalesceAssign",
            Self::NullsafeProperty => "NullsafeProperty",
            Self::NullsafeMethodCall => "NullsafeMethodCall",
            Self::And => "And",
            Self::Or => "Or",
            Self::LogicalAnd => "LogicalAnd",
            Self::LogicalOr => "LogicalOr",
            Self::Isset => "Isset",
        };
    }

    const fn outcomes(self) -> [EdgeKind; 2] {
        return match self {
            Self::If | Self::ElseIf | Self::Ternary => [EdgeKind::IfTrue, EdgeKind::IfFalse],
            Self::ShortTernary => [EdgeKind::TernaryValue, EdgeKind::TernaryFallback],
            Self::Coalesce | Self::CoalesceAssign => {
                [EdgeKind::CoalesceValue, EdgeKind::CoalesceFallback]
            }
            Self::NullsafeProperty | Self::NullsafeMethodCall => {
                [EdgeKind::NullsafeValue, EdgeKind::NullsafeNull]
            }
            Self::And | Self::Or | Self::LogicalAnd | Self::LogicalOr | Self::Isset => {
                [EdgeKind::Evaluate, EdgeKind::ShortCircuit]
            }
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConstructKind {
    Branch(BranchKind),
    Loop(LoopKind),
    Match,
    Switch,
    Catch,
    Finally,
}

impl ConstructKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        return match self {
            Self::Branch(branch) => branch.name(),
            Self::Loop(LoopKind::Foreach) => "Foreach",
            Self::Loop(LoopKind::For) => "For",
            Self::Loop(LoopKind::While) => "While",
            Self::Loop(LoopKind::DoWhile) => "DoWhile",
            Self::Match => "Match",
            Self::Switch => "Switch",
            Self::Catch => "Catch",
            Self::Finally => "Finally",
        };
    }
}

impl Serialize for ConstructKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.serialize_str(self.name());
    }
}

const fn loop_outcomes(kind: LoopKind) -> &'static [EdgeKind] {
    return match kind {
        LoopKind::Foreach => &[
            EdgeKind::LoopEnter,
            EdgeKind::LoopSkip,
            EdgeKind::LoopBack,
            EdgeKind::LoopExit,
        ],
        LoopKind::For | LoopKind::While | LoopKind::DoWhile => {
            &[EdgeKind::LoopBack, EdgeKind::LoopExit]
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum BodyPos {
    Clause(usize),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SwitchSubject {
    True,
    False,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Clause<S> {
    pub(super) span: S,
    pub(super) is_default: bool,
    pub(super) body: BodyPos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Shape<S> {
    Branch(BranchKind),
    Loop(LoopKind),
    Match {
        arms: Vec<S>,
        default: Option<usize>,
    },
    Switch {
        subject: SwitchSubject,
        clauses: Vec<Clause<S>>,
    },
    Catch {
        types: Vec<S>,
    },
    Finally {
        clause: S,
    },
}

impl<S> Shape<S> {
    pub(super) const fn kind(&self) -> ConstructKind {
        return match self {
            Self::Branch(branch) => ConstructKind::Branch(*branch),
            Self::Loop(kind) => ConstructKind::Loop(*kind),
            Self::Match { .. } => ConstructKind::Match,
            Self::Switch { .. } => ConstructKind::Switch,
            Self::Catch { .. } => ConstructKind::Catch,
            Self::Finally { .. } => ConstructKind::Finally,
        };
    }

    pub(super) fn try_map<T, E>(self, mut f: impl FnMut(S) -> Result<T, E>) -> Result<Shape<T>, E> {
        return Ok(match self {
            Self::Branch(branch) => Shape::Branch(branch),
            Self::Loop(kind) => Shape::Loop(kind),
            Self::Match { arms, default } => Shape::Match {
                arms: arms.into_iter().map(f).collect::<Result<_, _>>()?,
                default,
            },
            Self::Switch { subject, clauses } => Shape::Switch {
                subject,
                clauses: clauses
                    .into_iter()
                    .map(|clause| {
                        return Ok(Clause {
                            span: f(clause.span)?,
                            is_default: clause.is_default,
                            body: clause.body,
                        });
                    })
                    .collect::<Result<_, _>>()?,
            },
            Self::Catch { types } => Shape::Catch {
                types: types.into_iter().map(f).collect::<Result<_, _>>()?,
            },
            Self::Finally { clause } => Shape::Finally { clause: f(clause)? },
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ConstructOutcome {
    pub kind: EdgeKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Location {
    pub construct: ConstructKind,
    pub span: Span,
    pub outcome_span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Detail {
    Whole,
    Case(usize),
    CatchType(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unmatched {
    NotFromSource,
    NoConstruct,
    Ambiguous { sources: usize, candidates: usize },
    ClaimedFromSeveralLines,
    KindMismatch { construct: ConstructKind },
    ArmOutOfRange { arm: u32, arms: usize },
}

impl std::fmt::Display for Unmatched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return match self {
            Self::NotFromSource => {
                f.write_str("the outcome is engine-internal and has no source construct")
            }
            Self::NoConstruct => {
                f.write_str("no source construct of this kind can produce an opcode on this line")
            }
            Self::Ambiguous {
                sources,
                candidates,
            } => write!(
                f,
                "{sources} opcode(s) of this kind on the line but {candidates} candidate construct(s)"
            ),
            Self::ClaimedFromSeveralLines => {
                f.write_str("the candidate construct is also claimed by an opcode on another line")
            }
            Self::KindMismatch { construct } => {
                write!(
                    f,
                    "a {} construct cannot produce this outcome",
                    construct.name()
                )
            }
            Self::ArmOutOfRange { arm, arms } => {
                write!(
                    f,
                    "arm {arm} does not exist: the construct has {arms} arm(s)"
                )
            }
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Construct {
    span: Span,
    shape: Shape<Span>,
}

fn clauses_span(clauses: &[Clause<Span>], body: BodyPos) -> Option<Span> {
    let mut sharing = clauses.iter().filter(|clause| return clause.body == body);
    let first = sharing.next()?;
    let last = sharing.next_back().unwrap_or(first);
    return Some(Span::join(first.span, last.span));
}

fn distinct_bodies(bodies: impl Iterator<Item = BodyPos>) -> Vec<BodyPos> {
    let mut sorted: Vec<BodyPos> = bodies.collect();
    sorted.sort_unstable();
    sorted.dedup();
    return sorted;
}

fn case_bodies(clauses: &[Clause<Span>]) -> Vec<BodyPos> {
    return distinct_bodies(
        clauses
            .iter()
            .filter(|clause| return !clause.is_default)
            .map(|clause| return clause.body),
    );
}

fn default_target(clauses: &[Clause<Span>]) -> BodyPos {
    return clauses
        .iter()
        .find(|clause| return clause.is_default)
        .map_or(BodyPos::End, |clause| return clause.body);
}

fn rank<T: Copy>(items: &[T], index: u32) -> Result<T, Unmatched> {
    return usize::try_from(index)
        .ok()
        .and_then(|position| return items.get(position))
        .copied()
        .ok_or(Unmatched::ArmOutOfRange {
            arm: index,
            arms: items.len(),
        });
}

impl Construct {
    pub(super) const fn new(span: Span, shape: Shape<Span>) -> Self {
        return Self { span, shape };
    }

    #[must_use]
    pub const fn kind(&self) -> ConstructKind {
        return self.shape.kind();
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        return self.span;
    }

    const fn plain(&self, outcome: EdgeKind) -> ConstructOutcome {
        return ConstructOutcome {
            kind: outcome,
            span: self.span,
        };
    }

    fn body_span(&self, clauses: &[Clause<Span>], body: BodyPos) -> Span {
        return clauses_span(clauses, body).unwrap_or(self.span);
    }

    fn default_span(&self, clauses: &[Clause<Span>]) -> Span {
        return clauses
            .iter()
            .find(|clause| return clause.is_default)
            .map_or(self.span, |clause| return clause.span);
    }

    #[must_use]
    pub fn outcomes(&self) -> Vec<ConstructOutcome> {
        return match &self.shape {
            Shape::Branch(branch) => branch
                .outcomes()
                .map(|kind| return self.plain(kind))
                .to_vec(),
            Shape::Loop(kind) => loop_outcomes(*kind)
                .iter()
                .map(|&outcome| return self.plain(outcome))
                .collect(),
            Shape::Match { arms, default } => (0_u32..)
                .zip(arms)
                .map(|(arm, &span)| {
                    return ConstructOutcome {
                        kind: EdgeKind::MatchArm(arm),
                        span,
                    };
                })
                .chain(
                    default
                        .is_none()
                        .then(|| return self.plain(EdgeKind::MatchNoArm)),
                )
                .collect(),
            Shape::Switch { clauses, .. } => (0_u32..)
                .zip(case_bodies(clauses))
                .map(|(case, body)| {
                    return ConstructOutcome {
                        kind: EdgeKind::SwitchCase(case),
                        span: self.body_span(clauses, body),
                    };
                })
                .chain([ConstructOutcome {
                    kind: EdgeKind::SwitchDefault,
                    span: self.default_span(clauses),
                }])
                .collect(),
            Shape::Catch { types } => types
                .iter()
                .map(|&span| {
                    return ConstructOutcome {
                        kind: EdgeKind::Catch,
                        span,
                    };
                })
                .collect(),
            Shape::Finally { clause } => vec![
                ConstructOutcome {
                    kind: EdgeKind::Finally,
                    span: *clause,
                },
                ConstructOutcome {
                    kind: EdgeKind::FinallyException,
                    span: *clause,
                },
            ],
        };
    }

    const fn locate(&self, outcome_span: Span) -> Location {
        return Location {
            construct: self.kind(),
            span: self.span,
            outcome_span,
        };
    }

    fn resolve_switch(
        &self,
        subject: SwitchSubject,
        clauses: &[Clause<Span>],
        detail: Detail,
        kind: EdgeKind,
    ) -> Result<Location, Unmatched> {
        let mismatch = Unmatched::KindMismatch {
            construct: self.kind(),
        };
        return match (kind, detail) {
            (EdgeKind::SwitchCase(case), Detail::Whole) => {
                let body = rank(&case_bodies(clauses), case)?;
                Ok(self.locate(self.body_span(clauses, body)))
            }
            (EdgeKind::SwitchDefault, Detail::Whole) => Ok(self.locate(self.default_span(clauses))),
            (EdgeKind::MatchArm(arm), Detail::Whole) if subject == SwitchSubject::True => {
                let bodies = distinct_bodies(
                    clauses
                        .iter()
                        .filter(|clause| return !clause.is_default)
                        .map(|clause| return clause.body)
                        .chain([default_target(clauses)]),
                );
                let body = rank(&bodies, arm)?;
                Ok(self.locate(self.body_span(clauses, body)))
            }
            (EdgeKind::IfTrue | EdgeKind::IfFalse, Detail::Case(index))
                if subject == SwitchSubject::False =>
            {
                clauses
                    .get(index)
                    .map(|clause| return self.locate(clause.span))
                    .ok_or(mismatch)
            }
            _ => Err(mismatch),
        };
    }

    pub(super) fn resolve(&self, detail: Detail, kind: EdgeKind) -> Result<Location, Unmatched> {
        let mismatch = Unmatched::KindMismatch {
            construct: self.kind(),
        };
        return match (&self.shape, detail, kind) {
            (Shape::Branch(branch), Detail::Whole, _) if branch.outcomes().contains(&kind) => {
                Ok(self.locate(self.span))
            }
            (Shape::Loop(loop_kind), Detail::Whole, _)
                if loop_outcomes(*loop_kind).contains(&kind) =>
            {
                Ok(self.locate(self.span))
            }
            (Shape::Match { arms, .. }, Detail::Whole, EdgeKind::MatchArm(arm)) => {
                rank(arms, arm).map(|span| return self.locate(span))
            }
            (Shape::Match { default: None, .. }, Detail::Whole, EdgeKind::MatchNoArm) => {
                Ok(self.locate(self.span))
            }
            (Shape::Switch { subject, clauses }, _, _) => {
                self.resolve_switch(*subject, clauses, detail, kind)
            }
            (Shape::Catch { types }, Detail::CatchType(index), EdgeKind::Catch) => types
                .get(index)
                .map(|&span| return self.locate(span))
                .ok_or(mismatch),
            (
                Shape::Finally { clause },
                Detail::Whole,
                EdgeKind::Finally | EdgeKind::FinallyException,
            ) => Ok(self.locate(*clause)),
            _ => Err(mismatch),
        };
    }
}

impl Serialize for Construct {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Construct", 3)?;
        state.serialize_field("kind", &self.kind())?;
        state.serialize_field("span", &self.span)?;
        state.serialize_field("outcomes", &self.outcomes())?;
        return state.end();
    }
}
