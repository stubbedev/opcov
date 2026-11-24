use core::{
    cell::{OnceCell, UnsafeCell},
    ffi::c_void,
    fmt, ptr,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    hash::BuildHasher,
    panic::{self, AssertUnwindSafe},
    sync::atomic::{AtomicBool, Ordering},
};

use foldhash::fast::FixedState;

use opcov_core::{
    cfg::{Analysis, FilePath, FunctionKey, Line, Plan},
    filter::{Selection, SkipReason},
    report::{Settings, Sha256},
};
use opcov_sys::{ZTS, zend_op, zend_op_array};

use crate::{
    branches::{Activations, LoopState, Owner, Pending, SourceProbe},
    code::{Code, Explained},
    conflicts::Conflicts,
    guard::{COLLECTION, Disabled},
    select::{Directories, Pattern},
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
    NothingSelected { compiled: usize },
    Conflicts(Conflicts),
}

impl OffReason {
    pub(crate) const fn code(&self) -> Code {
        return match self {
            Self::Panic(_) => Code::Panic,
            Self::SecondThread => Code::SecondThread,
            Self::IniRegistrationFailed => Code::IniRegistrationFailed,
            Self::NotEnabledAtStartup => Code::NotEnabledAtStartup,
            Self::NoReservedSlot => Code::NoReservedSlot,
            Self::NotEnabled => Code::NotEnabled,
            Self::CompileHooksMissing => Code::CompileHooksMissing,
            Self::Selection(_) => Code::InvalidSelection,
            Self::CollectPathsEmpty => Code::CollectPathsEmpty,
            Self::InvalidPattern(_) => Code::InvalidExcludePattern,
            Self::NothingSelected { .. } => Code::NothingSelected,
            Self::Conflicts(conflicts) => conflicts.first().code(),
        };
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            Self::NotEnabled => write!(f, "opcov.enabled is 0; set opcov.enabled=1"),
            Self::CompileHooksMissing => {
                write!(f, "zend_compile_file or zend_compile_string is not set")
            }
            Self::Selection(message) => write!(f, "{message}"),
            Self::CollectPathsEmpty => write!(
                f,
                "opcov.collect_paths is not set; set it to the files and directories to measure"
            ),
            Self::InvalidPattern(pattern) => write!(
                f,
                "opcov.exclude_pattern is not a valid PCRE pattern with delimiters: {pattern}"
            ),
            Self::NothingSelected { compiled } => write!(
                f,
                "opcov.collect_paths selected none of the {compiled} files compiled during this run"
            ),
            Self::Conflicts(_) => Ok(()),
        };
    }
}

impl fmt::Display for OffReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Conflicts(conflicts) = self {
            for (position, conflict) in conflicts.iter().enumerate() {
                if position > 0 {
                    f.write_str("; ")?;
                }
                write!(
                    f,
                    "{}",
                    Explained {
                        code: conflict.code(),
                        text: conflict
                    }
                )?;
            }
            return Ok(());
        }
        return write!(
            f,
            "{}",
            Explained {
                code: self.code(),
                text: &Described(self)
            }
        );
    }
}

struct Described<'a>(&'a OffReason);

impl fmt::Display for Described<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return self.0.describe(f);
    }
}

pub(crate) type Map<K, V> = HashMap<K, V, FixedState>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FileId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct UnitId(u32);

fn slot_of(id: u32) -> usize {
    return usize::try_from(id).unwrap_or(usize::MAX);
}

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
    Source,
    Watch,
    LoopEntry,
    FinallyEntry,
}

impl Role {
    const fn bit(self) -> u8 {
        return match self {
            Self::Line => 1,
            Self::Source => 2,
            Self::Watch => 4,
            Self::LoopEntry => 8,
            Self::FinallyEntry => 16,
        };
    }

    pub(crate) const fn name(self) -> &'static str {
        return match self {
            Self::Line => "Line",
            Self::Source => "Source",
            Self::Watch => "Watch",
            Self::LoopEntry => "LoopEntry",
            Self::FinallyEntry => "FinallyEntry",
        };
    }

    const ALL: [Self; 5] = [
        Self::Line,
        Self::Source,
        Self::Watch,
        Self::LoopEntry,
        Self::FinallyEntry,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Roles(u8);

impl Roles {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FinallyEntry {
    pub(crate) outcome: u32,
    pub(crate) fast_call: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Probe {
    pub(crate) handler: Handler,
    pub(crate) line: Option<Line>,
    pub(crate) source: Option<u32>,
    pub(crate) watchers: u16,
    pub(crate) armed: Roles,
}

impl Probe {
    pub(crate) fn new(handler: Handler) -> Self {
        return Self {
            handler,
            line: None,
            source: None,
            watchers: 0,
            armed: Roles::default(),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoopEntry {
    pub(crate) probe: u32,
    pub(crate) looped: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FinallyProbe {
    pub(crate) probe: u32,
    pub(crate) entry: FinallyEntry,
}

#[derive(Debug)]
pub(crate) struct Unit {
    pub(crate) opcodes: Option<*mut zend_op>,
    pub(crate) last: usize,
    pub(crate) file: FileId,
    pub(crate) function: FunctionId,
    pub(crate) ops: Box<[u32]>,
    pub(crate) cursor: u32,
    pub(crate) probes: Box<[Probe]>,
    pub(crate) sources: Box<[SourceProbe]>,
    pub(crate) owners: Box<[Option<Owner>]>,
    pub(crate) twins: Box<[(u32, u32)]>,
    pub(crate) entries: Box<[LoopEntry]>,
    pub(crate) finals: Box<[FinallyProbe]>,
    pub(crate) seen: Box<[bool]>,
    pub(crate) loops: Box<[LoopState]>,
    pub(crate) pending: usize,
}

impl Unit {
    pub(crate) fn probe_at(&mut self, op: usize) -> Option<usize> {
        let op = u32::try_from(op).ok()?;
        let next = usize::try_from(self.cursor).ok()?.saturating_add(1);
        let probe = if self.ops.get(next) == Some(&op) {
            next
        } else {
            self.ops.binary_search(&op).ok()?
        };
        self.cursor = u32::try_from(probe).ok()?;
        return Some(probe);
    }

    pub(crate) fn op(&self, probe: usize) -> Option<usize> {
        return self.ops.get(probe).map(|&op| return slot_of(op));
    }

    pub(crate) fn loop_entries(&self, probe: u32) -> impl Iterator<Item = u32> {
        let first = self
            .entries
            .partition_point(|entry| return entry.probe < probe);
        return self
            .entries
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(move |entry| return entry.probe == probe)
            .map(|entry| return entry.looped);
    }

    pub(crate) fn finally_entry(&self, probe: u32) -> Option<FinallyEntry> {
        return self
            .finals
            .binary_search_by_key(&probe, |found| return found.probe)
            .ok()
            .and_then(|position| return self.finals.get(position))
            .map(|found| return found.entry);
    }

    pub(crate) unsafe fn arm(&mut self, probe: usize, role: Role, label: Handler) {
        let (Some(slot), Some(&op)) = (self.probes.get_mut(probe), self.ops.get(probe)) else {
            return;
        };
        slot.armed.insert(role);
        if let Some(opcodes) = self.opcodes {
            // SAFETY: the unit's op array is alive (`opcodes` is cleared by its destructor) and the
            // probe's opcode index is below `last`.
            unsafe { label.install(opcodes.wrapping_add(slot_of(op))) };
        }
    }

    pub(crate) unsafe fn disarm(&mut self, probe: usize, role: Role) {
        let (Some(slot), Some(&op)) = (self.probes.get_mut(probe), self.ops.get(probe)) else {
            return;
        };
        slot.armed.remove(role);
        if slot.armed.is_empty()
            && let Some(opcodes) = self.opcodes
        {
            // SAFETY: as in `arm`.
            unsafe { slot.handler.install(opcodes.wrapping_add(slot_of(op))) };
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
            value => value
                .checked_sub(Self::FIRST_UNIT)
                .and_then(|id| return u32::try_from(id).ok())
                .map_or(Self::NoProbes, |id| return Self::Unit(UnitId(id))),
        };
    }

    fn encode(self) -> *mut c_void {
        return ptr::without_provenance_mut(match self {
            Self::Unknown => 0,
            Self::NoProbes => Self::NO_PROBES,
            Self::Unit(UnitId(id)) => slot_of(id).saturating_add(Self::FIRST_UNIT),
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
    free: Vec<UnitId>,
    by_opcodes: Map<usize, UnitId>,
}

impl Units {
    pub(crate) fn get(&self, id: UnitId) -> Option<&Unit> {
        return self.slots.get(slot_of(id.0)).and_then(Option::as_ref);
    }

    pub(crate) fn get_mut(&mut self, id: UnitId) -> Option<&mut Unit> {
        return self.slots.get_mut(slot_of(id.0)).and_then(Option::as_mut);
    }

    pub(crate) fn find(&self, opcodes: *const zend_op) -> Option<UnitId> {
        return self.by_opcodes.get(&opcodes.addr()).copied();
    }

    pub(crate) fn insert(&mut self, unit: Unit) -> Option<UnitId> {
        let key = unit.opcodes.map(<*mut zend_op>::addr);
        let id = if let Some(free) = self.free.pop() {
            *self.slots.get_mut(slot_of(free.0))? = Some(unit);
            free
        } else {
            let id = UnitId(u32::try_from(self.slots.len()).ok()?);
            self.slots.push(Some(unit));
            id
        };
        if let Some(key) = key {
            self.by_opcodes.insert(key, id);
        }
        return Some(id);
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
        if let Some(slot) = self.slots.get_mut(slot_of(id.0)) {
            *slot = None;
            self.free.push(id);
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
pub(crate) enum Hit {
    Line(u32),
    Outcome(u32),
    Zero(u32),
    Second(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FiredRef {
    pub(crate) unit: UnitId,
    pub(crate) hit: Hit,
}

#[derive(Debug, Default)]
pub(crate) struct Fired {
    pub(crate) hits: Vec<FiredRef>,
    pub(crate) covered: Vec<Covered>,
    pub(crate) strays: Vec<FiredRef>,
    pub(crate) stopped: Vec<FiredRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FiredList {
    Hits,
    Strays,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fingerprint(u64);

impl Fingerprint {
    pub(crate) fn of(content: &[u8]) -> Self {
        return Self(FixedState::default().hash_one(content));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Content {
    File(Fingerprint),
    Text(Sha256),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    Compiled(Content),
    Changed,
    Unreadable(String),
}

impl Source {
    fn next(current: Option<Self>, compiled: Result<Content, String>) -> Self {
        return match (current, compiled) {
            (None, Ok(content)) => Self::Compiled(content),
            (None | Some(Self::Compiled(_)), Err(error)) => Self::Unreadable(error),
            (Some(Self::Compiled(first)), Ok(then)) if first != then => Self::Changed,
            (Some(kept), _) => kept,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unmodelled {
    pub(crate) message: String,
}

impl fmt::Display for Unmodelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            f,
            "{}",
            Explained {
                code: Code::SnapshotFailed,
                text: &self.message
            }
        );
    }
}

#[derive(Debug)]
pub(crate) struct FileRecord {
    pub(crate) path: FilePath,
    pub(crate) unmodelled: Option<Unmodelled>,
    pub(crate) source: Option<Source>,
    pub(crate) lines: BTreeSet<Line>,
    pub(crate) functions: BTreeMap<FunctionKey, FunctionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FunctionId(u32);

#[derive(Debug)]
pub(crate) struct FunctionRecord {
    pub(crate) file: FileId,
    pub(crate) key: FunctionKey,
    pub(crate) analysis: Analysis,
    #[cfg(feature = "engine-selftest")]
    pub(crate) input: opcov_core::cfg::FunctionInput,
    pub(crate) plan: Plan,
    pub(crate) zero_possible: Box<[bool]>,
    pub(crate) text: OnceCell<Box<str>>,
}

impl FunctionRecord {
    pub(crate) fn key_text(&self) -> &str {
        return self
            .text
            .get_or_init(|| return self.key.to_string().into_boxed_str());
    }
}

impl FileRecord {
    pub(crate) fn compiled_from(&mut self, compiled: Result<Content, String>) {
        self.source = Some(Source::next(self.source.take(), compiled));
    }

    pub(crate) fn give_up(&mut self, message: String) {
        self.unmodelled.get_or_insert(Unmodelled { message });
    }

    pub(crate) const fn modelled(&self) -> bool {
        return self.unmodelled.is_none();
    }
}

#[derive(Debug, Default)]
pub(crate) struct Files {
    records: Vec<FileRecord>,
    by_path: Map<FilePath, FileId>,
    functions: Vec<FunctionRecord>,
}

impl Files {
    pub(crate) fn intern(&mut self, path: FilePath) -> Option<FileId> {
        if let Some(&id) = self.by_path.get(&path) {
            return Some(id);
        }
        let id = FileId(u32::try_from(self.records.len()).ok()?);
        self.records.push(FileRecord {
            path: path.clone(),
            unmodelled: None,
            source: None,
            lines: BTreeSet::new(),
            functions: BTreeMap::new(),
        });
        self.by_path.insert(path, id);
        return Some(id);
    }

    pub(crate) fn get(&self, id: FileId) -> Option<&FileRecord> {
        return self.records.get(slot_of(id.0));
    }

    pub(crate) fn get_mut(&mut self, id: FileId) -> Option<&mut FileRecord> {
        return self.records.get_mut(slot_of(id.0));
    }

    pub(crate) fn find(&self, path: &str) -> Option<FileId> {
        return self.by_path.get(path).copied();
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (FileId, &FileRecord)> {
        return self
            .records
            .iter()
            .zip(0..)
            .map(|(record, index)| return (FileId(index), record));
    }

    pub(crate) fn function(&self, id: FunctionId) -> Option<&FunctionRecord> {
        return self.functions.get(slot_of(id.0));
    }

    pub(crate) fn find_function(&self, file: FileId, key: &FunctionKey) -> Option<FunctionId> {
        return self.get(file)?.functions.get(key).copied();
    }

    pub(crate) fn add_function(&mut self, record: FunctionRecord) -> Option<FunctionId> {
        let id = FunctionId(u32::try_from(self.functions.len()).ok()?);
        let file = self.records.get_mut(slot_of(record.file.0))?;
        file.functions.insert(record.key.clone(), id);
        self.functions.push(record);
        return Some(id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Covered {
    Line(FileId, Line),
    Outcome(FunctionId, u32),
    Zero(FunctionId, u32),
    Second(FunctionId, u32),
}

impl Covered {
    pub(crate) fn order(self) -> u128 {
        let (variant, owner, number): (u128, u32, u32) = match self {
            Self::Line(FileId(file), line) => (0, file, line.get()),
            Self::Outcome(FunctionId(function), number) => (1, function, number),
            Self::Zero(FunctionId(function), number) => (2, function, number),
            Self::Second(FunctionId(function), number) => (3, function, number),
        };
        return variant.wrapping_shl(64) | u128::from(owner).wrapping_shl(32) | u128::from(number);
    }

    pub(crate) fn from_order(order: u128) -> Option<Self> {
        let part = |shift: u32| return u32::try_from(order.wrapping_shr(shift) & 0xffff_ffff).ok();
        let (variant, owner, number) = (part(64)?, part(32)?, part(0)?);
        return match variant {
            0 => Line::new(number).map(|line| return Self::Line(FileId(owner), line)),
            1 => Some(Self::Outcome(FunctionId(owner), number)),
            2 => Some(Self::Zero(FunctionId(owner), number)),
            3 => Some(Self::Second(FunctionId(owner), number)),
            _ => None,
        };
    }
}

#[derive(Debug)]
pub(crate) struct AttributedTest {
    pub(crate) name: String,
    pub(crate) covered: Vec<Covered>,
}

#[derive(Debug, Default)]
pub(crate) struct Attribution {
    tests: Vec<AttributedTest>,
    by_name: Map<String, usize>,
}

impl Attribution {
    pub(crate) fn attribute(&mut self, name: &str, covered: &[Covered]) {
        let position = if let Some(&position) = self.by_name.get(name) {
            position
        } else {
            self.tests.push(AttributedTest {
                name: name.to_owned(),
                covered: Vec::new(),
            });
            let position = self.tests.len().saturating_sub(1);
            self.by_name.insert(name.to_owned(), position);
            position
        };
        let Some(test) = self.tests.get_mut(position) else {
            return;
        };
        let merge = !test.covered.is_empty();
        test.covered.extend_from_slice(covered);
        if merge {
            test.covered.sort_unstable();
            test.covered.dedup();
        }
    }

    pub(crate) fn tests(&self) -> &[AttributedTest] {
        return &self.tests;
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
    pub(crate) settings: Option<Settings>,
    pub(crate) pattern: Option<Pattern>,
    pub(crate) decisions: BTreeMap<String, Verdict>,
    pub(crate) directories: Directories,
    pub(crate) problem: Option<OffReason>,
}

#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) probes_installed: u64,
    pub(crate) probes_fired: u64,
    pub(crate) probes_rearmed: u64,
    pub(crate) watches_installed: u64,
    pub(crate) pending_discards: u64,
    pub(crate) jump_table_files: u64,
}

impl Counters {
    pub(crate) fn discarded(&mut self, count: usize) {
        self.pending_discards = self
            .pending_discards
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
    }
}

#[derive(Debug, Default)]
pub(crate) struct Scratch {
    pub(crate) op_arrays: Vec<*mut zend_op_array>,
    pub(crate) orders: Vec<u128>,
    pub(crate) covered: Vec<Covered>,
    pub(crate) files: Vec<FileId>,
}

#[derive(Debug)]
pub(crate) struct Engine {
    pub(crate) slot: usize,
    pub(crate) label: Option<Handler>,
    pub(crate) request: Option<Request>,
    pub(crate) units: Units,
    pub(crate) files: Files,
    pub(crate) attribution: Attribution,
    pub(crate) fired: Fired,
    pub(crate) pending: Vec<Pending>,
    pub(crate) activations: Activations,
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
            attribution: Attribution::default(),
            fired: Fired::default(),
            pending: Vec::new(),
            activations: Activations::default(),
            collecting: false,
            completed_cycles: 0,
            counters: Counters::default(),
            scratch: Scratch::default(),
        };
    }

    pub(crate) const fn list(&mut self, list: FiredList) -> &mut Vec<FiredRef> {
        return match list {
            FiredList::Hits => &mut self.fired.hits,
            FiredList::Strays => &mut self.fired.strays,
            FiredList::Stopped => &mut self.fired.stopped,
        };
    }

    pub(crate) fn record(&mut self, fired: FiredRef) {
        self.counters.probes_fired = self.counters.probes_fired.saturating_add(1);
        let Some(unit) = self.units.get_mut(fired.unit) else {
            return;
        };
        unit.pending = unit.pending.saturating_add(1);
        if !self.collecting {
            self.fired.strays.push(fired);
            return;
        }
        let covered = match fired.hit {
            Hit::Line(probe) => usize::try_from(probe)
                .ok()
                .and_then(|probe| return unit.probes.get(probe))
                .and_then(|probe| return probe.line)
                .map(|line| return Covered::Line(unit.file, line)),
            Hit::Outcome(number) => Some(Covered::Outcome(unit.function, number)),
            Hit::Zero(number) => Some(Covered::Zero(unit.function, number)),
            Hit::Second(number) => Some(Covered::Second(unit.function, number)),
        };
        self.fired.covered.extend(covered);
        self.fired.hits.push(fired);
    }

    pub(crate) fn rearm(&mut self, list: FiredList) {
        if list == FiredList::Hits {
            self.fired.covered.clear();
        }
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
                // SAFETY: the unit is alive, so its opcodes are; the hit names the unit's own probes.
                unsafe { unit.rearm(fired.hit, label) };
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
        self.drop_pending();
        self.activations.clear();
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
        self.pending.clear();
        self.activations.clear();
        for unit in self.units.live_mut() {
            let Some(opcodes) = unit.opcodes else {
                continue;
            };
            unit.restore_producers();
            for (probe, &op) in unit.probes.iter_mut().zip(&unit.ops) {
                probe.watchers = 0;
                if probe.armed.is_empty() {
                    continue;
                }
                probe.armed = Roles::default();
                // SAFETY: the unit is alive, so its opcodes are; `op` is below `last`.
                unsafe { probe.handler.install(opcodes.wrapping_add(slot_of(op))) };
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
    let entered = if ZTS {
        !ENGINE.busy.swap(true, Ordering::Acquire)
    } else {
        let free = !ENGINE.busy.load(Ordering::Relaxed);
        ENGINE.busy.store(true, Ordering::Relaxed);
        free
    };
    assert!(entered, "opcov state entered re-entrantly");
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

pub(crate) fn idle() -> bool {
    return !threads::foreign() && !ENGINE.busy.load(Ordering::Acquire);
}

pub(crate) fn install(engine: Engine) {
    enter();
    // SAFETY: as in `with_engine`; called once from module startup before any request.
    unsafe { *ENGINE.engine.get() = Some(engine) };
    ENGINE.busy.store(false, Ordering::Release);
}

pub(crate) fn uninstall() {
    enter();
    // SAFETY: as in `with_engine`; called once from module shutdown, after the probes are unregistered.
    let engine = unsafe { (*ENGINE.engine.get()).take() };
    ENGINE.busy.store(false, Ordering::Release);
    drop(engine);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(content: &[u8]) -> Content {
        return Content::File(Fingerprint::of(content));
    }

    #[test]
    fn a_file_compiled_again_from_the_same_bytes_keeps_its_content() {
        let first = Source::next(None, Ok(file(b"<?php echo 1;")));
        assert_eq!(
            Source::next(Some(first.clone()), Ok(file(b"<?php echo 1;"))),
            first
        );
    }

    #[test]
    fn a_file_compiled_again_from_other_bytes_is_changed_for_good() {
        let first = Source::next(None, Ok(file(b"<?php echo 1;")));
        let changed = Source::next(Some(first), Ok(file(b"<?php echo 2;")));
        assert_eq!(changed, Source::Changed);
        assert_eq!(
            Source::next(Some(changed), Ok(file(b"<?php echo 1;"))),
            Source::Changed
        );
    }

    #[test]
    fn an_unreadable_compilation_is_kept_as_the_reason() {
        let unreadable = Source::next(None, Err(String::from("gone")));
        assert_eq!(unreadable, Source::Unreadable(String::from("gone")));
        assert_eq!(
            Source::next(Some(unreadable.clone()), Ok(file(b"<?php"))),
            unreadable
        );
    }

    #[test]
    fn fingerprints_tell_apart_contents_that_differ_in_one_byte_or_in_length() {
        let base = vec![b'a'; 4096];
        let mut flipped = base.clone();
        if let Some(byte) = flipped.get_mut(2048) {
            *byte = b'b';
        }
        assert_ne!(Fingerprint::of(&base), Fingerprint::of(&flipped));
        assert_ne!(Fingerprint::of(&base), Fingerprint::of(&base[..4095]));
        assert_eq!(Fingerprint::of(&base), Fingerprint::of(&base.clone()));
    }

    #[test]
    fn covered_items_sort_by_their_order_as_they_compare_and_round_trip() {
        let line = |number| return Line::new(number).unwrap();
        let mut items = vec![
            Covered::Second(FunctionId(0), 0),
            Covered::Line(FileId(u32::MAX), line(u32::MAX)),
            Covered::Outcome(FunctionId(2), 1),
            Covered::Line(FileId(1), line(9)),
            Covered::Zero(FunctionId(u32::MAX), u32::MAX),
            Covered::Outcome(FunctionId(2), 0),
            Covered::Line(FileId(1), line(1)),
        ];
        let mut by_order = items.clone();
        by_order.sort_unstable_by_key(|item| return item.order());
        items.sort_unstable();
        assert_eq!(by_order, items);
        for item in items {
            assert_eq!(Covered::from_order(item.order()), Some(item));
        }
    }

    #[test]
    fn a_probe_takes_24_bytes() {
        assert_eq!(size_of::<Probe>(), 24);
    }
}
