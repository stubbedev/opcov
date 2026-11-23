use std::collections::BTreeSet;

use super::model::{Block, BlockFlag, BlockId, Edge, Function, Line, LoopShape, OpIndex};
use super::opcode::Opcode;
use crate::kind::EdgeKind;
use crate::report::{LoopId, OutcomeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Branch {
    Target,
    Fallthrough,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Edge {
        from: BlockId,
        to: BlockId,
        branch: Branch,
    },
    FinallyEntry {
        block: BlockId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutcomeSite {
    pub id: OutcomeId,
    pub site: OpIndex,
    pub kind: EdgeKind,
    pub line: Line,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopTest {
    Foreach {
        skip: OutcomeId,
        back: OutcomeId,
        exit: OutcomeId,
    },
    PreTest {
        test: BlockId,
        back: OutcomeId,
        exit: OutcomeId,
        entries: Vec<BlockId>,
    },
    PostTest {
        test: BlockId,
        back: OutcomeId,
    },
}

impl LoopTest {
    #[must_use]
    pub const fn shape(&self) -> LoopShape {
        return match self {
            Self::Foreach { .. } => LoopShape::Foreach,
            Self::PreTest { .. } => LoopShape::PreTest,
            Self::PostTest { .. } => LoopShape::PostTest,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopSite {
    pub id: LoopId,
    pub header: BlockId,
    pub line: Line,
    pub test: LoopTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpSpan {
    pub start: OpIndex,
    pub end: OpIndex,
}

impl OpSpan {
    #[must_use]
    pub fn contains(self, op: OpIndex) -> bool {
        return self.start <= op && op < self.end;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FramelessCall {
    fallback: OpSpan,
    frameless: OpSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    outcomes: Vec<OutcomeSite>,
    twins: Vec<OutcomeSite>,
    fallbacks: Vec<OpSpan>,
    loops: Vec<LoopSite>,
}

const fn is_jump(opcode: Opcode) -> bool {
    return matches!(opcode, Opcode::Jmp);
}

const fn is_table(opcode: Opcode) -> bool {
    return matches!(
        opcode,
        Opcode::Match | Opcode::SwitchLong | Opcode::SwitchString
    );
}

fn next_block(from: BlockId) -> Option<BlockId> {
    return from.get().checked_add(1).map(BlockId::new);
}

fn branch(function: &Function, edge: &Edge, opcode: Opcode) -> Branch {
    if is_table(opcode) {
        return Branch::Table;
    }
    if is_jump(opcode) || Some(edge.to) != next_block(edge.from) {
        return Branch::Target;
    }
    if function.edges_between(edge.from, edge.to).len() < 2 {
        return Branch::Fallthrough;
    }
    let fallthrough_kind =
        super::classify::opcode_edge_kind(opcode, super::classify::Successor::Fallthrough);
    if fallthrough_kind == Some(edge.kind) {
        return Branch::Fallthrough;
    }
    return Branch::Target;
}

struct Incoming<'f>(Vec<&'f Edge>);

impl<'f> Incoming<'f> {
    fn of(function: &'f Function) -> Self {
        if function.loops().is_empty() {
            return Self(Vec::new());
        }
        let mut edges: Vec<&Edge> = function.edges().iter().collect();
        edges.sort_by_key(|edge| return (edge.to, edge.from, edge.kind));
        return Self(edges);
    }

    fn into(&self, to: BlockId) -> impl Iterator<Item = &'f Edge> {
        let first = self.0.partition_point(|edge| return edge.to < to);
        return self
            .0
            .get(first..)
            .unwrap_or_default()
            .iter()
            .copied()
            .take_while(move |edge| return edge.to == to);
    }
}

struct Draft {
    site: OpIndex,
    kind: EdgeKind,
    line: Line,
    origin: Origin,
}

impl Draft {
    const fn shape(&self) -> (EdgeKind, Option<Branch>) {
        return match self.origin {
            Origin::Edge { branch, .. } => (self.kind, Some(branch)),
            Origin::FinallyEntry { .. } => (self.kind, None),
        };
    }

    const fn site(&self, id: OutcomeId) -> OutcomeSite {
        return OutcomeSite {
            id,
            site: self.site,
            kind: self.kind,
            line: self.line,
            origin: self.origin,
        };
    }

    const fn order(&self) -> (OpIndex, BlockId, EdgeKind) {
        let to = match self.origin {
            Origin::Edge { to, .. } => to,
            Origin::FinallyEntry { block } => block,
        };
        return (self.site, to, self.kind);
    }
}

fn edge_draft(function: &Function, edge: &Edge) -> Option<Draft> {
    if !edge.kind.is_outcome() {
        return None;
    }
    let block = function.block(edge.from)?;
    let terminator = block.ops().last()?;
    return Some(Draft {
        site: block.op_end(),
        kind: edge.kind,
        line: terminator.line,
        origin: Origin::Edge {
            from: edge.from,
            to: edge.to,
            branch: branch(function, edge, terminator.opcode),
        },
    });
}

fn finally_draft(block: &Block) -> Option<Draft> {
    if !block.is_reachable() || !block.flags().contains(BlockFlag::Finally) {
        return None;
    }
    let first = block.ops().first()?;
    return Some(Draft {
        site: block.op_start(),
        kind: EdgeKind::FinallyException,
        line: first.line,
        origin: Origin::FinallyEntry { block: block.id() },
    });
}

fn edges_from(function: &Function, from: BlockId) -> &[Edge] {
    let edges = function.edges();
    let first = edges.partition_point(|edge| return edge.from < from);
    let rest = edges.get(first..).unwrap_or_default();
    return rest
        .get(..rest.partition_point(|edge| return edge.from == from))
        .unwrap_or_default();
}

fn frameless_call(function: &Function, jump: &Block) -> Option<FramelessCall> {
    let fallback = function.block(next_block(jump.id())?)?;
    let frameless = edges_from(function, jump.id())
        .iter()
        .find(|edge| return edge.to != fallback.id())
        .and_then(|edge| return function.block(edge.to))?;
    let end_jump = function.block(BlockId::new(frameless.id().get().checked_sub(1)?))?;
    if end_jump.id() < fallback.id() || end_jump.terminator() != Some(Opcode::Jmp) {
        return None;
    }
    let [after] = edges_from(function, end_jump.id()) else {
        return None;
    };
    let end = function.block(after.to)?.op_start();
    if end <= frameless.op_start() {
        return None;
    }
    return Some(FramelessCall {
        fallback: OpSpan {
            start: fallback.op_start(),
            end: frameless.op_start(),
        },
        frameless: OpSpan {
            start: frameless.op_start(),
            end,
        },
    });
}

fn frameless_calls(function: &Function) -> Vec<FramelessCall> {
    return function
        .blocks()
        .iter()
        .filter(|block| return block.terminator() == Some(Opcode::JmpFrameless))
        .filter_map(|block| return frameless_call(function, block))
        .collect();
}

fn within(drafts: &[Draft], span: OpSpan) -> core::ops::Range<usize> {
    let start = drafts.partition_point(|draft| return draft.site < span.start);
    let end = drafts.partition_point(|draft| return draft.site < span.end);
    return start..end.max(start);
}

fn twin_drafts(drafts: &[Draft], calls: &[FramelessCall]) -> Vec<Option<usize>> {
    let mut twin = vec![None; drafts.len()];
    for call in calls {
        let fallback = within(drafts, call.fallback);
        let frameless = within(drafts, call.frameless);
        let shapes = |range: core::ops::Range<usize>| {
            return drafts
                .get(range)
                .unwrap_or_default()
                .iter()
                .map(Draft::shape)
                .collect::<Vec<_>>();
        };
        if shapes(fallback.clone()) != shapes(frameless.clone()) {
            continue;
        }
        for (copy, original) in fallback.zip(frameless) {
            if let Some(slot) = twin.get_mut(copy) {
                *slot = Some(original);
            }
        }
    }
    return twin;
}

impl Plan {
    #[must_use]
    pub fn of(function: &Function) -> Self {
        let mut drafts: Vec<Draft> = function
            .edges()
            .iter()
            .filter_map(|edge| return edge_draft(function, edge))
            .chain(function.blocks().iter().filter_map(finally_draft))
            .collect();
        drafts.sort_by_key(Draft::order);
        let calls = frameless_calls(function);
        let twin = twin_drafts(&drafts, &calls);
        let mut ids: Vec<Option<OutcomeId>> = vec![None; drafts.len()];
        let mut outcomes = Vec::with_capacity(drafts.len());
        for ((draft, slot), copy) in drafts.iter().zip(&mut ids).zip(&twin) {
            if copy.is_some() {
                continue;
            }
            let Ok(number) = u32::try_from(outcomes.len()) else {
                break;
            };
            let id = OutcomeId::new(number);
            *slot = Some(id);
            outcomes.push(draft.site(id));
        }
        let twins = drafts
            .iter()
            .zip(&twin)
            .filter_map(|(draft, copy)| {
                let id = ids.get((*copy)?).copied().flatten()?;
                return Some(draft.site(id));
            })
            .collect();
        let mut plan = Self {
            outcomes,
            twins,
            fallbacks: calls.iter().map(|call| return call.fallback).collect(),
            loops: Vec::new(),
        };
        let incoming = Incoming::of(function);
        plan.loops = (0..)
            .zip(
                function
                    .loops()
                    .iter()
                    .filter_map(|cfg_loop| return plan.loop_site(function, &incoming, cfg_loop)),
            )
            .map(|(number, (header, line, test))| {
                return LoopSite {
                    id: LoopId::new(number),
                    header,
                    line,
                    test,
                };
            })
            .collect();
        return plan;
    }

    #[must_use]
    pub fn outcomes(&self) -> &[OutcomeSite] {
        return &self.outcomes;
    }

    #[must_use]
    pub fn twins(&self) -> &[OutcomeSite] {
        return &self.twins;
    }

    #[must_use]
    pub fn fallbacks(&self) -> &[OpSpan] {
        return &self.fallbacks;
    }

    #[must_use]
    pub fn loops(&self) -> &[LoopSite] {
        return &self.loops;
    }

    #[must_use]
    pub fn outcome(&self, id: OutcomeId) -> Option<&OutcomeSite> {
        return usize::try_from(id.get())
            .ok()
            .and_then(|index| return self.outcomes.get(index));
    }

    fn outcomes_from(&self, function: &Function, from: BlockId) -> &[OutcomeSite] {
        let Some(site) = function.block(from).map(Block::op_end) else {
            return &[];
        };
        let first = self
            .outcomes
            .partition_point(|outcome| return outcome.site < site);
        let rest = self.outcomes.get(first..).unwrap_or_default();
        let count = rest.partition_point(|outcome| return outcome.site == site);
        return rest.get(..count).unwrap_or_default();
    }

    fn edge_outcome(
        &self,
        function: &Function,
        from: BlockId,
        kind: EdgeKind,
        to: Option<BlockId>,
    ) -> Option<OutcomeId> {
        return self
            .outcomes_from(function, from)
            .iter()
            .find(|outcome| {
                return outcome.kind == kind
                    && matches!(
                        outcome.origin,
                        Origin::Edge { from: source, to: target, .. }
                            if source == from && to.is_none_or(|wanted| return wanted == target)
                    );
            })
            .map(|outcome| return outcome.id);
    }

    fn leaves(&self, function: &Function, from: BlockId, members: &BTreeSet<BlockId>) -> bool {
        return self.outcomes_from(function, from).iter().any(|outcome| {
            return outcome.kind == EdgeKind::LoopExit
                && matches!(
                    outcome.origin,
                    Origin::Edge { from: source, to, .. } if source == from && !members.contains(&to)
                );
        });
    }

    fn back_edge_test(&self, function: &Function, members: &BTreeSet<BlockId>) -> Option<BlockId> {
        let mut tests = members.iter().flat_map(|&member| {
            return self
                .outcomes_from(function, member)
                .iter()
                .filter_map(move |outcome| {
                    return match outcome.origin {
                        Origin::Edge { from, to, .. }
                            if outcome.kind == EdgeKind::LoopBack
                                && from == member
                                && members.contains(&to)
                                && self.leaves(function, from, members) =>
                        {
                            Some(from)
                        }
                        Origin::Edge { .. } | Origin::FinallyEntry { .. } => None,
                    };
                });
        });
        let test = tests.next()?;
        return tests.all(|other| return other == test).then_some(test);
    }

    fn loop_site(
        &self,
        function: &Function,
        incoming: &Incoming<'_>,
        cfg_loop: &super::model::Loop,
    ) -> Option<(BlockId, Line, LoopTest)> {
        let header = cfg_loop.header();
        let line = function.block(header)?.ops().first()?.line;
        let members: BTreeSet<BlockId> = cfg_loop.members().collect();
        let test = match cfg_loop.kind() {
            LoopShape::Foreach => {
                let reset = incoming
                    .into(header)
                    .find(|edge| return edge.kind == EdgeKind::LoopEnter)?
                    .from;
                LoopTest::Foreach {
                    skip: self.edge_outcome(function, reset, EdgeKind::LoopSkip, None)?,
                    back: self.edge_outcome(function, header, EdgeKind::LoopBack, None)?,
                    exit: self.edge_outcome(function, header, EdgeKind::LoopExit, None)?,
                }
            }
            LoopShape::PreTest => {
                let test = self.back_edge_test(function, &members)?;
                let entries = incoming
                    .into(header)
                    .filter(|edge| return !members.contains(&edge.from))
                    .map(|edge| return edge.from)
                    .collect();
                LoopTest::PreTest {
                    test,
                    back: self.edge_outcome(function, test, EdgeKind::LoopBack, None)?,
                    exit: self.edge_outcome(function, test, EdgeKind::LoopExit, None)?,
                    entries,
                }
            }
            LoopShape::PostTest => {
                let test = self.back_edge_test(function, &members)?;
                LoopTest::PostTest {
                    test,
                    back: self.edge_outcome(function, test, EdgeKind::LoopBack, Some(header))?,
                }
            }
            LoopShape::Unstructured => return None,
        };
        return Some((header, line, test));
    }
}
