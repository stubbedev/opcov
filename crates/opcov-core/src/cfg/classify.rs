use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::model::{
    Block, BlockId, CfgError, Edge, Loop, LoopInput, LoopShape, OpIndex, check_loop_blocks,
};
use super::opcode::Opcode;
use crate::kind::EdgeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Successor {
    Target,
    Fallthrough,
    Arm(u32),
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Diagnostic {
    UnknownOpcode {
        block: BlockId,
        op: OpIndex,
        raw: u8,
    },
    UnexpectedSuccessorCount {
        block: BlockId,
        opcode: Opcode,
        count: usize,
    },
    UnstructuredLoop {
        header: BlockId,
    },
    UnrecognizedArmChain {
        block: BlockId,
    },
}

const fn is_exit(opcode: Opcode) -> bool {
    return matches!(
        opcode,
        Opcode::Return
            | Opcode::ReturnByRef
            | Opcode::GeneratorReturn
            | Opcode::Throw
            | Opcode::MatchError
            | Opcode::VerifyNeverType
            | Opcode::FastRet
    );
}

const fn is_two_way(opcode: Opcode) -> bool {
    return matches!(
        opcode,
        Opcode::Jmpz
            | Opcode::Jmpnz
            | Opcode::JmpzEx
            | Opcode::JmpnzEx
            | Opcode::JmpSet
            | Opcode::Coalesce
            | Opcode::AssertCheck
            | Opcode::JmpNull
            | Opcode::BindInitStaticOrJmp
            | Opcode::JmpFrameless
            | Opcode::FeResetR
            | Opcode::FeResetRw
            | Opcode::FeFetchR
            | Opcode::FeFetchRw
            | Opcode::FastCall
    );
}

const fn controls_flow(opcode: Opcode) -> bool {
    return is_exit(opcode)
        || is_two_way(opcode)
        || matches!(
            opcode,
            Opcode::Jmp | Opcode::Catch | Opcode::SwitchLong | Opcode::SwitchString | Opcode::Match
        );
}

#[must_use]
pub const fn opcode_edge_kind(opcode: Opcode, successor: Successor) -> Option<EdgeKind> {
    use EdgeKind as K;
    use Opcode as O;
    use Successor as S;
    return match (opcode, successor) {
        (O::Jmpz, S::Fallthrough) | (O::Jmpnz, S::Target) => Some(K::IfTrue),
        (O::Jmpz, S::Target) | (O::Jmpnz, S::Fallthrough) => Some(K::IfFalse),
        (O::JmpzEx | O::JmpnzEx, S::Target) => Some(K::ShortCircuit),
        (O::JmpzEx | O::JmpnzEx, S::Fallthrough) => Some(K::Evaluate),
        (O::Coalesce, S::Target) => Some(K::CoalesceValue),
        (O::Coalesce, S::Fallthrough) => Some(K::CoalesceFallback),
        (O::JmpSet, S::Target) => Some(K::TernaryValue),
        (O::JmpSet, S::Fallthrough) => Some(K::TernaryFallback),
        (O::JmpNull, S::Target) => Some(K::NullsafeNull),
        (O::JmpNull, S::Fallthrough) => Some(K::NullsafeValue),
        (O::Match, S::Arm(arm)) => Some(K::MatchArm(arm)),
        (O::Match, S::Default) => Some(K::MatchNoArm),
        (O::SwitchLong | O::SwitchString, S::Arm(case)) => Some(K::SwitchCase(case)),
        (O::SwitchLong | O::SwitchString, S::Default) => Some(K::SwitchDefault),
        (O::FeResetR | O::FeResetRw, S::Fallthrough) => Some(K::LoopEnter),
        (O::FeResetR | O::FeResetRw, S::Target) => Some(K::LoopSkip),
        (O::FeFetchR | O::FeFetchRw, S::Fallthrough) => Some(K::LoopBack),
        (O::FeFetchR | O::FeFetchRw, S::Target) => Some(K::LoopExit),
        (O::Catch, S::Fallthrough) => Some(K::Catch),
        (O::FastCall, S::Target) => Some(K::Finally),
        (O::AssertCheck | O::BindInitStaticOrJmp | O::JmpFrameless, S::Target | S::Fallthrough)
        | (O::SwitchLong | O::SwitchString, S::Fallthrough) => Some(K::Engine),
        (O::Jmp | O::Catch, S::Target) | (O::FastCall, S::Fallthrough) => Some(K::Flow),
        (other, S::Fallthrough) if !controls_flow(other) => Some(K::Flow),
        _ => None,
    };
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Node<'a> {
    pub(super) block: &'a Block,
    pub(super) successors: &'a [BlockId],
}

#[derive(Debug)]
pub(super) struct Classified {
    pub(super) edges: Vec<Edge>,
    pub(super) loops: Vec<Loop>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy)]
enum ChainRole {
    Test(EdgeKind),
    DefaultJump(EdgeKind),
    Unrecognized,
}

type LoopTests = BTreeMap<BlockId, BTreeSet<BlockId>>;
type ChainRoles = BTreeMap<BlockId, ChainRole>;

fn node<'n, 'a>(nodes: &'n [Node<'a>], id: BlockId) -> Option<&'n Node<'a>> {
    return id.index().and_then(|index| return nodes.get(index));
}

fn terminator_of(nodes: &[Node<'_>], id: BlockId) -> Option<Opcode> {
    return node(nodes, id).and_then(|found| return found.block.terminator());
}

fn check_successors(nodes: &[Node<'_>]) -> Result<(), CfgError> {
    for current in nodes {
        for &to in current.successors {
            if node(nodes, to).is_none() {
                return Err(CfgError::EdgeEndpointMissing {
                    from: current.block.id(),
                    to,
                });
            }
        }
    }
    return Ok(());
}

fn unknown_opcodes(nodes: &[Node<'_>]) -> Vec<Diagnostic> {
    return nodes
        .iter()
        .flat_map(|current| {
            let block = current.block;
            return (block.op_start().get()..=block.op_end().get())
                .map(OpIndex::new)
                .zip(block.ops())
                .filter_map(move |(op, entry)| {
                    if let Opcode::Unknown(raw) = entry.opcode {
                        return Some(Diagnostic::UnknownOpcode {
                            block: block.id(),
                            op,
                            raw,
                        });
                    }
                    return None;
                });
        })
        .collect();
}

fn loop_shape(
    nodes: &[Node<'_>],
    header: BlockId,
    members: &BTreeSet<BlockId>,
) -> (LoopShape, Option<BlockId>) {
    if matches!(
        terminator_of(nodes, header),
        Some(Opcode::FeFetchR | Opcode::FeFetchRw)
    ) {
        return (LoopShape::Foreach, None);
    }
    let Some(test) = members.last().and_then(|&last| return node(nodes, last)) else {
        return (LoopShape::Unstructured, None);
    };
    if !matches!(test.block.terminator(), Some(Opcode::Jmpz | Opcode::Jmpnz)) {
        return (LoopShape::Unstructured, None);
    }
    let inside: Vec<BlockId> = test
        .successors
        .iter()
        .copied()
        .filter(|to| return members.contains(to))
        .collect();
    return match (test.successors.len(), inside.as_slice()) {
        (2, [back]) if *back == header => (LoopShape::PostTest, Some(test.block.id())),
        (2, [_]) => (LoopShape::PreTest, Some(test.block.id())),
        _ => (LoopShape::Unstructured, None),
    };
}

fn classify_loops(
    nodes: &[Node<'_>],
    inputs: &[LoopInput],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(Vec<Loop>, LoopTests), CfgError> {
    let mut loops = BTreeMap::new();
    let mut tests = LoopTests::new();
    for input in inputs {
        let header = input.header;
        let members: BTreeSet<BlockId> = std::iter::once(header)
            .chain(input.body_blocks.iter().copied())
            .collect();
        check_loop_blocks(nodes.len(), header, members.iter().copied())?;
        let (shape, test) = loop_shape(nodes, header, &members);
        if shape == LoopShape::Unstructured {
            diagnostics.push(Diagnostic::UnstructuredLoop { header });
        }
        let classified = Loop::new(header, input.body_blocks.clone(), shape)?;
        if loops.insert(header, classified).is_some() {
            return Err(CfgError::DuplicateLoopHeader { header });
        }
        if let Some(test) = test {
            tests.insert(test, members);
        }
    }
    return Ok((loops.into_values().collect(), tests));
}

fn forward_jmpnz_target(candidate: &Node<'_>, tests: &LoopTests) -> Option<BlockId> {
    let id = candidate.block.id();
    if !candidate.block.is_reachable() || tests.contains_key(&id) {
        return None;
    }
    if candidate.block.terminator() != Some(Opcode::Jmpnz) {
        return None;
    }
    return match candidate.successors {
        &[target, _] if target > id => Some(target),
        _ => None,
    };
}

fn lone_jump_target(candidate: &Node<'_>) -> Option<BlockId> {
    let is_lone_jump = matches!(candidate.block.ops(), [only] if only.opcode == Opcode::Jmp);
    return match candidate.successors {
        &[target] if is_lone_jump && candidate.block.is_reachable() => Some(target),
        _ => None,
    };
}

fn rank(bodies: &BTreeSet<BlockId>, target: BlockId) -> Option<u32> {
    return bodies
        .iter()
        .position(|&body| return body == target)
        .and_then(|position| return u32::try_from(position).ok());
}

fn arm_role(bodies: &BTreeSet<BlockId>, target: BlockId) -> Option<Successor> {
    return rank(bodies, target).map(Successor::Arm);
}

fn match_default_role(
    nodes: &[Node<'_>],
    bodies: &BTreeSet<BlockId>,
    target: BlockId,
) -> Option<Successor> {
    if terminator_of(nodes, target) == Some(Opcode::MatchError) {
        return Some(Successor::Default);
    }
    return arm_role(bodies, target);
}

fn match_bodies(nodes: &[Node<'_>], arms: &[BlockId], default: BlockId) -> BTreeSet<BlockId> {
    let default_arm =
        (terminator_of(nodes, default) != Some(Opcode::MatchError)).then_some(default);
    return arms.iter().copied().chain(default_arm).collect();
}

fn match_roles(nodes: &[Node<'_>], successors: &[BlockId]) -> Option<Vec<Successor>> {
    let (&default, arms) = successors.split_last()?;
    let bodies = match_bodies(nodes, arms, default);
    return arms
        .iter()
        .map(|&arm| return arm_role(&bodies, arm))
        .chain(std::iter::once(match_default_role(nodes, &bodies, default)))
        .collect();
}

fn switch_roles(successors: &[BlockId]) -> Option<Vec<Successor>> {
    let (_, with_default) = successors.split_last()?;
    let (_, cases) = with_default.split_last()?;
    let bodies: BTreeSet<BlockId> = cases.iter().copied().collect();
    return cases
        .iter()
        .map(|&case| return arm_role(&bodies, case))
        .chain([Some(Successor::Default), Some(Successor::Fallthrough)])
        .collect();
}

fn successor_roles(
    nodes: &[Node<'_>],
    opcode: Opcode,
    successors: &[BlockId],
) -> Option<Vec<Successor>> {
    let count = successors.len();
    return match opcode {
        exit if is_exit(exit) => (count == 0).then(Vec::new),
        Opcode::Jmp => (count == 1).then(|| return vec![Successor::Target]),
        branch if is_two_way(branch) => {
            (count == 2).then(|| return vec![Successor::Target, Successor::Fallthrough])
        }
        Opcode::Catch if count == 1 => Some(vec![Successor::Fallthrough]),
        Opcode::Catch if count == 2 => Some(vec![Successor::Target, Successor::Fallthrough]),
        Opcode::Catch => None,
        Opcode::SwitchLong | Opcode::SwitchString => switch_roles(successors),
        Opcode::Match => match_roles(nodes, successors),
        _ => (count == 1).then(|| return vec![Successor::Fallthrough]),
    };
}

fn is_match_test(member: &Node<'_>) -> bool {
    let ops = member.block.ops();
    let test = ops
        .len()
        .checked_sub(2)
        .and_then(|index| return ops.get(index));
    return test
        .is_some_and(|op| return matches!(op.opcode, Opcode::CaseStrict | Opcode::IsIdentical));
}

fn chain_roles(
    nodes: &[Node<'_>],
    members: &[(&Node<'_>, BlockId)],
    default: (&Node<'_>, BlockId),
) -> Option<ChainRoles> {
    let (default_node, default_target) = default;
    let default_id = default_node.block.id();
    if default_target <= default_id
        || members
            .iter()
            .any(|&(_, target)| return target <= default_id)
    {
        return None;
    }
    let targets: Vec<BlockId> = members.iter().map(|&(_, target)| return target).collect();
    let is_match = members
        .iter()
        .all(|&(member, _)| return is_match_test(member));
    let (construct, bodies, default_role) = if is_match {
        let bodies = match_bodies(nodes, &targets, default_target);
        let role = match_default_role(nodes, &bodies, default_target)?;
        (Opcode::Match, bodies, role)
    } else {
        (
            Opcode::SwitchLong,
            targets.iter().copied().collect(),
            Successor::Default,
        )
    };
    let mut roles = ChainRoles::new();
    for &(member, target) in members {
        let kind = opcode_edge_kind(construct, arm_role(&bodies, target)?)?;
        roles.insert(member.block.id(), ChainRole::Test(kind));
    }
    roles.insert(
        default_id,
        ChainRole::DefaultJump(opcode_edge_kind(construct, default_role)?),
    );
    return Some(roles);
}

fn find_chains(
    nodes: &[Node<'_>],
    tests: &LoopTests,
    diagnostics: &mut Vec<Diagnostic>,
) -> ChainRoles {
    let mut roles = ChainRoles::new();
    for (position, start) in nodes.iter().enumerate() {
        let start_id = start.block.id();
        if roles.contains_key(&start_id) || forward_jmpnz_target(start, tests).is_none() {
            continue;
        }
        let mut members = Vec::new();
        let mut default = None;
        for later in nodes.iter().skip(position) {
            if let Some(target) = forward_jmpnz_target(later, tests) {
                members.push((later, target));
            } else if let Some(target) = lone_jump_target(later) {
                default = Some((later, target));
                break;
            }
        }
        if let Some(found) =
            default.and_then(|default| return chain_roles(nodes, &members, default))
        {
            roles.extend(found);
            continue;
        }
        diagnostics.push(Diagnostic::UnrecognizedArmChain { block: start_id });
        roles.extend(
            members
                .iter()
                .map(|&(member, _)| return (member.block.id(), ChainRole::Unrecognized)),
        );
    }
    return roles;
}

fn generic_kinds(nodes: &[Node<'_>], current: &Node<'_>, opcode: Opcode) -> Option<Vec<EdgeKind>> {
    return successor_roles(nodes, opcode, current.successors)?
        .into_iter()
        .map(|role| return opcode_edge_kind(opcode, role))
        .collect();
}

fn successor_kinds(
    nodes: &[Node<'_>],
    current: &Node<'_>,
    tests: &LoopTests,
    chains: &ChainRoles,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<EdgeKind>, CfgError> {
    let id = current.block.id();
    let opcode = current
        .block
        .terminator()
        .ok_or(CfgError::EmptyBlock { block: id })?;
    let fallback = vec![EdgeKind::Jump; current.successors.len()];
    if let Some(members) = tests.get(&id) {
        return Ok(current
            .successors
            .iter()
            .map(|to| {
                return if members.contains(to) {
                    EdgeKind::LoopBack
                } else {
                    EdgeKind::LoopExit
                };
            })
            .collect());
    }
    if let Some(&role) = chains.get(&id) {
        return Ok(match role {
            ChainRole::Test(kind) => vec![kind, EdgeKind::Flow],
            ChainRole::DefaultJump(kind) => vec![kind],
            ChainRole::Unrecognized => fallback,
        });
    }
    if let Some(kinds) = generic_kinds(nodes, current, opcode) {
        return Ok(kinds);
    }
    diagnostics.push(Diagnostic::UnexpectedSuccessorCount {
        block: id,
        opcode,
        count: current.successors.len(),
    });
    return Ok(fallback);
}

pub(super) fn classify(
    nodes: &[Node<'_>],
    loop_inputs: &[LoopInput],
) -> Result<Classified, CfgError> {
    check_successors(nodes)?;
    let mut diagnostics = unknown_opcodes(nodes);
    let (loops, tests) = classify_loops(nodes, loop_inputs, &mut diagnostics)?;
    let chains = find_chains(nodes, &tests, &mut diagnostics);
    let mut edges = BTreeSet::new();
    for current in nodes
        .iter()
        .filter(|candidate| return candidate.block.is_reachable())
    {
        let kinds = successor_kinds(nodes, current, &tests, &chains, &mut diagnostics)?;
        let from = current.block.id();
        edges.extend(
            current
                .successors
                .iter()
                .zip(kinds)
                .map(|(&to, kind)| return Edge { from, to, kind }),
        );
    }
    return Ok(Classified {
        edges: edges.into_iter().collect(),
        loops,
        diagnostics,
    });
}
