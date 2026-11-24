use opcov_core::{
    cfg::{BlockId, Branch, Function, LoopShape, LoopTest, Opcode, Origin, OutcomeSite, Plan},
    report::OutcomeId,
};
use opcov_sys::{
    IS_SMART_BRANCH_JMPNZ, IS_SMART_BRANCH_JMPZ, IS_TMP_VAR, opcov_branch_null, opcov_branch_set,
    opcov_branch_truth, opcov_constant_truth, opcov_fast_call_threw, opcov_fast_call_var,
    opcov_foreach_started, opcov_match_target, zend_execute_data, zend_op, zend_op_array,
};

use crate::{
    engine::{
        Engine, FinallyEntry, FinallyProbe, FiredRef, Handler, Hit, LoopEntry, Map, Probe, Role,
        Unit, UnitId,
    },
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
pub(crate) struct Activations(Map<(usize, UnitId, u32), bool>);

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

#[derive(Debug)]
struct Slots {
    index: Vec<u32>,
    probes: Vec<(u32, Probe)>,
    opcodes: *mut zend_op,
}

impl Slots {
    fn new(last: usize, opcodes: *mut zend_op) -> Self {
        return Self {
            index: vec![0; last],
            probes: Vec::new(),
            opcodes,
        };
    }

    fn at(&mut self, op: usize) -> Option<&mut Probe> {
        let entry = self.index.get_mut(op)?;
        if *entry == 0 {
            let number = u32::try_from(self.probes.len().checked_add(1)?).ok()?;
            // SAFETY: `op` indexes `index`, which has one entry per opcode of the op array; the
            // handler is the real one the reset installed.
            let handler = unsafe { Handler::of(self.opcodes.wrapping_add(op)) };
            self.probes
                .push((u32::try_from(op).ok()?, Probe::new(handler)));
            *entry = number;
        }
        return self
            .probes
            .get_mut(position(entry.checked_sub(1)?))
            .map(|(_, probe)| return probe);
    }

    fn finish(mut self) -> (ProbeIndex, Vec<u32>, Vec<Probe>) {
        self.probes.sort_unstable_by_key(|&(op, _)| return op);
        for (number, &(op, _)) in (1..).zip(&self.probes) {
            if let Some(entry) = self.index.get_mut(position(op)) {
                *entry = number;
            }
        }
        let (ops, probes) = self.probes.into_iter().unzip();
        return (ProbeIndex(self.index), ops, probes);
    }
}

struct ProbeIndex(Vec<u32>);

impl ProbeIndex {
    fn of(&self, op: usize) -> Option<u32> {
        return self
            .0
            .get(op)
            .copied()
            .and_then(|number| return number.checked_sub(1));
    }
}

type Target = (u32, usize, Branch);

struct Draft {
    site: usize,
    decision: Decision,
    successors: Vec<Target>,
    producer: Option<(usize, u8)>,
}

pub(crate) struct Built {
    pub(crate) ops: Box<[u32]>,
    pub(crate) probes: Box<[Probe]>,
    pub(crate) sources: Box<[SourceProbe]>,
    pub(crate) owners: Box<[Option<Owner>]>,
    pub(crate) twins: Box<[(u32, u32)]>,
    pub(crate) entries: Box<[LoopEntry]>,
    pub(crate) finals: Box<[FinallyProbe]>,
    pub(crate) loops: Box<[LoopState]>,
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

fn add_source_owner(
    owners: &mut [Option<Owner>],
    twins: &mut Vec<(u32, u32)>,
    outcome: u32,
    source: u32,
) {
    match owners.get(position(outcome)) {
        Some(Some(Owner::Source(_))) => twins.push((outcome, source)),
        Some(Some(Owner::Finally(_)) | None) | None => {
            set_owner(owners, outcome, Owner::Source(source));
        }
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
        let mut grouped: Vec<(BlockId, Vec<Target>)> = Vec::new();
        let mut finals = Vec::new();
        let mut planned: Vec<&OutcomeSite> = self
            .plan
            .outcomes()
            .iter()
            .chain(self.plan.twins())
            .collect();
        planned.sort_by_key(|outcome| {
            let to = match outcome.origin {
                Origin::Edge { to, .. } => to,
                Origin::FinallyEntry { block } => block,
            };
            return (outcome.site, to, outcome.kind);
        });
        for outcome in planned {
            let number = outcome.id.get();
            match outcome.origin {
                Origin::Edge { from, to, branch } => {
                    let Some(start) = self
                        .block_start(to)
                        .filter(|&start| return start < self.last)
                    else {
                        continue;
                    };
                    match grouped.last_mut() {
                        Some((last, successors)) if *last == from => {
                            successors.push((number, start, branch));
                        }
                        Some(_) | None => grouped.push((from, vec![(number, start, branch)])),
                    }
                }
                Origin::FinallyEntry { block } => {
                    if let Some(start) = self
                        .block_start(block)
                        .filter(|&start| return start < self.last)
                    {
                        slots.at(start);
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
            slots.at(site);
            for &(_, target, _) in &successors {
                slots.at(target);
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
                    slots.at(end);
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
        owners: &mut [Option<Owner>],
    ) -> Box<[FinallyProbe]> {
        let mut attached = Vec::with_capacity(finals.len());
        for (outcome, start) in finals {
            let (Some(probe), Ok(finally_op)) = (index.of(start), u32::try_from(start)) else {
                continue;
            };
            // SAFETY: the caller passes the live op array this layout describes.
            let fast_call = unsafe { opcov_fast_call_var(self.op_array, finally_op) };
            if fast_call == u32::MAX {
                continue;
            }
            attached.push(FinallyProbe {
                probe,
                entry: FinallyEntry { outcome, fast_call },
            });
            set_owner(owners, outcome, Owner::Finally(probe));
        }
        attached.sort_unstable_by_key(|found| return found.probe);
        return attached.into_boxed_slice();
    }

    fn loops(
        &self,
        index: &ProbeIndex,
        entry_ops: &[(usize, u32)],
        owners: &[Option<Owner>],
        sources: &mut [SourceProbe],
    ) -> Box<[LoopState]> {
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
        return loops.into_boxed_slice();
    }

    pub(crate) unsafe fn build(&self) -> Built {
        let mut slots = Slots::new(self.last, self.opcodes);
        for site in self.lines {
            if let Some(slot) = slots.at(site.op) {
                slot.line = Some(site.line);
            }
        }
        // SAFETY: forwarded from the caller.
        let (drafts, finals) = unsafe { self.drafts(&mut slots) };
        let entry_ops = self.entries(&mut slots);
        let (index, ops, mut probes) = slots.finish();
        let mut owners = vec![None; self.plan.outcomes().len()];
        let mut twins = Vec::new();
        let mut sources = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let Ok(number) = u32::try_from(sources.len()) else {
                break;
            };
            let Some(source) = self.source(&draft, &index) else {
                continue;
            };
            for successor in &source.successors {
                add_source_owner(&mut owners, &mut twins, successor.outcome, number);
            }
            if let Some(slot) = get_mut(&mut probes, source.probe) {
                slot.source = Some(number);
            }
            sources.push(source);
        }
        // SAFETY: forwarded from the caller.
        let finals = unsafe { self.attach_finals(finals, &index, &mut owners) };
        let loops = self.loops(&index, &entry_ops, &owners, &mut sources);
        let mut entries: Vec<LoopEntry> = entry_ops
            .iter()
            .filter_map(|&(op, looped)| {
                return index.of(op).map(|probe| return LoopEntry { probe, looped });
            })
            .collect();
        entries.sort_unstable_by_key(|entry| return (entry.probe, entry.looped));
        entries.dedup();
        twins.sort_unstable();
        return Built {
            ops: ops.into_boxed_slice(),
            probes: probes.into_boxed_slice(),
            sources: sources.into_boxed_slice(),
            owners: owners.into_boxed_slice(),
            twins: twins.into_boxed_slice(),
            entries: entries.into_boxed_slice(),
            finals,
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

    fn loop_entry(&self, looped: u32, index: usize) -> Option<u32> {
        return get(&self.loops, looped)?.entries.get(index).copied();
    }

    unsafe fn arm_loop(&mut self, looped: u32, label: Handler) {
        let Some(source) = get(&self.loops, looped).map(|state| return state.source) else {
            return;
        };
        let mut index = 0_usize;
        while let Some(entry) = self.loop_entry(looped, index) {
            // SAFETY: forwarded from the caller.
            unsafe {
                self.arm(position(entry), Role::LoopEntry, label);
            };
            index = index.saturating_add(1);
        }
        if let Some(source) = source {
            // SAFETY: forwarded from the caller.
            unsafe { self.arm_source(source, label) };
        }
    }

    unsafe fn disarm_loop_entries(&mut self, looped: u32) {
        let mut index = 0_usize;
        while let Some(entry) = self.loop_entry(looped, index) {
            // SAFETY: forwarded from the caller.
            unsafe {
                self.disarm(position(entry), Role::LoopEntry);
            };
            index = index.saturating_add(1);
        }
    }

    pub(crate) unsafe fn arm_everything(&mut self, label: Handler) {
        for probe in 0..self.probes.len() {
            if self
                .probes
                .get(probe)
                .is_some_and(|slot| return slot.line.is_some())
            {
                // SAFETY: forwarded from the caller.
                unsafe { self.arm(probe, Role::Line, label) };
            }
        }
        for index in 0..self.finals.len() {
            if let Some(probe) = self
                .finals
                .get(index)
                .map(|found| return position(found.probe))
            {
                // SAFETY: forwarded from the caller.
                unsafe { self.arm(probe, Role::FinallyEntry, label) };
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
                let first = self
                    .twins
                    .partition_point(|&(owned, _)| return owned < outcome);
                let twins = self.twins.get(first..).unwrap_or_default();
                let count = twins.partition_point(|&(owned, _)| return owned == outcome);
                for index in first..first.saturating_add(count) {
                    if let Some(&(_, source)) = self.twins.get(index) {
                        // SAFETY: forwarded from the caller.
                        unsafe { self.arm_source(source, label) };
                    }
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
        self.counters.discarded(pending.len());
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
            self.counters.discarded(1);
            self.unwatch(pending);
        }
        self.activations.forget_unit(id);
    }

    fn unwatch(&mut self, pending: Pending) {
        self.adjust_watch(pending, None);
    }

    fn watch(&mut self, pending: Pending, label: Handler) {
        self.counters.watches_installed = self.counters.watches_installed.saturating_add(1);
        self.adjust_watch(pending, Some(label));
        self.pending.push(pending);
    }

    fn adjust_watch(&mut self, pending: Pending, label: Option<Handler>) {
        let Some(unit) = self.units.get_mut(pending.unit) else {
            return;
        };
        let Unit {
            opcodes,
            ops,
            probes,
            sources,
            ..
        } = unit;
        let Some(source) = get(sources, pending.source) else {
            return;
        };
        for successor in &source.successors {
            let (Some(slot), Some(&op)) =
                (get_mut(probes, successor.probe), get(ops, successor.probe))
            else {
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
                unsafe { installed.install(opcodes.wrapping_add(position(op))) };
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
        let newly_seen = self.take_outcome(recording, id, successor.outcome);
        let role = self
            .units
            .get(id)
            .and_then(|unit| return get(&unit.sources, source))
            .and_then(|found| return found.loop_role);
        if !newly_seen && role.is_none() {
            return;
        }
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
        let only = self
            .units
            .get(id)
            .and_then(|unit| return get(&unit.sources, pending.source))
            .and_then(|found| {
                let mut matching = found
                    .successors
                    .iter()
                    .filter(|successor| return successor.start == op);
                return match (matching.next(), matching.next()) {
                    (Some(successor), None) => Some(*successor),
                    (None | Some(_), _) => None,
                };
            });
        if let Some(successor) = only {
            self.taken(
                recording,
                id,
                frame,
                pending.source,
                successor,
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
            self.counters.discarded(1);
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
        let Some((armed, op, source)) = self.units.get(id).and_then(|unit| {
            let slot = unit.probes.get(probe)?;
            return Some((slot.armed, unit.op(probe)?, slot.source));
        }) else {
            return After::Restored;
        };
        let Ok(number) = u32::try_from(probe) else {
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
            self.emit(recording, id, Hit::Line(number));
        }
        if armed.contains(Role::LoopEntry)
            && let Some(unit) = self.units.get(id)
        {
            for looped in unit.loop_entries(number) {
                self.activations.enter(frame.key(), id, looped);
            }
        }
        if armed.contains(Role::FinallyEntry)
            && let Some(entry) = self
                .units
                .get(id)
                .and_then(|unit| return unit.finally_entry(number))
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
