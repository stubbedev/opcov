use core::{
    ffi::{c_char, c_int},
    ptr,
};
use std::sync::OnceLock;

use opcov_core::cfg::{FunctionFlag, Opcode};
use opcov_sys::{
    IS_CONST, ZEND_ACC_DONE_PASS_TWO, ZEND_COMPILE_NO_JUMPTABLES, ZEND_PROPERTY_HOOK_COUNT,
    execute_ex, opcov_is_user_class, opcov_is_user_code, opcov_string_release, zend_class_entry,
    zend_compile_file, zend_compile_position, zend_compile_string, zend_execute_data,
    zend_execute_ex, zend_file_handle, zend_function, zend_op_array, zend_property_info,
    zend_resolve_path, zend_string,
};

use crate::{
    engine::{Engine, FileId, OffReason, Probe, Reserved, Role, Roles, Unit, Verdict, with_engine},
    guard::{COLLECTION, EntryPoint, guard},
    lines, probes,
    select::decide,
    snapshot::{self, PHP, Snapshot},
    threads, zend,
};

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
    // SAFETY: calls the hook that was installed before opcov's with the engine's own arguments.
    let op_array = unsafe { (originals.file)(handle, kind) };
    // SAFETY: as above.
    unsafe { (*globals).compiler_options = options };
    guard(EntryPoint::CompileFile, (), || {
        // SAFETY: the op array (or null) was just compiled; the tables are live.
        unsafe { after_compile(op_array, first_function, first_class) };
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
    // SAFETY: calls the hook that was installed before opcov's with the engine's own arguments.
    let op_array = unsafe { (originals.string)(source, filename, position) };
    // SAFETY: as above.
    unsafe { (*globals).compiler_options = options };
    guard(EntryPoint::CompileString, (), || {
        // SAFETY: the op array (or null) was just compiled; the tables are live.
        unsafe { after_compile(op_array, first_function, first_class) };
    });
    return op_array;
}

unsafe fn gather(list: &mut Vec<*mut zend_op_array>, op_array: *mut zend_op_array, slot: usize) {
    // SAFETY: the caller passes a live op array.
    let array = unsafe { &*op_array };
    let compiled = !array.opcodes.is_null() && array.fn_flags & ZEND_ACC_DONE_PASS_TWO != 0;
    // SAFETY: an op array is layout-compatible with the zend_function union it is part of.
    let user = unsafe { opcov_is_user_code(op_array.cast::<zend_function>()) };
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
    if !function.is_null() && unsafe { opcov_is_user_code(function) } {
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
        return threads::with_handler_table(|| {
            probes::unregister();
            for &op_array in pending {
                // SAFETY: gathered op arrays are live.
                let opcodes = unsafe { (*op_array).opcodes };
                if self.units.find(opcodes).is_none() {
                    // SAFETY: as above; the user handlers are unregistered right now.
                    unsafe { probes::reset_handlers(op_array) };
                }
            }
            probes::register();
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
        let function = snapshot.analysis.function.key().clone();
        let abstract_function = snapshot
            .analysis
            .function
            .flags()
            .contains(FunctionFlag::Abstract);
        if let Some(record) = self.files.get_mut(file) {
            record
                .functions
                .entry(function.clone())
                .or_insert(snapshot.analysis);
        }
        // SAFETY: the caller passes a live op array.
        let (opcodes, last) = unsafe { ((*op_array).opcodes, (*op_array).last) };
        let Ok(last) = usize::try_from(last) else {
            return;
        };
        if abstract_function || COLLECTION.disabled().is_some() {
            // SAFETY: as above.
            unsafe { Reserved::NoProbes.write(op_array, self.slot) };
            return;
        }
        let placement = lines::place(PHP, &snapshot.instrs, &snapshot.blocks);
        if let Some(record) = self.files.get_mut(file) {
            record.lines.extend(placement.lines.iter().copied());
        }
        let mut index = vec![0_u32; last].into_boxed_slice();
        let mut installed = Vec::with_capacity(placement.probes.len());
        for site in placement.probes.iter().filter(|site| return site.op < last) {
            let (Some(entry), Ok(number)) = (
                index.get_mut(site.op),
                u32::try_from(installed.len().saturating_add(1)),
            ) else {
                continue;
            };
            *entry = number;
            let op = opcodes.wrapping_add(site.op);
            // SAFETY: `site.op` is below `last`; the handler is the real one set by the reset.
            let handler = unsafe { Handler::of(op) };
            installed.push(Probe {
                op: site.op,
                line: site.line,
                handler,
                armed: Roles::only(Role::Line),
            });
            // SAFETY: as above; the probe label sends the opcode to the probe callback.
            unsafe { label.install(op) };
        }
        self.counters.probes_installed = self
            .counters
            .probes_installed
            .saturating_add(u64::try_from(installed.len()).unwrap_or(u64::MAX));
        let id = self.units.insert(Unit {
            opcodes: Some(opcodes),
            last,
            file,
            function,
            index,
            probes: installed,
            pending: 0,
        });
        // SAFETY: as above.
        unsafe { Reserved::Unit(id).write(op_array, self.slot) };
    }
}

use crate::engine::Handler;

unsafe fn process(op_array: *mut zend_op_array) {
    let Some(slot) = with_engine(|engine| return engine.slot) else {
        return;
    };
    // SAFETY: the caller passes a live op array.
    if unsafe { Reserved::read(op_array, slot) } != Reserved::Unknown {
        return;
    }
    // SAFETY: as above.
    let name = unsafe { zend::bytes((*op_array).filename) }.unwrap_or_default();
    let verdict = decide(name);
    let Verdict::Measure(file) = verdict else {
        // SAFETY: as above.
        unsafe { Reserved::NoProbes.write(op_array, slot) };
        return;
    };
    // SAFETY: as above.
    let opcodes = unsafe { (*op_array).opcodes };
    let shared = with_engine(|engine| return engine.units.find(opcodes)).flatten();
    if let Some(unit) = shared {
        // SAFETY: as above.
        unsafe { Reserved::Unit(unit).write(op_array, slot) };
        return;
    }
    // SAFETY: as above.
    let taken = unsafe { snapshot::take(op_array) };
    with_engine(|engine| match taken {
        // SAFETY: as above.
        Ok(snapshot) => unsafe { engine.install_unit(op_array, file, snapshot) },
        Err(failure) => {
            let path = engine
                .files
                .get(file)
                .map_or_else(String::new, |record| return record.path.clone());
            engine.set_problem(OffReason::Snapshot {
                file: path,
                message: failure.to_string(),
            });
            // SAFETY: as above.
            unsafe { Reserved::NoProbes.write(op_array, slot) };
        }
    });
}

unsafe fn instrument(roots: impl FnOnce(&mut Vec<*mut zend_op_array>, usize)) -> bool {
    let started = with_engine(|engine| {
        engine.label?;
        let start = engine.scratch.op_arrays.len();
        let slot = engine.slot;
        roots(&mut engine.scratch.op_arrays, slot);
        // SAFETY: the gathered op arrays are live.
        if unsafe { engine.reset_window(start) } {
            return Some((start, engine.scratch.op_arrays.len()));
        }
        engine.scratch.op_arrays.truncate(start);
        return None;
    })
    .flatten();
    let Some((start, end)) = started else {
        return false;
    };
    for position in start..end {
        let next =
            with_engine(|engine| return engine.scratch.op_arrays.get(position).copied()).flatten();
        if let Some(op_array) = next {
            // SAFETY: gathered op arrays are live.
            unsafe { process(op_array) };
        }
    }
    with_engine(|engine| engine.scratch.op_arrays.truncate(start));
    return true;
}

unsafe fn after_compile(main: *mut zend_op_array, first_function: u32, first_class: u32) {
    if main.is_null() || threads::foreign() {
        return;
    }
    // SAFETY: the compiler's tables are live; the new entries start at the given positions.
    let instrumented = unsafe {
        instrument(|list, slot| {
            gather(list, main, slot);
            for function in zend::new_functions(first_function) {
                gather_function(list, function, slot);
            }
            for class in zend::new_classes(first_class) {
                gather_class(list, class, slot);
            }
        })
    };
    if instrumented {
        // SAFETY: `main` was just compiled and is live.
        unsafe { record_skipped_return(main) };
    }
}

pub(crate) unsafe fn adopt(op_array: *mut zend_op_array) -> bool {
    // SAFETY: the caller passes the running frame's live op array.
    return unsafe { instrument(|list, slot| gather(list, op_array, slot)) };
}

unsafe fn record_skipped_return(main: *mut zend_op_array) {
    // SAFETY: the caller passes a live, compiled op array.
    let array = unsafe { &*main };
    if array.last != 1 || array.opcodes.is_null() {
        return;
    }
    // SAFETY: the op array has exactly one opcode.
    let only = unsafe { &*array.opcodes };
    let stock: unsafe extern "C" fn(*mut zend_execute_data) = execute_ex;
    // SAFETY: reads the engine's executor hook by value.
    let stock_executor =
        unsafe { zend_execute_ex }.is_some_and(|current| return ptr::fn_addr_eq(current, stock));
    if Opcode::decode(PHP, only.opcode) != Opcode::Return
        || only.op1_type != IS_CONST
        || !stock_executor
    {
        return;
    }
    // SAFETY: the executor globals are live during a request.
    let caller = unsafe { (*zend::executor_globals()).current_execute_data };
    if caller.is_null() {
        return;
    }
    // SAFETY: the current frame is live while the include compiles.
    let (function, opline) = unsafe { ((*caller).func, (*caller).opline) };
    // SAFETY: a non-null frame function is live.
    if function.is_null() || opline.is_null() || !unsafe { opcov_is_user_code(function) } {
        return;
    }
    // SAFETY: the frame's current opcode is live.
    if Opcode::decode(PHP, unsafe { (*opline).opcode }) != Opcode::IncludeOrEval {
        return;
    }
    with_engine(|engine| {
        // SAFETY: as above.
        let Reserved::Unit(id) = (unsafe { Reserved::read(main, engine.slot) }) else {
            return;
        };
        let armed = engine.units.get(id).and_then(|unit| {
            let probe = unit.probe_at(0)?;
            return unit
                .probes
                .get(probe)
                .filter(|slot| return slot.armed.contains(Role::Line))
                .map(|_| return probe);
        });
        if let Some(probe) = armed {
            engine.fire(id, probe);
        }
    });
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
            engine.units.destroyed(id);
        });
    });
}
