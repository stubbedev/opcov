use std::collections::{BTreeMap, HashMap};

use opcov_core::{
    cfg::{BlockId, Branch, Function, LoopShape, LoopTest, Opcode, Origin, Plan},
    report::OutcomeId,
};
use opcov_sys::{
    IS_SMART_BRANCH_JMPNZ, IS_SMART_BRANCH_JMPZ, IS_TMP_VAR, opcov_branch_null, opcov_branch_set,
    opcov_branch_truth, opcov_constant_truth, opcov_fast_call_threw, opcov_fast_call_var,
    opcov_foreach_started, opcov_match_target, zend_execute_data, zend_op, zend_op_array,
};

use crate::{
    engine::{Engine, FinallyEntry, FiredRef, Handler, Hit, Probe, Role, Unit, UnitId},
    lines::{Instr, ProbeSite},
};

#[cfg(feature = "engine-selftest")]
pub(crate) static FORCE_WATCH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "engine-selftest")]
fn forced_watch() -> bool {
    return FORCE_WATCH.load(std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(feature = "engine-selftest"))]
const fn forced_watch() -> bool {
    return false;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Truth { jump_when: bool },
    Set,
    Null,
    Always,
    Table,
    Watch,
}

impl Decision {
    const fn of(opcode: Opcode) -> Self {
        return match opcode {
            Opcode::Jmpz | Opcode::JmpzEx => Self::Truth { jump_when: false },
            Opcode::Jmpnz | Opcode::JmpnzEx | Opcode::JmpSet => Self::Truth { jump_when: true },
            Opcode::Coalesce => Self::Set,
            Opcode::JmpNull => Self::Null,
            Opcode::Jmp | Opcode::FastCall => Self::Always,
            Opcode::Match => Self::Table,
            _ => Self::Watch,
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Successor {
    pub(crate) start: usize,
    pub(crate) branch: Branch,
    pub(crate) outcome: u32,
    pub(crate) probe: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Producer {
    pub(crate) op: usize,
    pub(crate) probe: Option<u32>,
    pub(crate) smart: (u8, Handler),
    pub(crate) plain: (u8, Handler),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopRole {
    Skip { looped: u32, skip: u32 },
    Fetch { looped: u32, back: u32, exit: u32 },
    PreTest { looped: u32, back: u32, exit: u32 },
    PostTest { looped: u32, back: u32 },
}

#[derive(Debug, Clone)]
pub(crate) struct SourceProbe {
    pub(crate) probe: u32,
    pub(crate) decision: Decision,
    pub(crate) successors: Box<[Successor]>,
    pub(crate) producer: Option<Producer>,
    pub(crate) loop_role: Option<LoopRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    Source(u32),
    Finally(u32),
}

#[derive(Debug, Clone)]
pub(crate) struct LoopState {
    pub(crate) shape: LoopShape,
    pub(crate) zero_possible: bool,
    pub(crate) zero_seen: bool,
    pub(crate) second_seen: bool,
    pub(crate) entries: Box<[u32]>,
    pub(crate) source: Option<u32>,
}

impl LoopState {
    const fn saturated(&self) -> bool {
        return (self.zero_seen || !self.zero_possible) && self.second_seen;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pending {
    frame: usize,
    unit: UnitId,
    source: u32,
    started: Option<bool>,
}

#[derive(Debug, Default)]
pub(crate) struct Activations(HashMap<(usize, UnitId, u32), bool>);

impl Activations {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    fn enter(&mut self, frame: usize, unit: UnitId, looped: u32) {
        self.0.insert((frame, unit, looped), false);
    }

    fn iterate(&mut self, frame: usize, unit: UnitId, looped: u32) -> bool {
        return self.0.insert((frame, unit, looped), true).unwrap_or(false);
    }

    fn leave(&mut self, frame: usize, unit: UnitId, looped: u32) -> bool {
        return self.0.remove(&(frame, unit, looped)).unwrap_or(false);
    }

    fn forget(&mut self, unit: UnitId, looped: u32) {
        self.0
            .retain(|&(_, owner, which), _| return owner != unit || which != looped);
    }

    pub(crate) fn forget_unit(&mut self, unit: UnitId) {
        self.0.retain(|&(_, owner, _), _| return owner != unit);
    }
}

pub(crate) fn zero_possible(
    plan: &Plan,
    function: &Function,
    opcodes: *const zend_op,
) -> Box<[bool]> {
    return plan
        .loops()
        .iter()
        .map(|planned| {
            return match &planned.test {
                LoopTest::Foreach { .. } => true,
                LoopTest::PostTest { .. } => false,
                LoopTest::PreTest { test, .. } => function
                    .block(*test)
                    .and_then(|block| return usize::try_from(block.op_end().get()).ok())
                    .is_none_or(|op| {
                        // SAFETY: the caller passes the live opcodes of the function the plan
                        // describes; the test block's terminator lies within them.
                        let truth = unsafe { opcov_constant_truth(opcodes.wrapping_add(op)) };
                        return truth != 1;
                    }),
            };
        })
        .collect();
}

#[derive(Debug, Default)]
struct Slots {
    by_op: BTreeMap<usize, Probe>,
}

impl Slots {
    fn at(&mut self, op: usize, opcodes: *mut zend_op) -> &mut Probe {
        return self.by_op.entry(op).or_insert_with(|| {
            // SAFETY: the caller only passes opcode indexes below the op array's `last`; the
            // handler is the real one the reset installed.
            return Probe::new(op, unsafe { Handler::of(opcodes.wrapping_add(op)) });
        });
    }

    fn into_index(self, last: usize) -> (Box<[u32]>, Vec<Probe>) {
        let mut index = vec![0_u32; last].into_boxed_slice();
        let mut probes = Vec::with_capacity(self.by_op.len());
        for (op, probe) in self.by_op {
            if let (Some(entry), Ok(number)) = (
                index.get_mut(op),
                u32::try_from(probes.len().saturating_add(1)),
            ) {
                *entry = number;
                probes.push(probe);
            }
        }
        return (index, probes);
    }
}

struct ProbeIndex(Box<[u32]>);

impl ProbeIndex {
    fn of(&self, op: usize) -> Option<u32> {
        return self
            .0
            .get(op)
            .copied()
            .and_then(|number| return number.checked_sub(1));
    }
}

struct Draft {
    site: usize,
    decision: Decision,
    successors: Vec<(u32, usize, Branch)>,
    producer: Option<(usize, u8)>,
}

pub(crate) struct Built {
    pub(crate) index: Box<[u32]>,
    pub(crate) probes: Vec<Probe>,
    pub(crate) sources: Vec<SourceProbe>,
    pub(crate) owners: Box<[Option<Owner>]>,
    pub(crate) loops: Vec<LoopState>,
}

pub(crate) struct Layout<'a> {
    pub(crate) op_array: *const zend_op_array,
    pub(crate) opcodes: *mut zend_op,
    pub(crate) last: usize,
    pub(crate) function: &'a Function,
    pub(crate) plan: &'a Plan,
    pub(crate) zero_possible: &'a [bool],
    pub(crate) lines: &'a [ProbeSite],
    pub(crate) instrs: &'a [Instr],
}

fn position(number: u32) -> usize {
    return usize::try_from(number).unwrap_or(usize::MAX);
}

fn set_owner(owners: &mut [Option<Owner>], outcome: u32, owner: Owner) {
    if let Some(slot) = owners.get_mut(position(outcome)) {
        *slot = Some(owner);
    }
}

fn owning_source(owners: &[Option<Owner>], outcome: OutcomeId) -> Option<u32> {
    return match owners.get(position(outcome.get()))? {
        Some(Owner::Source(source)) => Some(*source),
        Some(Owner::Finally(_)) | None => None,
    };
}

fn loop_role(planned: &LoopTest, looped: u32, owners: &[Option<Owner>]) -> Vec<(u32, LoopRole)> {
    let mut roles = Vec::new();
    let mut add = |outcome: OutcomeId, role: LoopRole| {
        if let Some(source) = owning_source(owners, outcome) {
            roles.push((source, role));
        }
    };
    match planned {
        LoopTest::Foreach { skip, back, exit } => {
            add(
                *skip,
                LoopRole::Skip {
                    looped,
                    skip: skip.get(),
                },
            );
            add(
                *back,
                LoopRole::Fetch {
                    looped,
                    back: back.get(),
                    exit: exit.get(),
                },
            );
        }
        LoopTest::PreTest { back, exit, .. } => add(
            *back,
            LoopRole::PreTest {
                looped,
                back: back.get(),
                exit: exit.get(),
            },
        ),
        LoopTest::PostTest { back, .. } => add(
            *back,
            LoopRole::PostTest {
                looped,
                back: back.get(),
            },
        ),
    }
    return roles;
}

impl Layout<'_> {
    fn block_start(&self, block: BlockId) -> Option<usize> {
        return self
            .function
            .block(block)
            .and_then(|found| return usize::try_from(found.op_start().get()).ok());
    }

    fn block_end(&self, block: BlockId) -> Option<usize> {
        return self
            .function
            .block(block)
            .and_then(|found| return usize::try_from(found.op_end().get()).ok());
    }

    unsafe fn producer(
        &self,
        block_start: usize,
        site: usize,
        opcode: Opcode,
    ) -> Option<(usize, u8)> {
        let wanted = match opcode {
            Opcode::Jmpz => IS_TMP_VAR | IS_SMART_BRANCH_JMPZ,
            Opcode::Jmpnz => IS_TMP_VAR | IS_SMART_BRANCH_JMPNZ,
            _ => return None,
        };
        let op = site.checked_sub(1).filter(|&op| return op >= block_start)?;
        // SAFETY: `op` lies within the block, below `last`.
        let result_type = unsafe { (*self.opcodes.wrapping_add(op)).result_type };
        return (result_type == wanted).then_some((op, result_type));
    }

    unsafe fn drafts(&self, slots: &mut Slots) -> (Vec<Draft>, Vec<(u32, usize)>) {
        let mut grouped: BTreeMap<BlockId, Vec<(u32, usize, Branch)>> = BTreeMap::new();
        let mut finals = Vec::new();
        for outcome in self.plan.outcomes() {
            let number = outcome.id.get();
            match outcome.origin {
                Origin::Edge { from, to, branch } => {
                    if let Some(start) = self
                        .block_start(to)
                        .filter(|&start| return start < self.last)
                    {
                        grouped
                            .entry(from)
                            .or_default()
                            .push((number, start, branch));
                    }
                }
                Origin::FinallyEntry { block } => {
                    if let Some(start) = self
                        .block_start(block)
                        .filter(|&start| return start < self.last)
                    {
                        slots.at(start, self.opcodes);
                        finals.push((number, start));
                    }
                }
            }
        }
        let mut drafts = Vec::with_capacity(grouped.len());
        for (from, successors) in grouped {
            let (Some(start), Some(site)) = (self.block_start(from), self.block_end(from)) else {
                continue;
            };
            let Some(instr) = self.instrs.get(site).filter(|_| return site < self.last) else {
                continue;
            };
            slots.at(site, self.opcodes);
            for &(_, target, _) in &successors {
                slots.at(target, self.opcodes);
            }
            drafts.push(Draft {
                site,
                decision: Decision::of(instr.opcode),
                successors,
                // SAFETY: the block lies within the op array.
                producer: unsafe { self.producer(start, site, instr.opcode) },
            });
        }
        return (drafts, finals);
    }

    fn entries(&self, slots: &mut Slots) -> Vec<(usize, u32)> {
        let mut entry_ops = Vec::new();
        for (number, planned) in (0_u32..).zip(self.plan.loops()) {
            let LoopTest::PreTest { entries, .. } = &planned.test else {
                continue;
            };
            for &entry in entries {
                let Some(end) = self.block_end(entry).filter(|&end| return end < self.last) else {
                    continue;
                };
                let jumps = self
                    .instrs
                    .get(end)
                    .is_some_and(|instr| return Decision::of(instr.opcode) == Decision::Always);
                if jumps {
                    slots.at(end, self.opcodes);
                    entry_ops.push((end, number));
                }
            }
        }
        return entry_ops;
    }

    fn source(&self, draft: &Draft, index: &ProbeIndex) -> Option<SourceProbe> {
        let probe = index.of(draft.site)?;
        let successors = draft
            .successors
            .iter()
            .filter_map(|&(outcome, start, branch)| {
                return index.of(start).map(|probe| {
                    return Successor {
                        start,
                        branch,
                        outcome,
                        probe,
                    };
                });
            })
            .collect();
        let producer = draft.producer.map(|(op, smart_type)| {
            // SAFETY: `op` lies below `last`; the reset installed its real handler. The plain
            // handler is filled in by `prepare_producers` before the unit is armed.
            let handler = unsafe { Handler::of(self.opcodes.wrapping_add(op)) };
            return Producer {
                op,
                probe: index.of(op),
                smart: (smart_type, handler),
                plain: (
                    smart_type & !(IS_SMART_BRANCH_JMPZ | IS_SMART_BRANCH_JMPNZ),
                    handler,
                ),
            };
        });
        return Some(SourceProbe {
            probe,
            decision: draft.decision,
            successors,
            producer,
            loop_role: None,
        });
    }

    unsafe fn attach_finals(
        &self,
        finals: Vec<(u32, usize)>,
        index: &ProbeIndex,
        probes: &mut [Probe],
        owners: &mut [Option<Owner>],
    ) {
        for (outcome, start) in finals {
            let (Some(probe), Ok(finally_op)) = (index.of(start), u32::try_from(start)) else {
                continue;
            };
            // SAFETY: the caller passes the live op array this layout describes.
            let fast_call = unsafe { opcov_fast_call_var(self.op_array, finally_op) };
            if fast_call == u32::MAX {
                continue;
            }
            if let Some(slot) = get_mut(probes, probe) {
                slot.finally = Some(FinallyEntry { outcome, fast_call });
            }
            set_owner(owners, outcome, Owner::Finally(probe));
        }
    }

    fn loops(
        &self,
        index: &ProbeIndex,
        entry_ops: &[(usize, u32)],
        owners: &[Option<Owner>],
        probes: &mut [Probe],
        sources: &mut [SourceProbe],
    ) -> Vec<LoopState> {
        let mut loops = Vec::with_capacity(self.plan.loops().len());
        for (number, planned) in (0_u32..).zip(self.plan.loops()) {
            let roles = loop_role(&planned.test, number, owners);
            let mut tested_by = None;
            for (source, role) in roles {
                if !matches!(role, LoopRole::Skip { .. }) {
                    tested_by = Some(source);
                }
                if let Some(found) = get_mut(sources, source) {
                    found.loop_role = Some(role);
                }
            }
            let entries: Box<[u32]> = entry_ops
                .iter()
                .filter(|&&(_, looped)| return looped == number)
                .filter_map(|&(op, _)| return index.of(op))
                .collect();
            for &entry in &entries {
                if let Some(slot) = get_mut(probes, entry) {
                    slot.entries = slot.entries.iter().copied().chain([number]).collect();
                }
            }
            loops.push(LoopState {
                shape: planned.test.shape(),
                zero_possible: self
                    .zero_possible
                    .get(position(number))
                    .copied()
                    .unwrap_or(true),
                zero_seen: false,
                second_seen: false,
                entries,
                source: tested_by,
            });
        }
        return loops;
    }

    pub(crate) unsafe fn build(&self) -> Built {
        let mut slots = Slots::default();
        for site in self.lines.iter().filter(|site| return site.op < self.last) {
            slots.at(site.op, self.opcodes).line = Some(site.line);
        }
        // SAFETY: forwarded from the caller.
        let (drafts, finals) = unsafe { self.drafts(&mut slots) };
        let entry_ops = self.entries(&mut slots);
        let (index, mut probes) = slots.into_index(self.last);
        let index = ProbeIndex(index);
        let mut owners = vec![None; self.plan.outcomes().len()];
        let mut sources = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let Ok(number) = u32::try_from(sources.len()) else {
                break;
            };
            let Some(source) = self.source(&draft, &index) else {
                continue;
            };
            for successor in &source.successors {
                set_owner(&mut owners, successor.outcome, Owner::Source(number));
            }
            if let Some(slot) = get_mut(&mut probes, source.probe) {
                slot.source = Some(number);
            }
            sources.push(source);
        }
        // SAFETY: forwarded from the caller.
        unsafe { self.attach_finals(finals, &index, &mut probes, &mut owners) };
        let loops = self.loops(&index, &entry_ops, &owners, &mut probes, &mut sources);
        return Built {
            index: index.0,
            probes,
            sources,
            owners: owners.into_boxed_slice(),
            loops,
        };
    }
}

fn get<T>(items: &[T], number: u32) -> Option<&T> {
    return usize::try_from(number)
        .ok()
        .and_then(|at| return items.get(at));
}

fn get_mut<T>(items: &mut [T], number: u32) -> Option<&mut T> {
    return usize::try_from(number)
        .ok()
        .and_then(|at| return items.get_mut(at));
}

impl Unit {
    fn seen(&self, outcome: u32) -> bool {
        return get(&self.seen, outcome).copied().unwrap_or(true);
    }

    fn loop_saturated(&self, looped: u32) -> bool {
        return get(&self.loops, looped).is_none_or(LoopState::saturated);
    }

    fn source_saturated(&self, source: u32) -> bool {
        let Some(found) = get(&self.sources, source) else {
            return true;
        };
        let outcomes = found
            .successors
            .iter()
            .all(|successor| return self.seen(successor.outcome));
        let looped = match found.loop_role {
            Some(LoopRole::Fetch { looped, .. } | LoopRole::PreTest { looped, .. }) => {
                self.loop_saturated(looped)
            }
            Some(LoopRole::Skip { .. } | LoopRole::PostTest { .. }) | None => true,
        };
        return outcomes && looped;
    }

    unsafe fn set_producer(&mut self, source: u32, plain: bool) {
        let Some(producer) = get(&self.sources, source).and_then(|found| return found.producer)
        else {
            return;
        };
        let Some(opcodes) = self.opcodes else {
            return;
        };
        let (result_type, handler) = if plain {
            producer.plain
        } else {
            producer.smart
        };
        let op = opcodes.wrapping_add(producer.op);
        // SAFETY: the unit is alive, so its opcodes are; the producer lies below `last`.
        unsafe { (*op).result_type = result_type };
        let labelled = producer
            .probe
            .and_then(|probe| return get_mut(&mut self.probes, probe))
            .is_some_and(|slot| {
                slot.handler = handler;
                return !slot.armed.is_empty();
            });
        if !labelled {
            // SAFETY: as above.
            unsafe { handler.install(op) };
        }
    }

    pub(crate) unsafe fn prepare_producers(&mut self, plain: &[(usize, Handler)]) {
        for source in &mut self.sources {
            if let Some(producer) = source.producer.as_mut()
                && let Some(&(_, handler)) = plain.iter().find(|&&(op, _)| return op == producer.op)
            {
                producer.plain.1 = handler;
            }
        }
    }

    pub(crate) fn producer_ops(&self) -> Vec<(usize, u8)> {
        return self
            .sources
            .iter()
            .filter_map(|source| {
                return source
                    .producer
                    .map(|producer| return (producer.op, producer.plain.0));
            })
            .collect();
    }

    pub(crate) fn drop_producers(&mut self) {
        for source in &mut self.sources {
            source.producer = None;
        }
    }

    pub(crate) fn restore_producers(&mut self) {
        let count = u32::try_from(self.sources.len()).unwrap_or(u32::MAX);
        for source in 0..count {
            // SAFETY: the unit is alive (or its opcodes are cleared and nothing is written).
            unsafe { self.set_producer(source, false) };
        }
    }

    unsafe fn arm_source(&mut self, source: u32, label: Handler) {
        let Some(probe) = get(&self.sources, source).map(|found| return found.probe) else {
            return;
        };
        let armed =
            get(&self.probes, probe).is_some_and(|slot| return slot.armed.contains(Role::Source));
        if armed {
            return;
        }
        // SAFETY: forwarded from the caller; the unit is alive.
        unsafe {
            self.set_producer(source, true);
            self.arm(position(probe), Role::Source, label);
        }
    }

    unsafe fn disarm_source(&mut self, source: u32) {
        let Some(probe) = get(&self.sources, source).map(|found| return found.probe) else {
            return;
        };
        // SAFETY: forwarded from the caller; the unit is alive.
        unsafe {
            self.disarm(position(probe), Role::Source);
            self.set_producer(source, false);
        }
    }

    unsafe fn arm_loop(&mut self, looped: u32, label: Handler) {
        let Some(state) = get(&self.loops, looped) else {
            return;
        };
        let entries = state.entries.clone();
        let source = state.source;
        for entry in entries {
            // SAFETY: forwarded from the caller.
            unsafe {
                self.arm(position(entry), Role::LoopEntry, label);
            };
        }
        if let Some(source) = source {
            // SAFETY: forwarded from the caller.
            unsafe { self.arm_source(source, label) };
        }
    }

    unsafe fn disarm_loop_entries(&mut self, looped: u32) {
        let Some(entries) = get(&self.loops, looped).map(|state| return state.entries.clone())
        else {
            return;
        };
        for entry in entries {
            // SAFETY: forwarded from the caller.
            unsafe {
                self.disarm(position(entry), Role::LoopEntry);
            };
        }
    }

    pub(crate) unsafe fn arm_everything(&mut self, label: Handler) {
        let lines: Vec<usize> = (0..self.probes.len())
            .filter(|&probe| {
                return self
                    .probes
                    .get(probe)
                    .is_some_and(|slot| return slot.line.is_some() || slot.finally.is_some());
            })
            .collect();
        for probe in lines {
            let roles: Vec<Role> = self.probes.get(probe).map_or_else(Vec::new, |slot| {
                let mut roles = Vec::new();
                if slot.line.is_some() {
                    roles.push(Role::Line);
                }
                if slot.finally.is_some() {
                    roles.push(Role::FinallyEntry);
                }
                return roles;
            });
            for role in roles {
                // SAFETY: forwarded from the caller.
                unsafe { self.arm(probe, role, label) };
            }
        }
        let count = u32::try_from(self.sources.len()).unwrap_or(u32::MAX);
        for source in 0..count {
            // SAFETY: forwarded from the caller.
            unsafe { self.arm_source(source, label) };
        }
        let loops = u32::try_from(self.loops.len()).unwrap_or(u32::MAX);
        for looped in 0..loops {
            // SAFETY: forwarded from the caller.
            unsafe { self.arm_loop(looped, label) };
        }
    }

    pub(crate) unsafe fn rearm(&mut self, hit: Hit, label: Handler) {
        match hit {
            Hit::Line(probe) => {
                // SAFETY: forwarded from the caller.
                unsafe {
                    self.arm(position(probe), Role::Line, label);
                };
            }
            Hit::Outcome(outcome) => {
                if let Some(seen) = get_mut(&mut self.seen, outcome) {
                    *seen = false;
                }
                match get(&self.owners, outcome).copied().flatten() {
                    // SAFETY: forwarded from the caller.
                    Some(Owner::Source(source)) => unsafe { self.arm_source(source, label) },
                    Some(Owner::Finally(probe)) => {
                        // SAFETY: forwarded from the caller.
                        unsafe {
                            self.arm(position(probe), Role::FinallyEntry, label);
                        };
                    }
                    None => {}
                }
            }
            Hit::Zero(looped) | Hit::Second(looped) => {
                if let Some(state) = get_mut(&mut self.loops, looped) {
                    if matches!(hit, Hit::Zero(_)) {
                        state.zero_seen = false;
                    } else {
                        state.second_seen = false;
                    }
                }
                if get(&self.loops, looped).is_some_and(|state| {
                    return state.shape == LoopShape::PreTest || state.shape == LoopShape::Foreach;
                }) {
                    // SAFETY: forwarded from the caller.
                    unsafe { self.arm_loop(looped, label) };
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) execute_data: *mut zend_execute_data,
    pub(crate) opline: *const zend_op,
}

impl Frame {
    fn key(self) -> usize {
        return self.execute_data.addr();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Taken(Successor),
    Untracked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum After {
    Restored,
    Armed,
}

impl Engine {
    pub(crate) fn drop_pending(&mut self) {
        let pending = core::mem::take(&mut self.pending);
        for entry in &pending {
            self.unwatch(*entry);
        }
    }

    pub(crate) fn exception_thrown(&mut self) {
        self.drop_pending();
    }

    pub(crate) fn forget_unit(&mut self, id: UnitId) {
        while let Some(position) = self
            .pending
            .iter()
            .position(|entry| return entry.unit == id)
        {
            let pending = self.pending.swap_remove(position);
            self.unwatch(pending);
        }
        self.activations.forget_unit(id);
    }

    fn unwatch(&mut self, pending: Pending) {
        self.adjust_watch(pending, None);
    }

    fn watch(&mut self, pending: Pending, label: Handler) {
        self.adjust_watch(pending, Some(label));
        self.pending.push(pending);
    }

    fn adjust_watch(&mut self, pending: Pending, label: Option<Handler>) {
        let Some(unit) = self.units.get_mut(pending.unit) else {
            return;
        };
        let Unit {
            opcodes,
            probes,
            sources,
            ..
        } = unit;
        let Some(source) = get(sources, pending.source) else {
            return;
        };
        for successor in &source.successors {
            let Some(slot) = get_mut(probes, successor.probe) else {
                continue;
            };
            let installed = if let Some(label) = label {
                slot.watchers = slot.watchers.saturating_add(1);
                slot.armed.insert(Role::Watch);
                label
            } else {
                slot.watchers = slot.watchers.saturating_sub(1);
                if slot.watchers > 0 {
                    continue;
                }
                slot.armed.remove(Role::Watch);
                if !slot.armed.is_empty() {
                    continue;
                }
                slot.handler
            };
            if let Some(opcodes) = *opcodes {
                // SAFETY: the unit is alive, so its opcodes are; the probe lies below `last`.
                unsafe { installed.install(opcodes.wrapping_add(slot.op)) };
            }
        }
    }

    fn emit(&mut self, recording: bool, unit: UnitId, hit: Hit) {
        if recording {
            self.record(FiredRef { unit, hit });
        }
    }

    fn take_outcome(&mut self, recording: bool, id: UnitId, outcome: u32) -> bool {
        let Some(unit) = self.units.get_mut(id) else {
            return false;
        };
        let Some(seen) = get_mut(&mut unit.seen, outcome) else {
            return false;
        };
        if *seen {
            return false;
        }
        *seen = true;
        self.emit(recording, id, Hit::Outcome(outcome));
        return true;
    }

    fn take_loop(&mut self, recording: bool, id: UnitId, looped: u32, zero: bool) {
        let Some(unit) = self.units.get_mut(id) else {
            return;
        };
        let Some(state) = get_mut(&mut unit.loops, looped) else {
            return;
        };
        let flag = if zero {
            &mut state.zero_seen
        } else {
            &mut state.second_seen
        };
        if *flag || (zero && !state.zero_possible) {
            return;
        }
        *flag = true;
        let saturated = state.saturated();
        let hit = if zero {
            Hit::Zero(looped)
        } else {
            Hit::Second(looped)
        };
        if saturated {
            // SAFETY: the unit is alive while its probes fire.
            unsafe { unit.disarm_loop_entries(looped) };
            self.activations.forget(id, looped);
        }
        self.emit(recording, id, hit);
    }

    fn taken(
        &mut self,
        recording: bool,
        id: UnitId,
        frame: Frame,
        source: u32,
        successor: Successor,
        started: Option<bool>,
    ) {
        self.take_outcome(recording, id, successor.outcome);
        let role = self
            .units
            .get(id)
            .and_then(|unit| return get(&unit.sources, source))
            .and_then(|found| return found.loop_role);
        match role {
            Some(LoopRole::Skip { looped, skip }) if successor.outcome == skip => {
                self.take_loop(recording, id, looped, true);
            }
            Some(LoopRole::Fetch { looped, back, .. })
                if successor.outcome == back && started == Some(true) =>
            {
                self.take_loop(recording, id, looped, false);
            }
            Some(LoopRole::Fetch { looped, exit, .. })
                if successor.outcome == exit && started == Some(false) =>
            {
                self.take_loop(recording, id, looped, true);
            }
            Some(LoopRole::PostTest { looped, back }) if successor.outcome == back => {
                self.take_loop(recording, id, looped, false);
            }
            Some(LoopRole::PreTest { looped, back, .. }) if successor.outcome == back => {
                if self.activations.iterate(frame.key(), id, looped) {
                    self.take_loop(recording, id, looped, false);
                }
            }
            Some(LoopRole::PreTest { looped, exit, .. }) if successor.outcome == exit => {
                if !self.activations.leave(frame.key(), id, looped) {
                    self.take_loop(recording, id, looped, true);
                }
            }
            Some(_) | None => {}
        }
        let saturated = self
            .units
            .get(id)
            .is_none_or(|unit| return unit.source_saturated(source));
        if saturated && let Some(unit) = self.units.get_mut(id) {
            // SAFETY: the unit is alive while its probes fire.
            unsafe { unit.disarm_source(source) };
        }
    }

    fn resolve_watch(&mut self, recording: bool, id: UnitId, frame: Frame, op: usize) -> bool {
        let Some(position) = self
            .pending
            .iter()
            .position(|entry| return entry.frame == frame.key() && entry.unit == id)
        else {
            return false;
        };
        let pending = self.pending.swap_remove(position);
        self.unwatch(pending);
        let candidates: Vec<Successor> = self
            .units
            .get(id)
            .and_then(|unit| return get(&unit.sources, pending.source))
            .map_or_else(Vec::new, |found| {
                return found
                    .successors
                    .iter()
                    .filter(|successor| return successor.start == op)
                    .copied()
                    .collect();
            });
        if let [successor] = candidates.as_slice() {
            self.taken(
                recording,
                id,
                frame,
                pending.source,
                *successor,
                pending.started,
            );
        }
        return true;
    }

    fn discard_frame(&mut self, frame: Frame) {
        while let Some(position) = self
            .pending
            .iter()
            .position(|entry| return entry.frame == frame.key())
        {
            let pending = self.pending.swap_remove(position);
            self.unwatch(pending);
        }
    }

    unsafe fn decide(&self, id: UnitId, frame: Frame, source: &SourceProbe) -> Verdict {
        let Some(opcodes) = self.units.get(id).and_then(|unit| return unit.opcodes) else {
            return Verdict::Unknown;
        };
        let pick = |branch: Branch| -> Verdict {
            let mut found = source
                .successors
                .iter()
                .filter(|successor| return successor.branch == branch);
            return match (found.next(), found.next()) {
                (Some(successor), None) => Verdict::Taken(*successor),
                (None, _) => Verdict::Untracked,
                (Some(_), Some(_)) => Verdict::Unknown,
            };
        };
        let jump = |raw: i32, when: bool| -> Verdict {
            return match raw {
                0 => pick(if when {
                    Branch::Fallthrough
                } else {
                    Branch::Target
                }),
                1 => pick(if when {
                    Branch::Target
                } else {
                    Branch::Fallthrough
                }),
                _ => Verdict::Unknown,
            };
        };
        let op = frame.opline;
        // SAFETY: the frame and opline are the ones the VM is executing; the evaluators only read
        // the opcode's operands and never run user code.
        return unsafe {
            match source.decision {
                Decision::Truth { jump_when } => {
                    jump(opcov_branch_truth(frame.execute_data, op), jump_when)
                }
                Decision::Set => jump(opcov_branch_set(frame.execute_data, op), true),
                Decision::Null => jump(opcov_branch_null(frame.execute_data, op), true),
                Decision::Always => pick(Branch::Target),
                Decision::Table => {
                    let target = opcov_match_target(frame.execute_data, op, opcodes);
                    usize::try_from(target).map_or(Verdict::Unknown, |target| {
                        return source
                            .successors
                            .iter()
                            .find(|successor| return successor.start == target)
                            .map_or(Verdict::Untracked, |successor| {
                                return Verdict::Taken(*successor);
                            });
                    })
                }
                Decision::Watch => Verdict::Unknown,
            }
        };
    }

    pub(crate) unsafe fn trigger(
        &mut self,
        recording: bool,
        id: UnitId,
        probe: usize,
        frame: Frame,
    ) -> After {
        let Some(label) = self.label else {
            return After::Restored;
        };
        let Some((armed, op, source, finally)) = self
            .units
            .get(id)
            .and_then(|unit| return unit.probes.get(probe))
            .map(|slot| return (slot.armed, slot.op, slot.source, slot.finally))
        else {
            return After::Restored;
        };
        let watched = armed.contains(Role::Watch) && self.resolve_watch(recording, id, frame, op);
        if !watched {
            self.discard_frame(frame);
        }
        if armed.contains(Role::Line)
            && let Some(unit) = self.units.get_mut(id)
        {
            // SAFETY: the unit is alive while its probes fire.
            unsafe { unit.disarm(probe, Role::Line) };
            if let Ok(number) = u32::try_from(probe) {
                self.emit(recording, id, Hit::Line(number));
            }
        }
        if armed.contains(Role::LoopEntry) {
            let loops = self
                .units
                .get(id)
                .and_then(|unit| return unit.probes.get(probe))
                .map_or_else(Box::default, |slot| return slot.entries.clone());
            for looped in loops {
                self.activations.enter(frame.key(), id, looped);
            }
        }
        if armed.contains(Role::FinallyEntry)
            && let Some(entry) = finally
        {
            // SAFETY: the frame is the one the VM is executing; the variable is the finally's
            // fast-call slot of this op array.
            let threw = unsafe { opcov_fast_call_threw(frame.execute_data, entry.fast_call) };
            if threw {
                self.take_outcome(recording, id, entry.outcome);
                if let Some(unit) = self.units.get_mut(id) {
                    // SAFETY: as above.
                    unsafe { unit.disarm(probe, Role::FinallyEntry) };
                }
            }
        }
        if armed.contains(Role::Source)
            && let Some(number) = source
        {
            self.fire_source(recording, id, number, frame, label);
        }
        let armed = self
            .units
            .get(id)
            .and_then(|unit| return unit.probes.get(probe))
            .is_some_and(|slot| return !slot.armed.is_empty());
        return if armed { After::Armed } else { After::Restored };
    }

    fn fire_source(
        &mut self,
        recording: bool,
        id: UnitId,
        number: u32,
        frame: Frame,
        label: Handler,
    ) {
        let Some(source) = self
            .units
            .get(id)
            .and_then(|unit| return get(&unit.sources, number))
        else {
            return;
        };
        let started = matches!(source.loop_role, Some(LoopRole::Fetch { .. })).then(|| {
            // SAFETY: an FE_FETCH opcode of the running frame; only engine state is read.
            return unsafe { opcov_foreach_started(frame.execute_data, frame.opline) } == 1;
        });
        let verdict = if forced_watch() {
            Verdict::Unknown
        } else {
            // SAFETY: the frame is the one the VM is executing.
            unsafe { self.decide(id, frame, source) }
        };
        match verdict {
            Verdict::Taken(successor) => {
                self.taken(recording, id, frame, number, successor, started);
            }
            Verdict::Untracked => {}
            Verdict::Unknown => self.watch(
                Pending {
                    frame: frame.key(),
                    unit: id,
                    source: number,
                    started,
                },
                label,
            ),
        }
    }
}

pub(crate) fn on_exception() {
    if crate::engine::idle() {
        crate::engine::with_engine(Engine::exception_thrown);
    }
}
