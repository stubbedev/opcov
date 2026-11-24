use core::{
    cell::OnceCell,
    ffi::{c_char, c_int},
    ptr, slice,
};
use std::{fs, ops::Range, sync::OnceLock};

use opcov_core::{
    cfg::{FilePath, FunctionFlag, FunctionKey, Line, LineSpan, Plan},
    report::Sha256,
};
use opcov_sys::{
    ZEND_ACC_DONE_PASS_TWO, ZEND_COMPILE_NO_JUMPTABLES, ZEND_PROPERTY_HOOK_COUNT,
    opcov_compile_file, opcov_compile_string, opcov_is_user_class, opcov_string_release,
    zend_class_entry, zend_compile_file, zend_compile_position, zend_compile_string,
    zend_file_handle, zend_function, zend_object, zend_op_array, zend_property_info,
    zend_resolve_path, zend_string, zend_throw_exception_hook,
};

use crate::{
    branches::{self, Layout},
    engine::{
        Content, Engine, FileId, FileRecord, Fingerprint, FunctionRecord, Reserved, Unit, Verdict,
        with_engine,
    },
    guard::{self, COLLECTION, EntryPoint, guard},
    lines,
    ordinals::{Group, Ordinals},
    probes::{self, Opcodes},
    select::decide,
    snapshot::{self, Named, Names, PHP, Snapshot, SnapshotFailure},
    threads, zend,
};

#[cfg(feature = "engine-selftest")]
pub(crate) static FAIL_NEXT_SNAPSHOT: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

type CompileFile = unsafe extern "C" fn(*mut zend_file_handle, c_int) -> *mut zend_op_array;
type CompileString = unsafe extern "C" fn(
    *mut zend_string,
    *const c_char,
    zend_compile_position,
) -> *mut zend_op_array;

#[derive(Debug, Clone, Copy)]
struct Originals {
    file: CompileFile,
    string: CompileString,
}

static ORIGINALS: OnceLock<Originals> = OnceLock::new();

type ThrowHook = unsafe extern "C" fn(*mut zend_object);

static PREVIOUS_THROW_HOOK: OnceLock<Option<ThrowHook>> = OnceLock::new();

unsafe extern "C" fn throw_hook(exception: *mut zend_object) {
    guard(EntryPoint::ExceptionHook, (), branches::on_exception);
    if let Some(Some(previous)) = PREVIOUS_THROW_HOOK.get() {
        // SAFETY: the hook installed before opcov's, called with the engine's own argument.
        unsafe { previous(exception) };
    }
}

pub(crate) fn hook() -> bool {
    // SAFETY: reads and replaces the engine's compile hooks at the first request, on the owner
    // thread, before any file of that request is compiled.
    unsafe {
        let (Some(file), Some(string)) = (zend_compile_file, zend_compile_string) else {
            return false;
        };
        if ORIGINALS.set(Originals { file, string }).is_err() {
            return true;
        }
        zend_compile_file = Some(compile_file);
        zend_compile_string = Some(compile_string);
        if PREVIOUS_THROW_HOOK.set(zend_throw_exception_hook).is_ok() {
            zend_throw_exception_hook = Some(throw_hook);
        }
    }
    return true;
}

fn measuring() -> bool {
    return with_engine(|engine| return engine.measuring()).unwrap_or(false);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpTables {
    Unchanged,
    Keep,
    Remove,
}

impl JumpTables {
    const fn apply(self, options: u32) -> u32 {
        return match self {
            Self::Unchanged => options,
            Self::Keep => options & !ZEND_COMPILE_NO_JUMPTABLES,
            Self::Remove => options | ZEND_COMPILE_NO_JUMPTABLES,
        };
    }

    const fn of(verdict: &Verdict) -> Self {
        return match verdict {
            Verdict::Skip(opcov_core::filter::SkipReason::Unresolvable)
            | Verdict::Measure(_)
            | Verdict::MeasureEval { .. }
            | Verdict::Off => Self::Remove,
            Verdict::Skip(_) => Self::Keep,
        };
    }
}

unsafe fn resolved_name(handle: *const zend_file_handle) -> JumpTables {
    // SAFETY: the engine passes a live file handle.
    let (opened, filename) = unsafe { ((*handle).opened_path, (*handle).filename) };
    // SAFETY: an opened path is null or a live zend_string.
    if let Some(name) = unsafe { zend::bytes(opened) } {
        return JumpTables::of(&decide(name));
    }
    // SAFETY: the handle's file name is null or a live zend_string.
    if let Some(name) = unsafe { zend::bytes(filename) }
        && name.starts_with(b"/")
    {
        return JumpTables::of(&decide(name));
    }
    // SAFETY: reads the engine's resolver hook by value.
    let (Some(resolve), false) = (unsafe { zend_resolve_path }, filename.is_null()) else {
        return JumpTables::Remove;
    };
    // SAFETY: `filename` is the handle's live file name; the resolver returns a new string or null.
    let resolved = unsafe { resolve(filename) };
    // SAFETY: null or a live zend_string returned above.
    let Some(name) = (unsafe { zend::bytes(resolved) }) else {
        return JumpTables::Remove;
    };
    let decision = JumpTables::of(&decide(name));
    // SAFETY: releases the string the resolver returned.
    unsafe { opcov_string_release(resolved) };
    return decision;
}

unsafe fn jump_tables(handle: *const zend_file_handle) -> JumpTables {
    if !measuring() {
        return JumpTables::Unchanged;
    }
    // SAFETY: forwarded from the caller.
    let decision = unsafe { resolved_name(handle) };
    if decision == JumpTables::Keep {
        with_engine(|engine| {
            engine.counters.jump_table_files = engine.counters.jump_table_files.saturating_add(1);
        });
    }
    return decision;
}

unsafe extern "C" fn compile_file(
    handle: *mut zend_file_handle,
    kind: c_int,
) -> *mut zend_op_array {
    let Some(originals) = ORIGINALS.get().copied() else {
        return ptr::null_mut();
    };
    let globals = zend::compiler_globals();
    // SAFETY: the compiler globals are live during compilation.
    let options = unsafe { (*globals).compiler_options };
    // SAFETY: the engine passes a live file handle.
    let tables = guard(EntryPoint::CompileFile, JumpTables::Remove, || unsafe {
        return jump_tables(handle);
    });
    // SAFETY: as above; the original options are put back right after compilation.
    let (first_function, first_class) = unsafe {
        (*globals).compiler_options = tables.apply(options);
        (zend::function_table_used(), zend::class_table_used())
    };
    let mut op_array: *mut zend_op_array = ptr::null_mut();
    // SAFETY: calls the hook that was installed before opcov's with the engine's own arguments; a
    // fatal error during compilation is caught inside the call.
    let completed =
        unsafe { opcov_compile_file(Some(originals.file), handle, kind, &raw mut op_array) };
    // SAFETY: as above.
    unsafe { (*globals).compiler_options = options };
    if !completed {
        guard::resume_bailout();
    }
    guard(EntryPoint::CompileFile, (), || {
        // SAFETY: the op array (or null) was just compiled from the live handle; the tables are
        // live.
        unsafe {
            after_compile(
                op_array,
                Compiled::File(handle),
                first_function,
                first_class,
            );
        }
    });
    return op_array;
}

unsafe extern "C" fn compile_string(
    source: *mut zend_string,
    filename: *const c_char,
    position: zend_compile_position,
) -> *mut zend_op_array {
    let Some(originals) = ORIGINALS.get().copied() else {
        return ptr::null_mut();
    };
    let globals = zend::compiler_globals();
    let tables = guard(EntryPoint::CompileString, JumpTables::Remove, || {
        return if measuring() {
            JumpTables::Remove
        } else {
            JumpTables::Unchanged
        };
    });
    // SAFETY: the compiler globals are live during compilation; the options are put back below.
    let (options, first_function, first_class) = unsafe {
        let options = (*globals).compiler_options;
        (*globals).compiler_options = tables.apply(options);
        (
            options,
            zend::function_table_used(),
            zend::class_table_used(),
        )
    };
    let mut op_array: *mut zend_op_array = ptr::null_mut();
    // SAFETY: calls the hook that was installed before opcov's with the engine's own arguments; a
    // fatal error during compilation is caught inside the call.
    let completed = unsafe {
        opcov_compile_string(
            Some(originals.string),
            source,
            filename,
            position,
            &raw mut op_array,
        )
    };
    // SAFETY: as above.
    unsafe { (*globals).compiler_options = options };
    if !completed {
        guard::resume_bailout();
    }
    guard(EntryPoint::CompileString, (), || {
        // SAFETY: the op array (or null) was just compiled from the live source; the tables are
        // live.
        unsafe {
            after_compile(
                op_array,
                Compiled::String(source),
                first_function,
                first_class,
            );
        }
    });
    return op_array;
}

unsafe fn gather(list: &mut Vec<*mut zend_op_array>, op_array: *mut zend_op_array, slot: usize) {
    // SAFETY: the caller passes a live op array.
    let array = unsafe { &*op_array };
    let compiled = !array.opcodes.is_null() && array.fn_flags & ZEND_ACC_DONE_PASS_TWO != 0;
    // SAFETY: an op array is layout-compatible with the zend_function union it is part of.
    let user = unsafe { zend::is_user_code(op_array.cast::<zend_function>()) };
    // SAFETY: forwarded from the caller.
    if !user || !compiled || unsafe { Reserved::read(op_array, slot) } != Reserved::Unknown {
        return;
    }
    list.push(op_array);
    let Ok(count) = usize::try_from(array.num_dynamic_func_defs) else {
        return;
    };
    for index in 0..count {
        // SAFETY: `dynamic_func_defs` holds `num_dynamic_func_defs` live op arrays.
        let nested = unsafe { *array.dynamic_func_defs.wrapping_add(index) };
        if !nested.is_null() {
            // SAFETY: as above.
            unsafe { gather(list, nested, slot) };
        }
    }
}

unsafe fn gather_function(
    list: &mut Vec<*mut zend_op_array>,
    function: *mut zend_function,
    slot: usize,
) {
    // SAFETY: the caller passes a live function.
    if !function.is_null() && unsafe { zend::is_user_code(function) } {
        // SAFETY: user code is an op array.
        unsafe { gather(list, &raw mut (*function).op_array, slot) };
    }
}

unsafe fn gather_class(
    list: &mut Vec<*mut zend_op_array>,
    class: *mut zend_class_entry,
    slot: usize,
) {
    // SAFETY: the caller passes a live class entry.
    if !unsafe { opcov_is_user_class(class) } {
        return;
    }
    // SAFETY: as above.
    let entry = unsafe { &*class };
    // SAFETY: a class's method table is a live map of zend_function pointers.
    for function in unsafe { zend::map_values::<zend_function>(&raw const entry.function_table, 0) }
    {
        // SAFETY: as above.
        unsafe { gather_function(list, function, slot) };
    }
    if entry.num_hooked_props == 0 {
        return;
    }
    // SAFETY: a class's property table is a live map of zend_property_info pointers.
    let properties =
        unsafe { zend::map_values::<zend_property_info>(&raw const entry.properties_info, 0) };
    for property in properties {
        // SAFETY: each value is a live property info.
        let hooks = unsafe { (*property).hooks };
        if hooks.is_null() {
            continue;
        }
        for index in 0..ZEND_PROPERTY_HOOK_COUNT {
            let Ok(offset) = usize::try_from(index) else {
                continue;
            };
            // SAFETY: a non-null hook array holds ZEND_PROPERTY_HOOK_COUNT entries.
            let hook = unsafe { *hooks.wrapping_add(offset) };
            // SAFETY: a hook is null or a live function.
            unsafe { gather_function(list, hook, slot) };
        }
    }
}

impl Engine {
    unsafe fn reset_window(&self, start: usize) -> bool {
        let pending = self.scratch.op_arrays.get(start..).unwrap_or_default();
        let unshared = || {
            return pending.iter().copied().filter(|&op_array| {
                // SAFETY: gathered op arrays are live.
                return self.units.find(unsafe { (*op_array).opcodes }).is_none();
            });
        };
        let mut present = Opcodes::default();
        for op_array in unshared() {
            // SAFETY: as above.
            unsafe { probes::opcodes_of(op_array, &mut present) };
        }
        return probes::with_real_handlers(present, || {
            for op_array in unshared() {
                // SAFETY: as above; opcov's user handlers are removed for every opcode they hold.
                unsafe { probes::reset_handlers(op_array) };
            }
        })
        .is_some();
    }

    unsafe fn install_unit(
        &mut self,
        op_array: *mut zend_op_array,
        file: FileId,
        snapshot: Snapshot,
    ) {
        let Some(label) = self.label else {
            return;
        };
        // SAFETY: the caller passes a live op array.
        let (opcodes, last) = unsafe { ((*op_array).opcodes, (*op_array).last) };
        let Ok(last) = usize::try_from(last) else {
            return;
        };
        let Snapshot {
            analysis,
            #[cfg(feature = "engine-selftest")]
            input,
            instrs,
            blocks,
        } = snapshot;
        let cfg = &analysis.function;
        let key = cfg.key().clone();
        let abstract_function = cfg.flags().contains(FunctionFlag::Abstract);
        let plan = Plan::of(cfg);
        let zero_possible = branches::zero_possible(&plan, cfg, opcodes);
        let declarations_only =
            matches!(key, FunctionKey::Main { .. }) && lines::declarations_only(&instrs);
        let placement = if declarations_only {
            lines::Placement::default()
        } else {
            lines::place(PHP, &instrs, &blocks, plan.fallbacks())
        };
        // SAFETY: the op array is live; the layout only reads its opcodes and try/catch table.
        let built = unsafe {
            Layout {
                op_array,
                opcodes,
                last,
                function: cfg,
                plan: &plan,
                zero_possible: &zero_possible,
                lines: &placement.probes,
                instrs: &instrs,
            }
            .build()
        };
        let outcomes = plan.outcomes().len();
        let function = match self.files.find_function(file, &key) {
            Some(function) => Some(function),
            None => self.files.add_function(FunctionRecord {
                file,
                key,
                analysis,
                #[cfg(feature = "engine-selftest")]
                input,
                plan,
                zero_possible,
                name: OnceCell::new(),
            }),
        };
        let Some(function) = function else {
            return;
        };
        if abstract_function || COLLECTION.disabled().is_some() {
            // SAFETY: as above.
            unsafe { Reserved::NoProbes.write(op_array, self.slot) };
            return;
        }
        if let Some(record) = self.files.get_mut(file) {
            record.lines.extend(placement.lines.iter().copied());
        }
        let mut unit = Unit {
            opcodes: Some(opcodes),
            last,
            file,
            function,
            ops: built.ops,
            cursor: 0,
            probes: built.probes,
            sources: built.sources,
            owners: built.owners,
            twins: built.twins,
            entries: built.entries,
            finals: built.finals,
            seen: vec![false; outcomes].into_boxed_slice(),
            loops: built.loops,
            pending: 0,
        };
        let producers = unit.producer_ops();
        if !producers.is_empty() {
            // SAFETY: the op array is live and its producers lie below `last`.
            match unsafe { probes::plain_handlers(opcodes, &producers) } {
                // SAFETY: as above.
                Some(handlers) => unsafe { unit.prepare_producers(&handlers) },
                None => unit.drop_producers(),
            }
        }
        // SAFETY: the op array is live; the unit's probes lie below `last`.
        unsafe { unit.arm_everything(label) };
        self.counters.probes_installed = self
            .counters
            .probes_installed
            .saturating_add(u64::try_from(unit.probes.len()).unwrap_or(u64::MAX));
        let reserved = self
            .units
            .insert(unit)
            .map_or(Reserved::NoProbes, Reserved::Unit);
        // SAFETY: as above.
        unsafe { reserved.write(op_array, self.slot) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Compiled,
    Adopted,
}

impl Engine {
    fn closures_taken(&self, origin: Origin, group: &Group) -> u32 {
        if origin == Origin::Compiled {
            return 0;
        }
        let (file, start, end) = group;
        let Ok(file) = FilePath::from_bytes(file) else {
            return 0;
        };
        let Some(record) = self
            .files
            .find(file.as_str())
            .and_then(|id| return self.files.get(id))
        else {
            return 0;
        };
        let Some(lines) = Line::new(*start)
            .zip(Line::new(*end))
            .and_then(|(start, end)| return LineSpan::new(start, end))
        else {
            return 0;
        };
        let key = |ordinal| {
            return FunctionKey::Closure {
                file: file.clone(),
                lines,
                ordinal,
            };
        };
        return record
            .functions
            .range(key(0)..=key(u32::MAX))
            .next_back()
            .map_or(0, |(last, _)| {
                return match last {
                    FunctionKey::Closure { ordinal, .. } => ordinal.saturating_add(1),
                    _ => 0,
                };
            });
    }
}

#[derive(Debug, Clone)]
struct EvalSource {
    compiled: FilePath,
    file: FileId,
}

struct Window {
    range: Range<usize>,
    origin: Origin,
    eval: Option<EvalSource>,
    ordinals: Option<Ordinals>,
    names: Names,
    verdict: Option<(usize, Verdict)>,
}

impl Window {
    fn new(range: Range<usize>, origin: Origin, eval: Option<EvalSource>) -> Self {
        return Self {
            range,
            origin,
            eval,
            ordinals: None,
            names: Names::default(),
            verdict: None,
        };
    }

    fn file(&self, verdict: &Verdict) -> Option<FileId> {
        return match verdict {
            Verdict::Measure(file) => Some(*file),
            Verdict::MeasureEval { compiled } => self
                .eval
                .as_ref()
                .filter(|eval| return eval.compiled == *compiled)
                .map(|eval| return eval.file),
            Verdict::Skip(_) | Verdict::Off => None,
        };
    }

    unsafe fn name(&mut self, op_array: *const zend_op_array) -> Result<Named, SnapshotFailure> {
        let range = self.range.clone();
        let origin = self.origin;
        let ordinals = self.ordinals.get_or_insert_with(|| {
            return with_engine(|engine| {
                let window = engine.scratch.op_arrays.get(range).unwrap_or_default();
                let taken = |group: &Group| return engine.closures_taken(origin, group);
                // SAFETY: the window's op arrays were gathered by this compilation and are live.
                return unsafe { Ordinals::rank(window, taken) };
            })
            .unwrap_or_default();
        });
        // SAFETY: forwarded from the caller.
        return unsafe { snapshot::name(op_array, &mut self.names, ordinals) };
    }

    fn verdict(&mut self, name: *const zend_string) -> Verdict {
        if let Some((cached, verdict)) = &self.verdict
            && *cached == name.addr()
        {
            return verdict.clone();
        }
        // SAFETY: the caller passes a compiled op array's file name, a live zend_string.
        let verdict = decide(unsafe { zend::bytes(name) }.unwrap_or_default());
        self.verdict = Some((name.addr(), verdict.clone()));
        return verdict;
    }
}

unsafe fn process(op_array: *mut zend_op_array, window: &mut Window) {
    let Some(slot) = with_engine(|engine| return engine.slot) else {
        return;
    };
    // SAFETY: the caller passes a live op array.
    if unsafe { Reserved::read(op_array, slot) } != Reserved::Unknown {
        return;
    }
    // SAFETY: as above.
    let verdict = window.verdict(unsafe { (*op_array).filename });
    // SAFETY: as above.
    let opcodes = unsafe { (*op_array).opcodes };
    let (measured, shared) = with_engine(|engine| {
        let shared = engine.units.find(opcodes);
        let measured = window
            .file(&verdict)
            .filter(|&file| return engine.files.get(file).is_some_and(FileRecord::modelled));
        return (measured, shared);
    })
    .unwrap_or_default();
    let eval_from_elsewhere = matches!(verdict, Verdict::MeasureEval { .. }) && measured.is_none();
    let file = match (measured, shared) {
        (Some(file), None) => file,
        (Some(_), Some(unit)) => {
            // SAFETY: as above.
            unsafe { Reserved::Unit(unit).write(op_array, slot) };
            return;
        }
        (None, Some(unit)) if eval_from_elsewhere => {
            // SAFETY: as above.
            unsafe { Reserved::Unit(unit).write(op_array, slot) };
            return;
        }
        (None, _) => {
            // SAFETY: as above.
            unsafe { Reserved::NoProbes.write(op_array, slot) };
            return;
        }
    };
    // SAFETY: as above.
    let taken = unsafe { window.name(op_array) }
        // SAFETY: as above.
        .and_then(|named| return unsafe { snapshot::take(op_array, named) });
    #[cfg(feature = "engine-selftest")]
    let taken = taken.and_then(|snapshot| {
        if FAIL_NEXT_SNAPSHOT.swap(false, core::sync::atomic::Ordering::Relaxed) {
            return Err(SnapshotFailure::Injected);
        }
        return Ok(snapshot);
    });
    with_engine(|engine| match taken {
        // SAFETY: as above.
        Ok(snapshot) => unsafe { engine.install_unit(op_array, file, snapshot) },
        Err(failure) => {
            if let Some(record) = engine.files.get_mut(file) {
                record.give_up(format!(
                    "cannot analyse code compiled from {}: {failure}",
                    record.path
                ));
            }
            // SAFETY: as above.
            unsafe { Reserved::NoProbes.write(op_array, slot) };
        }
    });
}

unsafe fn instrument(
    origin: Origin,
    eval: Option<EvalSource>,
    roots: impl FnOnce(&mut Vec<*mut zend_op_array>, usize),
) -> bool {
    let started = with_engine(|engine| {
        engine.label?;
        let start = engine.scratch.op_arrays.len();
        let slot = engine.slot;
        roots(&mut engine.scratch.op_arrays, slot);
        // SAFETY: the gathered op arrays are live.
        if unsafe { engine.reset_window(start) } {
            return Some(Window::new(
                start..engine.scratch.op_arrays.len(),
                origin,
                eval,
            ));
        }
        engine.scratch.op_arrays.truncate(start);
        return None;
    })
    .flatten();
    let Some(mut window) = started else {
        return false;
    };
    for position in window.range.clone() {
        let next =
            with_engine(|engine| return engine.scratch.op_arrays.get(position).copied()).flatten();
        if let Some(op_array) = next {
            // SAFETY: gathered op arrays are live.
            unsafe { process(op_array, &mut window) };
        }
    }
    with_engine(|engine| engine.scratch.op_arrays.truncate(window.range.start));
    return true;
}

#[derive(Debug, Clone, Copy)]
enum Compiled {
    File(*const zend_file_handle),
    String(*const zend_string),
}

unsafe fn compiled_content(compiled: Compiled, path: &FilePath) -> Result<Content, String> {
    let handle = match compiled {
        Compiled::String(source) => {
            // SAFETY: the caller passes the live source string of the compilation.
            return unsafe { zend::bytes(source) }
                .map(|text| return Content::Text(Sha256::digest(text)))
                .ok_or_else(|| return format!("the compiled source of {path} is missing"));
        }
        Compiled::File(handle) => handle,
    };
    // SAFETY: the caller passes the live handle of the compilation; after the scanner read the
    // file, `buf` holds its `len` bytes until the handle is destroyed.
    let (buf, len) = unsafe { ((*handle).buf, (*handle).len) };
    if buf.is_null() {
        return fs::read(path.to_path_buf())
            .map(|bytes| return Content::File(Fingerprint::of(&bytes)))
            .map_err(|error| return format!("cannot read {path}: {error}"));
    }
    // SAFETY: as above.
    let bytes = unsafe { slice::from_raw_parts(buf.cast::<u8>(), len) };
    return Ok(Content::File(Fingerprint::of(bytes)));
}

unsafe fn eval_source(compiled: Compiled, name: FilePath) -> Option<EvalSource> {
    let Compiled::String(source) = compiled else {
        return None;
    };
    // SAFETY: the caller passes the live source string of the compilation.
    let sha256 = Sha256::digest(unsafe { zend::bytes(source) }?);
    let file = with_engine(|engine| return engine.eval_file(&name, sha256)).flatten()?;
    return Some(EvalSource {
        compiled: name,
        file,
    });
}

unsafe fn record_source(main: *const zend_op_array, compiled: Compiled) -> Option<EvalSource> {
    // SAFETY: the caller passes a live, compiled op array.
    let name = unsafe { zend::bytes((*main).filename) }.unwrap_or_default();
    let file = match decide(name) {
        Verdict::Measure(file) => file,
        // SAFETY: forwarded from the caller.
        Verdict::MeasureEval { compiled: name } => return unsafe { eval_source(compiled, name) },
        Verdict::Skip(_) | Verdict::Off => return None,
    };
    let path = with_engine(|engine| {
        return engine
            .files
            .get(file)
            .map(|record| return record.path.clone());
    })
    .flatten()?;
    // SAFETY: forwarded from the caller.
    let content = unsafe { compiled_content(compiled, &path) };
    with_engine(|engine| {
        if let Some(record) = engine.files.get_mut(file) {
            record.compiled_from(content);
        }
    });
    return None;
}

unsafe fn after_compile(
    main: *mut zend_op_array,
    compiled: Compiled,
    first_function: u32,
    first_class: u32,
) {
    if main.is_null() || threads::foreign() {
        return;
    }
    // SAFETY: `main` was just compiled from `compiled`, which is still live.
    let eval = unsafe { record_source(main, compiled) };
    // SAFETY: the compiler's tables are live; the new entries start at the given positions.
    unsafe {
        instrument(Origin::Compiled, eval, |list, slot| {
            gather(list, main, slot);
            for function in zend::new_functions(first_function) {
                gather_function(list, function, slot);
            }
            for class in zend::new_classes(first_class) {
                gather_class(list, class, slot);
            }
        })
    };
}

pub(crate) unsafe fn adopt(op_array: *mut zend_op_array) -> bool {
    // SAFETY: the caller passes the running frame's live op array.
    return unsafe {
        instrument(Origin::Adopted, None, |list, slot| {
            gather(list, op_array, slot);
        })
    };
}

pub(crate) unsafe extern "C" fn op_array_dtor(op_array: *mut zend_op_array) {
    guard(EntryPoint::OpArrayDtor, (), || {
        if threads::foreign() {
            return;
        }
        with_engine(|engine| {
            // SAFETY: the engine passes the op array being destroyed; only its fields are read.
            let Reserved::Unit(id) = (unsafe { Reserved::read(op_array, engine.slot) }) else {
                return;
            };
            // SAFETY: as above.
            unsafe { Reserved::Unknown.write(op_array, engine.slot) };
            engine.forget_unit(id);
            engine.units.destroyed(id);
        });
    });
}
