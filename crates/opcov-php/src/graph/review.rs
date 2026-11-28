use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::entries::BrokenRoute;
use super::{Edge, EdgeChange, Node, NodeChange, NodeId};
use crate::target::Resolution;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(super) struct Link {
    pub(super) from: NodeId,
    pub(super) to: NodeId,
    pub(super) via: Option<String>,
}

impl From<&Edge> for Link {
    fn from(edge: &Edge) -> Self {
        return Self {
            from: edge.from.clone(),
            to: edge.to.clone(),
            via: edge.via.clone(),
        };
    }
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Risks {
    pub(super) stops_resolving: Vec<Link>,
    pub(super) starts_resolving: Vec<Link>,
    pub(super) untouched_signature_callers: Vec<Link>,
    pub(super) unproven_uses: Vec<Link>,
    pub(super) broken_routes: Vec<BrokenRoute>,
    pub(super) untested: Option<Vec<NodeId>>,
}

impl Risks {
    pub(super) fn of(nodes: &[Node], edges: &[Edge], broken_routes: Vec<BrokenRoute>) -> Self {
        let by_id: BTreeMap<&NodeId, &Node> =
            nodes.iter().map(|node| return (&node.id, node)).collect();
        let change = |id: &NodeId| return by_id.get(id).map(|node| return node.change);
        let changed = |id: &NodeId| return by_id.get(id).is_some_and(|node| return node.depth == 0);
        let mut risks = Self {
            broken_routes,
            ..Self::default()
        };
        for edge in edges {
            let link = Link::from(edge);
            match edge.change {
                EdgeChange::NoLongerResolves => risks.stops_resolving.push(link.clone()),
                EdgeChange::NowResolves => risks.starts_resolving.push(link.clone()),
                EdgeChange::Added | EdgeChange::Removed | EdgeChange::Kept => {}
            }
            if edge.head.is_empty() {
                continue;
            }
            if change(&edge.to) == Some(NodeChange::SignatureChanged)
                && change(&edge.from) == Some(NodeChange::Unchanged)
            {
                risks.untouched_signature_callers.push(link.clone());
            }
            if edge.resolution == Resolution::ByName && changed(&edge.to) {
                risks.unproven_uses.push(link);
            }
        }
        return risks;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) struct Impact {
    pub(super) callers: usize,
    pub(super) proven_callers: usize,
    pub(super) entry_points: usize,
}

fn reaching<'e>(
    start: &'e NodeId,
    edges: &'e [Edge],
    follows: impl Fn(&Edge) -> bool,
) -> BTreeSet<&'e NodeId> {
    let mut callers: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for edge in edges.iter().filter(|edge| return follows(edge)) {
        callers.entry(&edge.to).or_default().push(&edge.from);
    }
    let mut reached: BTreeSet<&NodeId> = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(id) = pending.pop() {
        for caller in callers.get(id).into_iter().flatten() {
            if *caller != start && reached.insert(caller) {
                pending.push(caller);
            }
        }
    }
    return reached;
}

pub(super) fn impacts(nodes: &[Node], edges: &[Edge]) -> Vec<Option<Impact>> {
    let entries: BTreeMap<&NodeId, usize> = nodes
        .iter()
        .map(|node| return (&node.id, node.entries.len()))
        .collect();
    return nodes
        .iter()
        .map(|node| {
            if node.depth != 0 {
                return None;
            }
            let callers = reaching(&node.id, edges, |_| return true);
            let proven = reaching(&node.id, edges, |edge| {
                return edge.resolution == Resolution::Typed;
            });
            let entry_points = callers
                .iter()
                .copied()
                .chain(std::iter::once(&node.id))
                .map(|id| return entries.get(id).copied().unwrap_or_default())
                .sum();
            return Some(Impact {
                callers: callers.len(),
                proven_callers: proven.len(),
                entry_points,
            });
        })
        .collect();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Ratio {
    pub covered: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NodeCoverage {
    Measured {
        lines: Ratio,
        outcomes: Ratio,
        tests: Vec<String>,
    },
    NeverLoaded,
    Unmeasured {
        reason: String,
    },
}

impl NodeCoverage {
    pub(super) const fn is_untested(&self) -> bool {
        return match self {
            Self::Measured { tests, .. } => tests.is_empty(),
            Self::NeverLoaded => true,
            Self::Unmeasured { .. } => false,
        };
    }
}
