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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    outcomes: Vec<OutcomeSite>,
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
    let both_fall_through = function
        .edges()
        .iter()
        .filter(|other| return other.from == edge.from && other.to == edge.to)
        .nth(1)
        .is_some();
    if !both_fall_through {
        return Branch::Fallthrough;
    }
    let fallthrough_kind =
        super::classify::opcode_edge_kind(opcode, super::classify::Successor::Fallthrough);
    if fallthrough_kind == Some(edge.kind) {
        return Branch::Fallthrough;
    }
    return Branch::Target;
}

struct Draft {
    site: OpIndex,
    kind: EdgeKind,
    line: Line,
    origin: Origin,
}

impl Draft {
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
        let outcomes: Vec<OutcomeSite> = (0..)
            .zip(drafts)
            .map(|(number, draft)| {
                return OutcomeSite {
                    id: OutcomeId::new(number),
                    site: draft.site,
                    kind: draft.kind,
                    line: draft.line,
                    origin: draft.origin,
                };
            })
            .collect();
        let mut plan = Self {
            outcomes,
            loops: Vec::new(),
        };
        plan.loops = (0..)
            .zip(
                function
                    .loops()
                    .iter()
                    .filter_map(|cfg_loop| return plan.loop_site(function, cfg_loop)),
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
    pub fn loops(&self) -> &[LoopSite] {
        return &self.loops;
    }

    #[must_use]
    pub fn outcome(&self, id: OutcomeId) -> Option<&OutcomeSite> {
        return usize::try_from(id.get())
            .ok()
            .and_then(|index| return self.outcomes.get(index));
    }

    fn edge_outcome(
        &self,
        from: BlockId,
        kind: EdgeKind,
        to: Option<BlockId>,
    ) -> Option<OutcomeId> {
        return self
            .outcomes
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

    fn leaves(&self, from: BlockId, members: &BTreeSet<BlockId>) -> bool {
        return self.outcomes.iter().any(|outcome| {
            return outcome.kind == EdgeKind::LoopExit
                && matches!(
                    outcome.origin,
                    Origin::Edge { from: source, to, .. } if source == from && !members.contains(&to)
                );
        });
    }

    fn back_edge_test(&self, members: &BTreeSet<BlockId>) -> Option<BlockId> {
        let mut tests = self.outcomes.iter().filter_map(|outcome| {
            return match outcome.origin {
                Origin::Edge { from, to, .. }
                    if outcome.kind == EdgeKind::LoopBack
                        && members.contains(&from)
                        && members.contains(&to)
                        && self.leaves(from, members) =>
                {
                    Some(from)
                }
                Origin::Edge { .. } | Origin::FinallyEntry { .. } => None,
            };
        });
        let test = tests.next()?;
        return tests.all(|other| return other == test).then_some(test);
    }

    fn loop_site(
        &self,
        function: &Function,
        cfg_loop: &super::model::Loop,
    ) -> Option<(BlockId, Line, LoopTest)> {
        let header = cfg_loop.header();
        let line = function.block(header)?.ops().first()?.line;
        let members: BTreeSet<BlockId> = cfg_loop.members().collect();
        let test = match cfg_loop.kind() {
            LoopShape::Foreach => {
                let reset = function
                    .edges()
                    .iter()
                    .find(|edge| return edge.to == header && edge.kind == EdgeKind::LoopEnter)?
                    .from;
                LoopTest::Foreach {
                    skip: self.edge_outcome(reset, EdgeKind::LoopSkip, None)?,
                    back: self.edge_outcome(header, EdgeKind::LoopBack, None)?,
                    exit: self.edge_outcome(header, EdgeKind::LoopExit, None)?,
                }
            }
            LoopShape::PreTest => {
                let test = self.back_edge_test(&members)?;
                let entries = function
                    .edges()
                    .iter()
                    .filter(|edge| return edge.to == header && !members.contains(&edge.from))
                    .map(|edge| return edge.from)
                    .collect();
                LoopTest::PreTest {
                    test,
                    back: self.edge_outcome(test, EdgeKind::LoopBack, None)?,
                    exit: self.edge_outcome(test, EdgeKind::LoopExit, None)?,
                    entries,
                }
            }
            LoopShape::PostTest => {
                let test = self.back_edge_test(&members)?;
                LoopTest::PostTest {
                    test,
                    back: self.edge_outcome(test, EdgeKind::LoopBack, Some(header))?,
                }
            }
            LoopShape::Unstructured => return None,
        };
        return Some((header, line, test));
    }
}
