use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Serialize, Serializer};

use crate::consumers::{Search, Untyped, Use};
use crate::index::{Index, IndexStats};
use crate::members::{Member, innermost_callable};
use crate::routes::Routes;
use crate::target::{Kind, Request, Resolution, Target};
use crate::tree::{FileChange, Hunks, attributed, split_via};
use crate::unaffected::MemberChange;

mod entries;
mod markdown;
mod review;

use entries::{EntryPoint, RouteTable};
pub use markdown::Links;
use review::{Impact, Risks};
pub use review::{NodeCoverage, Ratio};

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum NodeId {
    Member(Target),
    Script(String),
}

impl NodeId {
    const fn kind(&self) -> NodeKind {
        return match self {
            Self::Member(target) => match target.kind {
                Kind::Method => NodeKind::Method,
                Kind::Constant => NodeKind::Constant,
                Kind::Property => NodeKind::Property,
                Kind::Function => NodeKind::Function,
                Kind::GlobalConstant => NodeKind::GlobalConstant,
            },
            Self::Script(_) => NodeKind::Script,
        };
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Member(target) => write!(formatter, "{target}"),
            Self::Script(path) => write!(formatter, "script {path}"),
        };
    }
}

impl Serialize for NodeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.collect_str(self);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum NodeKind {
    Method,
    Constant,
    Property,
    Function,
    GlobalConstant,
    Script,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Location {
    path: String,
    first: u32,
    last: u32,
}

impl From<&Member> for Location {
    fn from(member: &Member) -> Self {
        return Self {
            path: member.path.clone(),
            first: member.first,
            last: member.last,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Site {
    path: String,
    line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    from: NodeId,
    to: NodeId,
    via: Option<String>,
}

#[derive(Debug)]
struct Calls {
    sites: BTreeSet<Site>,
    in_changed_code: bool,
    resolution: Resolution,
}

#[derive(Debug)]
struct Reached {
    location: Location,
    depth: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Revision<'a> {
    pub index: &'a Index,
    pub changes: &'a [FileChange],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphError {
    pub used: String,
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "the use search returned a use of {:?}, which names no member it searched for",
            self.used
        );
    }
}

impl std::error::Error for GraphError {}

#[derive(Debug, Default)]
struct Frontier {
    searched: BTreeMap<String, NodeId>,
    pending: Vec<Target>,
}

impl Frontier {
    fn search(&mut self, target: &Target, id: &NodeId) {
        if let Entry::Vacant(slot) = self.searched.entry(target.to_string()) {
            slot.insert(id.clone());
            self.pending.push(target.clone());
        }
    }
}

#[derive(Debug, Default)]
struct Side {
    hunks: BTreeMap<NodeId, Hunks>,
    paths: BTreeSet<String>,
    files: BTreeMap<String, Vec<Member>>,
    declared: BTreeMap<NodeId, Location>,
    reached: BTreeMap<NodeId, Reached>,
    touched: BTreeSet<NodeId>,
    edges: BTreeMap<EdgeKey, Calls>,
    untyped: BTreeSet<Untyped>,
}

impl Side {
    fn seed(&mut self, index: &Index, change: &FileChange, frontier: &mut Frontier) {
        self.paths.insert(change.path.clone());
        let members = index.members_of(&change.path);
        for member in &members {
            let id = NodeId::Member(member.target.clone());
            self.declared.insert(id.clone(), Location::from(member));
            if !change.touches(member) {
                continue;
            }
            self.touched.insert(id.clone());
            self.hunks.insert(id.clone(), change.within(member));
            self.reached.entry(id.clone()).or_insert_with(|| {
                return Reached {
                    location: Location::from(member),
                    depth: 0,
                };
            });
            frontier.search(&member.target, &id);
        }
        if let Some(lines) = index.line_count(&change.path) {
            let callables: Vec<&Member> = members
                .iter()
                .filter(|member| return member.is_callable())
                .collect();
            let outside = change.outside(&callables, lines);
            if !outside.is_empty() {
                self.hunks
                    .insert(NodeId::Script(change.path.clone()), outside);
            }
        }
        self.files.insert(change.path.clone(), members);
    }

    fn reach(&mut self, from: &NodeId, owner: Option<&Member>, site: &Site, depth: u32) {
        match self.reached.entry(from.clone()) {
            Entry::Vacant(slot) => {
                let location = owner.map_or_else(
                    || {
                        return Location {
                            path: site.path.clone(),
                            first: site.line,
                            last: site.line,
                        };
                    },
                    Location::from,
                );
                slot.insert(Reached { location, depth });
            }
            Entry::Occupied(mut slot) if owner.is_none() => {
                let location = &mut slot.get_mut().location;
                location.first = location.first.min(site.line);
                location.last = location.last.max(site.line);
            }
            Entry::Occupied(_) => {}
        }
    }

    fn follow(
        &mut self,
        index: &Index,
        found: Use,
        depth: u32,
        frontier: &mut Frontier,
    ) -> Result<(), GraphError> {
        let (used, via) = split_via(&found.target);
        let to = frontier.searched.get(used).cloned().ok_or_else(|| {
            return GraphError {
                used: found.target.clone(),
            };
        })?;
        let in_file = self
            .files
            .entry(found.path.clone())
            .or_insert_with(|| return index.members_of(&found.path));
        let enclosing = innermost_callable(in_file, found.line);
        let in_changed_code = enclosing.map_or_else(
            || return self.paths.contains(&found.path),
            |member| {
                return self
                    .touched
                    .contains(&NodeId::Member(member.target.clone()));
            },
        );
        let owner = enclosing.map(|member| return attributed(member, via));
        let from = owner.as_ref().map_or_else(
            || return NodeId::Script(found.path.clone()),
            |member| return NodeId::Member(member.target.clone()),
        );
        let resolution = found.resolution;
        let site = Site {
            path: found.path,
            line: found.line,
        };
        self.reach(&from, owner.as_ref(), &site, depth);
        if let Some(member) = &owner {
            frontier.search(&member.target, &from);
        }
        let sites = self
            .edges
            .entry(EdgeKey {
                from,
                to,
                via: via.map(str::to_owned),
            })
            .or_insert_with(|| {
                return Calls {
                    sites: BTreeSet::new(),
                    in_changed_code,
                    resolution,
                };
            });
        sites.sites.insert(site);
        sites.in_changed_code |= in_changed_code;
        sites.resolution = sites.resolution.min(resolution);
        return Ok(());
    }

    fn absorb(
        &mut self,
        index: &Index,
        search: Search,
        depth: u32,
        frontier: &mut Frontier,
    ) -> Result<(), GraphError> {
        self.untyped.extend(search.untyped);
        for found in search.uses {
            self.follow(index, found, depth, frontier)?;
        }
        return Ok(());
    }

    fn location(&self, id: &NodeId) -> Option<Location> {
        return self.reached.get(id).map_or_else(
            || return self.declared.get(id).cloned(),
            |reached| return Some(reached.location.clone()),
        );
    }
}

#[derive(Debug, Serialize)]
struct Sides<T> {
    base: T,
    head: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NodeChange {
    Added,
    Removed,
    SignatureChanged,
    BodyChanged,
    FormattingChanged,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EdgeChange {
    Added,
    Removed,
    Kept,
    NowResolves,
    NoLongerResolves,
}

#[derive(Debug, Serialize)]
struct Node {
    id: NodeId,
    kind: NodeKind,
    change: NodeChange,
    resolution: Resolution,
    depth: u32,
    entries: Vec<EntryPoint>,
    impact: Option<Impact>,
    coverage: Option<NodeCoverage>,
    changed: Sides<Option<Hunks>>,
    base: Option<Location>,
    head: Option<Location>,
}

#[derive(Debug, Serialize)]
struct Edge {
    from: NodeId,
    to: NodeId,
    via: Option<String>,
    change: EdgeChange,
    resolution: Resolution,
    base: Vec<Site>,
    head: Vec<Site>,
}

#[derive(Debug, Serialize)]
pub struct Graph {
    version: u32,
    depth: u32,
    risks: Risks,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    untyped: Sides<Vec<Untyped>>,
    index: Sides<IndexStats>,
}

fn listed(side: Option<Calls>) -> Vec<Site> {
    return side.map_or_else(Vec::new, |side| return side.sites.into_iter().collect());
}

struct Merge {
    base: Side,
    head: Side,
    modifications: BTreeMap<NodeId, MemberChange>,
}

fn node_entries(id: &NodeId, base: &Index, head: &Index, routes: &RouteTable) -> Vec<EntryPoint> {
    return match id {
        NodeId::Member(target) => {
            let class = target.class.as_deref().unwrap_or_default();
            let index = if head
                .metadata
                .get_method(class.as_bytes(), target.member.as_bytes())
                .is_some()
            {
                head
            } else {
                base
            };
            entries::of_member(index, routes, target)
        }
        NodeId::Script(path) => {
            let index = if head.positions.contains_key(path) {
                head
            } else {
                base
            };
            if entries::is_pest_file(index, path) {
                vec![EntryPoint::PestTest]
            } else {
                Vec::new()
            }
        }
    };
}

fn modifications(
    base_side: &Side,
    head_side: &Side,
    base: Revision<'_>,
    head: Revision<'_>,
) -> BTreeMap<NodeId, MemberChange> {
    let paths: BTreeSet<&str> = base
        .changes
        .iter()
        .chain(head.changes)
        .map(|change| return change.path.as_str())
        .collect();
    let paths: Vec<&str> = paths.into_iter().collect();
    let (before, after) = rayon::join(
        || return base.index.signatures(&paths),
        || return head.index.signatures(&paths),
    );
    return base_side
        .touched
        .union(&head_side.touched)
        .filter(|id| {
            return base_side.declared.contains_key(*id) && head_side.declared.contains_key(*id);
        })
        .filter_map(|id| {
            let NodeId::Member(target) = id else {
                return None;
            };
            return Some((id.clone(), before.change(&after, target)));
        })
        .collect();
}

fn proven<'e>(
    changed: impl Iterator<Item = &'e NodeId>,
    edges: &'e [Edge],
) -> BTreeSet<&'e NodeId> {
    let mut callers: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for edge in edges
        .iter()
        .filter(|edge| return edge.resolution == Resolution::Typed)
    {
        callers.entry(&edge.to).or_default().push(&edge.from);
    }
    let mut proven: BTreeSet<&NodeId> = changed.collect();
    let mut pending: Vec<&NodeId> = proven.iter().copied().collect();
    while let Some(id) = pending.pop() {
        for caller in callers.get(id).into_iter().flatten() {
            if proven.insert(caller) {
                pending.push(caller);
            }
        }
    }
    return proven;
}

impl Merge {
    fn in_diff(&self, path: &str) -> bool {
        return self.base.paths.contains(path) || self.head.paths.contains(path);
    }

    fn modified(&self, id: &NodeId) -> NodeChange {
        return match self.modifications.get(id) {
            Some(MemberChange::Signature) | None => NodeChange::SignatureChanged,
            Some(MemberChange::Body) => NodeChange::BodyChanged,
            Some(MemberChange::Formatting) => NodeChange::FormattingChanged,
        };
    }

    fn change(&self, id: &NodeId) -> NodeChange {
        let (base, head) = (&self.base, &self.head);
        if let NodeId::Script(path) = id {
            return match (base.paths.contains(path), head.paths.contains(path)) {
                (false, false) => NodeChange::Unchanged,
                (true, true) => NodeChange::BodyChanged,
                (false, true) => NodeChange::Added,
                (true, false) => NodeChange::Removed,
            };
        }
        return match (base.touched.contains(id), head.touched.contains(id)) {
            (false, false) => NodeChange::Unchanged,
            (true, true) => self.modified(id),
            (false, true) if base.declared.contains_key(id) => self.modified(id),
            (false, true) => NodeChange::Added,
            (true, false) if head.declared.contains_key(id) => self.modified(id),
            (true, false) => NodeChange::Removed,
        };
    }

    fn locations(&self, id: &NodeId) -> (Option<Location>, Option<Location>) {
        let (base, head) = (self.base.location(id), self.head.location(id));
        let same = |other: &Option<Location>| {
            return other
                .as_ref()
                .filter(|location| {
                    return matches!(id, NodeId::Member(_)) && !self.in_diff(&location.path);
                })
                .cloned();
        };
        return match (base, head) {
            (None, head) => (same(&head), head),
            (base, None) => (base.clone(), same(&base)),
            both => both,
        };
    }

    fn depth(&self, id: &NodeId) -> u32 {
        return [&self.base, &self.head]
            .into_iter()
            .filter_map(|side| return side.reached.get(id).map(|reached| return reached.depth))
            .min()
            .unwrap_or_default();
    }

    fn node(&self, id: NodeId, proven: &BTreeSet<&NodeId>) -> Node {
        let (base, head) = self.locations(&id);
        let resolution = if proven.contains(&id) {
            Resolution::Typed
        } else {
            Resolution::ByName
        };
        return Node {
            kind: id.kind(),
            change: self.change(&id),
            resolution,
            entries: Vec::new(),
            impact: None,
            coverage: None,
            changed: Sides {
                base: self.base.hunks.get(&id).cloned(),
                head: self.head.hunks.get(&id).cloned(),
            },
            depth: self.depth(&id),
            base,
            head,
            id,
        };
    }

    fn nodes(&self, edges: &[Edge]) -> Vec<Node> {
        let ids: BTreeSet<&NodeId> = self
            .base
            .reached
            .keys()
            .chain(self.head.reached.keys())
            .collect();
        let proven = proven(
            ids.iter().copied().filter(|id| return self.depth(id) == 0),
            edges,
        );
        let mut nodes: Vec<Node> = ids
            .iter()
            .map(|id| return self.node((*id).clone(), &proven))
            .collect();
        nodes.sort_by(|left, right| return (left.depth, &left.id).cmp(&(right.depth, &right.id)));
        return nodes;
    }

    fn edges(&mut self) -> Vec<Edge> {
        let keys: BTreeSet<EdgeKey> = self
            .base
            .edges
            .keys()
            .chain(self.head.edges.keys())
            .cloned()
            .collect();
        return keys
            .into_iter()
            .map(|key| {
                let base = self.base.edges.remove(&key);
                let head = self.head.edges.remove(&key);
                let change = match (&base, &head) {
                    (Some(_), Some(_)) => EdgeChange::Kept,
                    (None, Some(head)) if !head.in_changed_code => EdgeChange::NowResolves,
                    (None, _) => EdgeChange::Added,
                    (Some(base), None) if !base.in_changed_code => EdgeChange::NoLongerResolves,
                    (Some(_), None) => EdgeChange::Removed,
                };
                let resolution = base
                    .iter()
                    .chain(&head)
                    .map(|sites| return sites.resolution)
                    .min()
                    .unwrap_or(Resolution::ByName);
                return Edge {
                    from: key.from,
                    to: key.to,
                    via: key.via,
                    change,
                    resolution,
                    base: listed(base),
                    head: listed(head),
                };
            })
            .collect();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageTarget {
    pub node: usize,
    pub path: String,
    pub function: String,
}

impl Graph {
    #[must_use]
    pub fn coverage_targets(&self) -> Vec<CoverageTarget> {
        return self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(position, node)| {
                let NodeId::Member(target) = &node.id else {
                    return None;
                };
                if !target.kind.is_callable() {
                    return None;
                }
                let base = node.base.as_ref()?;
                let function = target.class.as_ref().map_or_else(
                    || return target.member.clone(),
                    |class| return format!("{class}::{}", target.member),
                );
                return Some(CoverageTarget {
                    node: position,
                    path: base.path.clone(),
                    function,
                });
            })
            .collect();
    }

    pub fn cover(&mut self, covered: impl IntoIterator<Item = (usize, NodeCoverage)>) {
        for (position, coverage) in covered {
            if let Some(node) = self.nodes.get_mut(position) {
                node.coverage = Some(coverage);
            }
        }
        let untested = self
            .nodes
            .iter()
            .filter(|node| {
                return node
                    .coverage
                    .as_ref()
                    .is_some_and(NodeCoverage::is_untested);
            })
            .map(|node| return node.id.clone())
            .collect();
        self.risks.untested = Some(untested);
    }

    pub fn walk(
        base: Revision<'_>,
        head: Revision<'_>,
        routes: &Routes,
        within: &[String],
        depth: u32,
    ) -> Result<Self, GraphError> {
        let mut frontier = Frontier::default();
        let mut sides = [Side::default(), Side::default()];
        let revisions = [base, head];
        for (side, revision) in sides.iter_mut().zip(revisions) {
            for change in revision.changes {
                side.seed(revision.index, change, &mut frontier);
            }
        }
        for level in 1..=depth {
            if frontier.pending.is_empty() {
                break;
            }
            let request = Request {
                targets: std::mem::take(&mut frontier.pending),
                within: within.to_vec(),
            };
            Index::analyse_wanted(&[head.index, base.index], &request);
            let found = head.index.uses(&request);
            let [base_side, head_side] = &mut sides;
            base_side.absorb(base.index, base.index.uses(&request), level, &mut frontier)?;
            head_side.absorb(head.index, found, level, &mut frontier)?;
        }
        let [base_side, head_side] = sides;
        let modifications = modifications(&base_side, &head_side, base, head);
        let mut merge = Merge {
            base: base_side,
            head: head_side,
            modifications,
        };
        let edges = merge.edges();
        let mut nodes = merge.nodes(&edges);
        let table = RouteTable::new(routes, head.index);
        for node in &mut nodes {
            node.entries = node_entries(&node.id, base.index, head.index, &table);
        }
        let impacts = review::impacts(&nodes, &edges);
        for (node, impact) in nodes.iter_mut().zip(impacts) {
            node.impact = impact;
        }
        let risks = Risks::of(&nodes, &edges, table.broken);
        return Ok(Self {
            version: VERSION,
            depth,
            risks,
            nodes,
            edges,
            untyped: Sides {
                base: std::mem::take(&mut merge.base.untyped)
                    .into_iter()
                    .collect(),
                head: std::mem::take(&mut merge.head.untyped)
                    .into_iter()
                    .collect(),
            },
            index: Sides {
                base: base.index.stats(),
                head: head.index.stats(),
            },
        });
    }
}
