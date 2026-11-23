use core::{cell::UnsafeCell, ffi::c_void, fmt, ptr};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    panic::{self, AssertUnwindSafe},
    sync::atomic::{AtomicBool, Ordering},
};

use opcov_core::{
    cfg::{Analysis, FunctionKey, Line},
    filter::{Selection, SkipReason},
};
use opcov_sys::{zend_op, zend_op_array};

use crate::{
    guard::{COLLECTION, Disabled},
    select::Pattern,
    threads,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OffReason {
    Panic(Disabled),
    SecondThread,
    IniRegistrationFailed,
    NotEnabledAtStartup,
    NoReservedSlot,
    NotEnabled,
    CompileHooksMissing,
    Selection(String),
    CollectPathsEmpty,
    InvalidPattern(String),
    PatternFailed(String),
    Snapshot { file: String, message: String },
}

impl fmt::Display for OffReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Panic(disabled) => write!(f, "{disabled}"),
            Self::SecondThread => write!(
                f,
                "a second PHP thread started in this process (FrankenPHP, ext-parallel); opcov collects on one PHP thread only"
            ),
            Self::IniRegistrationFailed => {
                write!(f, "the engine refused to register opcov's INI settings")
            }
            Self::NotEnabledAtStartup => write!(
                f,
                "opcov.enabled was 0 when PHP started; set opcov.enabled=1 in php.ini or with -d"
            ),
            Self::NoReservedSlot => write!(
                f,
                "PHP has no free op_array.reserved slot left for opcov (other extensions use all of them)"
            ),
            Self::NotEnabled => write!(f, "opcov.enabled is 0"),
            Self::CompileHooksMissing => {
                write!(f, "zend_compile_file or zend_compile_string is not set")
            }
            Self::Selection(message) => write!(f, "{message}"),
            Self::CollectPathsEmpty => write!(f, "opcov.collect_paths is not set"),
            Self::InvalidPattern(pattern) => write!(
                f,
                "opcov.exclude_pattern is not a valid PCRE pattern with delimiters: {pattern}"
            ),
            Self::PatternFailed(path) => {
                write!(f, "opcov.exclude_pattern failed to match against {path}")
            }
            Self::Snapshot { file, message } => {
                write!(f, "cannot analyse code compiled from {file}: {message}")
            }
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FileId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct UnitId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Handler(*const c_void);

impl Handler {
    pub(crate) const unsafe fn of(op: *const zend_op) -> Self {
        // SAFETY: the caller passes a live opcode.
        return Self(unsafe { (*op).handler });
    }

    pub(crate) unsafe fn install(self, op: *mut zend_op) {
        // SAFETY: the caller passes a live, writable opcode of the owner thread.
        unsafe { (*op).handler = self.0 };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Line,
}

impl Role {
    const fn bit(self) -> u8 {
        return match self {
            Self::Line => 1,
        };
    }

    pub(crate) const fn name(self) -> &'static str {
        return match self {
            Self::Line => "Line",
        };
    }

    const ALL: [Self; 1] = [Self::Line];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Roles(u8);

impl Roles {
    pub(crate) const fn only(role: Role) -> Self {
        return Self(role.bit());
    }

    pub(crate) const fn contains(self, role: Role) -> bool {
        return self.0 & role.bit() != 0;
    }

    pub(crate) const fn insert(&mut self, role: Role) {
        self.0 |= role.bit();
    }

    pub(crate) const fn remove(&mut self, role: Role) {
        self.0 &= !role.bit();
    }

    pub(crate) const fn is_empty(self) -> bool {
        return self.0 == 0;
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = Role> {
        return Role::ALL
            .into_iter()
            .filter(move |&role| return self.contains(role));
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Probe {
    pub(crate) op: usize,
    pub(crate) line: Line,
    pub(crate) handler: Handler,
    pub(crate) armed: Roles,
}

#[derive(Debug)]
pub(crate) struct Unit {
    pub(crate) opcodes: Option<*mut zend_op>,
    pub(crate) last: usize,
    pub(crate) file: FileId,
    pub(crate) function: FunctionKey,
    pub(crate) index: Box<[u32]>,
    pub(crate) probes: Vec<Probe>,
    pub(crate) pending: usize,
}

impl Unit {
    pub(crate) fn probe_at(&self, op: usize) -> Option<usize> {
        let number = usize::try_from(*self.index.get(op)?).ok()?;
        return number.checked_sub(1);
    }

    pub(crate) unsafe fn arm(&mut self, probe: usize, role: Role, label: Handler) {
        let Some(slot) = self.probes.get_mut(probe) else {
            return;
        };
        slot.armed.insert(role);
        if let Some(opcodes) = self.opcodes {
            // SAFETY: the unit's op array is alive (`opcodes` is cleared by its destructor) and the
            // probe's opcode index is below `last`.
            unsafe { label.install(opcodes.wrapping_add(slot.op)) };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reserved {
    Unknown,
    NoProbes,
    Unit(UnitId),
}

impl Reserved {
    const NO_PROBES: usize = 1;
    const FIRST_UNIT: usize = 2;

    pub(crate) fn decode(raw: *mut c_void) -> Self {
        return match raw.addr() {
            0 => Self::Unknown,
            Self::NO_PROBES => Self::NoProbes,
            value => Self::Unit(UnitId(value.saturating_sub(Self::FIRST_UNIT))),
        };
    }

    const fn encode(self) -> *mut c_void {
        return ptr::without_provenance_mut(match self {
            Self::Unknown => 0,
            Self::NoProbes => Self::NO_PROBES,
            Self::Unit(UnitId(id)) => id.saturating_add(Self::FIRST_UNIT),
        });
    }

    pub(crate) unsafe fn read(op_array: *const zend_op_array, slot: usize) -> Self {
        // SAFETY: the caller passes a live op array; reserved slots are plain pointer values.
        let reserved = unsafe { (*op_array).reserved };
        return reserved
            .get(slot)
            .map_or(Self::Unknown, |&raw| return Self::decode(raw));
    }

    pub(crate) unsafe fn write(self, op_array: *mut zend_op_array, slot: usize) {
        // SAFETY: the caller passes a live op array of the owner thread.
        let reserved = unsafe { &mut (*op_array).reserved };
        if let Some(entry) = reserved.get_mut(slot) {
            *entry = self.encode();
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Units {
    slots: Vec<Option<Unit>>,
    free: Vec<usize>,
    by_opcodes: HashMap<usize, UnitId>,
}

impl Units {
    pub(crate) fn get(&self, id: UnitId) -> Option<&Unit> {
        return self.slots.get(id.0).and_then(Option::as_ref);
    }

    pub(crate) fn get_mut(&mut self, id: UnitId) -> Option<&mut Unit> {
        return self.slots.get_mut(id.0).and_then(Option::as_mut);
    }

    pub(crate) fn find(&self, opcodes: *const zend_op) -> Option<UnitId> {
        return self.by_opcodes.get(&opcodes.addr()).copied();
    }

    pub(crate) fn insert(&mut self, unit: Unit) -> UnitId {
        let key = unit.opcodes.map(<*mut zend_op>::addr);
        let id = if let Some(free) = self.free.pop() {
            if let Some(slot) = self.slots.get_mut(free) {
                *slot = Some(unit);
            }
            UnitId(free)
        } else {
            self.slots.push(Some(unit));
            UnitId(self.slots.len().saturating_sub(1))
        };
        if let Some(key) = key {
            self.by_opcodes.insert(key, id);
        }
        return id;
    }

    pub(crate) fn live(&self) -> impl Iterator<Item = &Unit> {
        return self
            .slots
            .iter()
            .flatten()
            .filter(|unit| return unit.opcodes.is_some());
    }

    pub(crate) fn live_mut(&mut self) -> impl Iterator<Item = &mut Unit> {
        return self
            .slots
            .iter_mut()
            .flatten()
            .filter(|unit| return unit.opcodes.is_some());
    }

    pub(crate) fn count_live(&self) -> usize {
        return self.live().count();
    }

    fn release(&mut self, id: UnitId) {
        if let Some(slot) = self.slots.get_mut(id.0) {
            *slot = None;
            self.free.push(id.0);
        }
    }

    pub(crate) fn destroyed(&mut self, id: UnitId) {
        let Some(unit) = self.get_mut(id) else {
            return;
        };
        let key = unit.opcodes.take().map(<*mut zend_op>::addr);
        let orphan_without_hits = unit.pending == 0;
        if let Some(key) = key {
            self.by_opcodes.remove(&key);
        }
        if orphan_without_hits {
            self.release(id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProbeRef {
    pub(crate) unit: UnitId,
    pub(crate) probe: usize,
}

#[derive(Debug, Default)]
pub(crate) struct Fired {
    pub(crate) hits: Vec<ProbeRef>,
    pub(crate) strays: Vec<ProbeRef>,
    pub(crate) stopped: Vec<ProbeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FiredList {
    Hits,
    Strays,
    Stopped,
}

#[derive(Debug)]
pub(crate) struct FileRecord {
    pub(crate) path: String,
    pub(crate) lines: BTreeSet<Line>,
    pub(crate) functions: BTreeMap<FunctionKey, Analysis>,
}

#[derive(Debug, Default)]
pub(crate) struct Files {
    records: Vec<FileRecord>,
    by_path: HashMap<String, FileId>,
}

impl Files {
    pub(crate) fn intern(&mut self, path: &str) -> FileId {
        if let Some(&id) = self.by_path.get(path) {
            return id;
        }
        let id = FileId(self.records.len());
        self.records.push(FileRecord {
            path: path.to_owned(),
            lines: BTreeSet::new(),
            functions: BTreeMap::new(),
        });
        self.by_path.insert(path.to_owned(), id);
        return id;
    }

    pub(crate) fn get(&self, id: FileId) -> Option<&FileRecord> {
        return self.records.get(id.0);
    }

    pub(crate) fn get_mut(&mut self, id: FileId) -> Option<&mut FileRecord> {
        return self.records.get_mut(id.0);
    }

    pub(crate) fn find(&self, path: &str) -> Option<FileId> {
        return self.by_path.get(path).copied();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Measure(FileId),
    Skip(SkipReason),
    Off,
}

#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) selection: Option<Selection>,
    pub(crate) pattern: Option<Pattern>,
    pub(crate) decisions: BTreeMap<String, Verdict>,
    pub(crate) problem: Option<OffReason>,
}

#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) probes_installed: u64,
    pub(crate) probes_fired: u64,
    pub(crate) probes_rearmed: u64,
    pub(crate) jump_table_files: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Scratch {
    pub(crate) op_arrays: Vec<*mut zend_op_array>,
    pub(crate) lines: Vec<(FileId, Line)>,
    pub(crate) files: Vec<FileId>,
    pub(crate) value: serde_json::Value,
}

#[derive(Debug)]
pub(crate) struct Engine {
    pub(crate) slot: usize,
    pub(crate) label: Option<Handler>,
    pub(crate) request: Option<Request>,
    pub(crate) units: Units,
    pub(crate) files: Files,
    pub(crate) fired: Fired,
    pub(crate) collecting: bool,
    pub(crate) completed_cycles: u64,
    pub(crate) counters: Counters,
    pub(crate) scratch: Scratch,
}

impl Engine {
    pub(crate) fn new(slot: usize) -> Self {
        return Self {
            slot,
            label: None,
            request: None,
            units: Units::default(),
            files: Files::default(),
            fired: Fired::default(),
            collecting: false,
            completed_cycles: 0,
            counters: Counters::default(),
            scratch: Scratch::default(),
        };
    }

    pub(crate) const fn list(&mut self, list: FiredList) -> &mut Vec<ProbeRef> {
        return match list {
            FiredList::Hits => &mut self.fired.hits,
            FiredList::Strays => &mut self.fired.strays,
            FiredList::Stopped => &mut self.fired.stopped,
        };
    }

    pub(crate) fn record(&mut self, fired: ProbeRef) {
        self.counters.probes_fired = self.counters.probes_fired.saturating_add(1);
        if let Some(unit) = self.units.get_mut(fired.unit) {
            unit.pending = unit.pending.saturating_add(1);
        }
        let list = if self.collecting {
            FiredList::Hits
        } else {
            FiredList::Strays
        };
        self.list(list).push(fired);
    }

    pub(crate) fn rearm(&mut self, list: FiredList) {
        let refs = core::mem::take(self.list(list));
        let label = self
            .label
            .filter(|_| return COLLECTION.disabled().is_none());
        for fired in &refs {
            let Some(unit) = self.units.get_mut(fired.unit) else {
                continue;
            };
            unit.pending = unit.pending.saturating_sub(1);
            if unit.opcodes.is_none() {
                if unit.pending == 0 {
                    self.units.release(fired.unit);
                }
                continue;
            }
            if let Some(label) = label {
                // SAFETY: the unit is alive, so its opcodes are; the probe index comes from the
                // unit's own probe list.
                unsafe { unit.arm(fired.probe, Role::Line, label) };
            }
        }
        self.counters.probes_rearmed = self
            .counters
            .probes_rearmed
            .saturating_add(u64::try_from(refs.len()).unwrap_or(u64::MAX));
        let mut emptied = refs;
        emptied.clear();
        *self.list(list) = emptied;
    }

    pub(crate) fn rearm_all(&mut self) {
        self.rearm(FiredList::Hits);
        self.rearm(FiredList::Strays);
        self.rearm(FiredList::Stopped);
    }

    pub(crate) fn problem(&self) -> Option<&OffReason> {
        return self
            .request
            .as_ref()
            .and_then(|request| return request.problem.as_ref());
    }

    pub(crate) fn set_problem(&mut self, reason: OffReason) {
        if let Some(request) = self.request.as_mut()
            && request.problem.is_none()
        {
            request.problem = Some(reason);
        }
    }

    pub(crate) fn measuring(&self) -> bool {
        return self.label.is_some()
            && self
                .request
                .as_ref()
                .is_some_and(|request| return request.selection.is_some());
    }

    pub(crate) fn restore_every_probe(&mut self) {
        for unit in self.units.live_mut() {
            let Some(opcodes) = unit.opcodes else {
                continue;
            };
            for probe in &mut unit.probes {
                if probe.armed.is_empty() {
                    continue;
                }
                probe.armed = Roles::default();
                // SAFETY: the unit is alive, so its opcodes are; `op` is below `last`.
                unsafe { probe.handler.install(opcodes.wrapping_add(probe.op)) };
            }
        }
    }
}

struct EngineCell {
    engine: UnsafeCell<Option<Engine>>,
    busy: AtomicBool,
}

// SAFETY: the engine is only touched on the owner thread (the first thread that ran a request; on
// NTS builds the only thread): `with_engine` refuses other threads once a second thread exists, and
// before that no other thread runs PHP code. `busy` rejects re-entrant access, so at most one
// mutable reference exists at a time.
unsafe impl Sync for EngineCell {}

static ENGINE: EngineCell = EngineCell {
    engine: UnsafeCell::new(None),
    busy: AtomicBool::new(false),
};

fn enter() {
    assert!(
        !threads::foreign(),
        "opcov state touched from a thread that does not own the probes"
    );
    assert!(
        !ENGINE.busy.swap(true, Ordering::Acquire),
        "opcov state entered re-entrantly"
    );
}

pub(crate) fn with_engine<R>(body: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    enter();
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `enter` established that this is the owner thread and that no other reference
        // to the engine is live; the reference does not escape `body`.
        let engine = unsafe { &mut *ENGINE.engine.get() };
        return engine.as_mut().map(body);
    }));
    ENGINE.busy.store(false, Ordering::Release);
    return result.unwrap_or_else(|payload| panic::resume_unwind(payload));
}

pub(crate) fn install(engine: Engine) {
    enter();
    // SAFETY: as in `with_engine`; called once from module startup before any request.
    unsafe { *ENGINE.engine.get() = Some(engine) };
    ENGINE.busy.store(false, Ordering::Release);
}

pub(crate) fn after_panic() {
    if threads::foreign() || ENGINE.busy.load(Ordering::Acquire) {
        return;
    }
    with_engine(|engine| {
        engine.collecting = false;
        engine.restore_every_probe();
    });
}
