use std::collections::{BTreeMap, BTreeSet};

use super::classify::Node;
use super::model::{BlockId, CfgError, HandlerInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NaturalLoop {
    pub(super) header: BlockId,
    pub(super) body: BTreeSet<BlockId>,
}

struct Ordered {
    blocks: Vec<usize>,
    successors: Vec<Vec<usize>>,
    predecessors: Vec<Vec<usize>>,
}

fn check_handlers(block_count: usize, handlers: &[HandlerInput]) -> Result<(), CfgError> {
    for &HandlerInput {
        first_protected,
        handler,
    } in handlers
    {
        if !handler
            .index()
            .is_some_and(|index| return index < block_count)
        {
            return Err(CfgError::HandlerBlockMissing { handler });
        }
        if first_protected > handler {
            return Err(CfgError::HandlerBeforeProtected {
                first_protected,
                handler,
            });
        }
    }
    return Ok(());
}

fn has_retreating_edge(nodes: &[Node<'_>]) -> bool {
    return nodes
        .iter()
        .filter(|current| return current.block.is_reachable())
        .any(|current| {
            let from = current.block.id();
            return current.successors.iter().any(|&to| return to <= from);
        });
}

fn flow_graph(nodes: &[Node<'_>], handlers: &[HandlerInput]) -> Vec<Vec<BlockId>> {
    let mut graph: Vec<Vec<BlockId>> = nodes
        .iter()
        .map(|current| {
            if current.block.is_reachable() {
                return current.successors.to_vec();
            }
            return Vec::new();
        })
        .collect();
    for &HandlerInput {
        first_protected,
        handler,
    } in handlers
    {
        for (current, successors) in nodes.iter().zip(&mut graph) {
            let id = current.block.id();
            if current.block.is_reachable() && (first_protected..handler).contains(&id) {
                successors.push(handler);
            }
        }
    }
    return graph;
}

fn reverse_postorder(graph: &[Vec<BlockId>]) -> Ordered {
    let mut visited = vec![false; graph.len()];
    let mut postorder = Vec::with_capacity(graph.len());
    let mut stack: Vec<(usize, usize)> = Vec::new();
    if let Some(entry) = visited.first_mut() {
        *entry = true;
        stack.push((0, 0));
    }
    while let Some((block, next)) = stack.last_mut() {
        let current = *block;
        let successor = graph
            .get(current)
            .and_then(|successors| return successors.get(*next))
            .and_then(|successor| return successor.index());
        let Some(successor) = successor else {
            postorder.push(current);
            stack.pop();
            continue;
        };
        *next = next.saturating_add(1);
        if let Some(seen) = visited.get_mut(successor)
            && !*seen
        {
            *seen = true;
            stack.push((successor, 0));
        }
    }
    postorder.reverse();
    let mut number = vec![None; graph.len()];
    for (position, &block) in postorder.iter().enumerate() {
        if let Some(slot) = number.get_mut(block) {
            *slot = Some(position);
        }
    }
    let successors: Vec<Vec<usize>> = postorder
        .iter()
        .map(|&block| {
            return graph
                .get(block)
                .into_iter()
                .flatten()
                .filter_map(|successor| {
                    return successor
                        .index()
                        .and_then(|index| return number.get(index).copied().flatten());
                })
                .collect();
        })
        .collect();
    let mut predecessors = vec![Vec::new(); successors.len()];
    for (from, targets) in successors.iter().enumerate() {
        for &to in targets {
            if let Some(sources) = predecessors.get_mut(to) {
                sources.push(from);
            }
        }
    }
    return Ordered {
        blocks: postorder,
        successors,
        predecessors,
    };
}

fn intersect(idom: &[Option<usize>], mut left: usize, mut right: usize) -> Option<usize> {
    while left != right {
        while left > right {
            left = idom.get(left).copied().flatten()?;
        }
        while right > left {
            right = idom.get(right).copied().flatten()?;
        }
    }
    return Some(left);
}

fn immediate_dominators(predecessors: &[Vec<usize>]) -> Vec<Option<usize>> {
    let mut idom = vec![None; predecessors.len()];
    if let Some(entry) = idom.first_mut() {
        *entry = Some(0);
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (block, sources) in predecessors.iter().enumerate().skip(1) {
            let mut processed = sources
                .iter()
                .copied()
                .filter(|&source| return idom.get(source).copied().flatten().is_some());
            let found = processed.next().and_then(|first| {
                return processed.try_fold(first, |chosen, source| {
                    return intersect(&idom, chosen, source);
                });
            });
            if let Some(slot) = idom.get_mut(block)
                && found.is_some()
                && *slot != found
            {
                *slot = found;
                changed = true;
            }
        }
    }
    return idom;
}

fn dominates(idom: &[Option<usize>], dominator: usize, mut block: usize) -> bool {
    while block > dominator {
        let Some(parent) = idom.get(block).copied().flatten() else {
            return false;
        };
        block = parent;
    }
    return block == dominator;
}

fn loop_body(predecessors: &[Vec<usize>], header: usize, tails: &[usize]) -> BTreeSet<usize> {
    let mut body = BTreeSet::new();
    let mut work = tails.to_vec();
    while let Some(block) = work.pop() {
        if block == header || !body.insert(block) {
            continue;
        }
        work.extend(predecessors.get(block).into_iter().flatten().copied());
    }
    return body;
}

pub(super) fn natural_loops(
    nodes: &[Node<'_>],
    handlers: &[HandlerInput],
) -> Result<Vec<NaturalLoop>, CfgError> {
    check_handlers(nodes.len(), handlers)?;
    if !has_retreating_edge(nodes) {
        return Ok(Vec::new());
    }
    let ordered = reverse_postorder(&flow_graph(nodes, handlers));
    let idom = immediate_dominators(&ordered.predecessors);
    let mut tails: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (block, targets) in ordered.successors.iter().enumerate() {
        for &header in targets {
            if header <= block && dominates(&idom, header, block) {
                tails.entry(header).or_default().push(block);
            }
        }
    }
    let block_id = |number: usize| {
        return ordered
            .blocks
            .get(number)
            .and_then(|&index| return nodes.get(index))
            .map(|current| return current.block.id());
    };
    let loops: BTreeMap<BlockId, BTreeSet<BlockId>> = tails
        .iter()
        .filter_map(|(&header, sources)| {
            let body = loop_body(&ordered.predecessors, header, sources)
                .into_iter()
                .filter_map(block_id)
                .collect();
            return block_id(header).map(|id| return (id, body));
        })
        .collect();
    return Ok(loops
        .into_iter()
        .map(|(header, body)| return NaturalLoop { header, body })
        .collect());
}
