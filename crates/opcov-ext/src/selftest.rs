use core::{
    ffi::{c_int, c_void},
    ptr, slice,
};

use opcov_sys::{
    ArgInfo, IS_UNUSED, MAY_BE_STRING, MAY_BE_VOID, SUCCESS, ZEND_BB_LOOP_HEADER, ZEND_NOP,
    ZEND_USER_OPCODE, ZEND_USER_OPCODE_DISPATCH, function_entry, opcov_arena_create,
    opcov_arena_destroy, opcov_compiler_globals, opcov_executor_globals, opcov_op_handler,
    zend_arena, zend_build_cfg, zend_cfg, zend_cfg_build_predecessors,
    zend_cfg_compute_dominators_tree, zend_cfg_identify_loops, zend_compile_file,
    zend_compile_string, zend_execute_data, zend_extension, zend_function_entry,
    zend_get_extension, zend_get_resource_handle, zend_get_user_opcode_handler, zend_op,
    zend_op_array, zend_register_extension, zend_set_user_opcode_handler,
    zend_vm_set_opcode_handler, zval,
};

use crate::{
    VERSION, accepts_no_arguments,
    guard::{EntryPoint, guard},
    info::vm_kind,
    return_string, zend,
};

const ARENA_SIZE: usize = 64 * 1024;
const SELFTEST_EXTENSION: &core::ffi::CStr = c"opcov engine selftest";

static ENGINE_SELFTEST_ARGINFO: ArgInfo<0> = ArgInfo::returning(MAY_BE_STRING);
static SELFTEST_PANIC_ARGINFO: ArgInfo<0> = ArgInfo::returning(MAY_BE_VOID);

pub(crate) const ENGINE_SELFTEST_FUNCTION: zend_function_entry = function_entry(
    c"opcov\\engine_selftest",
    engine_selftest,
    &ENGINE_SELFTEST_ARGINFO,
);

pub(crate) const SELFTEST_PANIC_IN_PROBE_FUNCTION: zend_function_entry = function_entry(
    c"opcov\\selftest_panic_in_probe",
    selftest_panic_in_probe,
    &SELFTEST_PANIC_ARGINFO,
);

pub(crate) const SELFTEST_FORCE_WATCH_FUNCTION: zend_function_entry = function_entry(
    c"opcov\\selftest_force_watch",
    selftest_force_watch,
    &SELFTEST_PANIC_ARGINFO,
);

pub(crate) const SELFTEST_FAIL_SNAPSHOT_FUNCTION: zend_function_entry = function_entry(
    c"opcov\\selftest_fail_snapshot",
    selftest_fail_snapshot,
    &SELFTEST_PANIC_ARGINFO,
);

pub(crate) const SELFTEST_PANIC_FUNCTION: zend_function_entry = function_entry(
    c"opcov\\selftest_panic",
    selftest_panic,
    &SELFTEST_PANIC_ARGINFO,
);

const unsafe extern "C" fn dispatch(_execute_data: *mut zend_execute_data) -> c_int {
    return ZEND_USER_OPCODE_DISPATCH;
}

fn unused_operands(opcode: u8) -> zend_op {
    return zend_op {
        opcode,
        op1_type: IS_UNUSED,
        op2_type: IS_UNUSED,
        result_type: IS_UNUSED,
        ..zend_op::default()
    };
}

fn handler_of(op: &mut zend_op) -> *const c_void {
    // SAFETY: `op` is a valid opcode with unused operands, for which the VM picks a handler from its
    // specialisation table without reading any operand.
    unsafe { zend_vm_set_opcode_handler(op) };
    // SAFETY: `op` is a valid, initialised zend_op.
    return unsafe { opcov_op_handler(op) };
}

fn probe_label() -> Result<&'static str, String> {
    // SAFETY: reads the process-wide user opcode handler table for a valid opcode.
    let previous = unsafe { zend_get_user_opcode_handler(ZEND_NOP) };
    // SAFETY: installs a valid handler for a valid opcode; it is replaced by `previous` below before
    // any code is compiled or executed.
    if unsafe { zend_set_user_opcode_handler(ZEND_NOP, Some(dispatch)) } != SUCCESS {
        return Err(String::from(
            "zend_set_user_opcode_handler(ZEND_NOP) failed",
        ));
    }
    let mut nop = unused_operands(ZEND_NOP);
    let label = handler_of(&mut nop);
    let user_opcode = handler_of(&mut unused_operands(ZEND_USER_OPCODE));
    // SAFETY: restores the handler that was registered before this check.
    if unsafe { zend_set_user_opcode_handler(ZEND_NOP, previous) } != SUCCESS {
        return Err(String::from(
            "restoring the ZEND_NOP user opcode handler failed",
        ));
    }
    let real = handler_of(&mut nop);
    if real.is_null() || label.is_null() {
        return Err(String::from(
            "zend_vm_set_opcode_handler produced a null handler",
        ));
    }
    if label != user_opcode {
        return Err(String::from(
            "a NOP with a user handler did not get the ZEND_USER_OPCODE handler",
        ));
    }
    if label == real {
        return Err(String::from(
            "the probe label equals the real NOP handler, so a probe could not be told apart",
        ));
    }
    return Ok("ok");
}

fn compile_hooks() -> Result<&'static str, String> {
    // SAFETY: reads the engine's compile hook pointers by value; nothing writes them concurrently.
    let (file, string) = unsafe { (zend_compile_file, zend_compile_string) };
    if file.is_none() || string.is_none() {
        return Err(String::from(
            "zend_compile_file or zend_compile_string is not set",
        ));
    }
    return Ok("ok");
}

unsafe fn globals(execute_data: *mut zend_execute_data) -> Result<&'static str, String> {
    // SAFETY: the accessors return the current thread's engine globals, valid during a request.
    let (executor, compiler) = unsafe { (&*opcov_executor_globals(), &*opcov_compiler_globals()) };
    if executor.current_execute_data != execute_data {
        return Err(String::from(
            "EG(current_execute_data) is not this call's frame",
        ));
    }
    if executor.function_table != compiler.function_table {
        return Err(String::from(
            "EG(function_table) differs from CG(function_table)",
        ));
    }
    return Ok("ok");
}

fn resource_handle() -> Result<&'static str, String> {
    // SAFETY: registers a reserved op_array slot under a NUL-terminated name.
    let handle = unsafe { zend_get_resource_handle(SELFTEST_EXTENSION.as_ptr()) };
    if handle < 0 {
        return Err(format!("zend_get_resource_handle returned {handle}"));
    }
    return Ok("ok");
}

fn extension_registration() -> Result<&'static str, String> {
    let mut extension = zend_extension {
        name: SELFTEST_EXTENSION.as_ptr(),
        version: VERSION.as_ptr(),
        ..zend_extension::default()
    };
    // SAFETY: the engine copies the struct; its strings are 'static and it has no hooks or handle.
    unsafe { zend_register_extension(&raw mut extension, ptr::null_mut()) };
    // SAFETY: looks the extension up by a NUL-terminated name.
    if unsafe { zend_get_extension(SELFTEST_EXTENSION.as_ptr()) }.is_null() {
        return Err(String::from(
            "zend_get_extension did not find the registered extension",
        ));
    }
    return Ok("ok");
}

struct Arena(*mut zend_arena);

impl Arena {
    fn new() -> Self {
        // SAFETY: allocates a fresh arena that this value owns.
        return Self(unsafe { opcov_arena_create(ARENA_SIZE) });
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: the arena (and every segment chained onto it) is owned by this value.
        unsafe { opcov_arena_destroy(self.0) };
    }
}

unsafe fn caller_op_array(
    execute_data: *mut zend_execute_data,
) -> Result<*const zend_op_array, String> {
    // SAFETY: the caller passes this call's frame, whose prev_execute_data is the calling frame.
    let caller = unsafe { (*execute_data).prev_execute_data };
    if caller.is_null() {
        return Err(String::from("no calling frame"));
    }
    // SAFETY: `caller` is a live frame.
    let function = unsafe { (*caller).func };
    if function.is_null() {
        return Err(String::from("the calling frame has no function"));
    }
    // SAFETY: `function` is the live function of a live frame.
    if !unsafe { zend::is_user_code(function) } {
        return Err(String::from("the caller is not user code"));
    }
    // SAFETY: user code is an op_array.
    return Ok(unsafe { &raw const (*function).op_array });
}

unsafe fn caller_cfg(execute_data: *mut zend_execute_data) -> Result<String, String> {
    // SAFETY: forwarded from the caller.
    let op_array = unsafe { caller_op_array(execute_data)? };
    let mut arena = Arena::new();
    let mut cfg = zend_cfg::default();
    // SAFETY: `op_array` is the live, compiled caller; the CFG is allocated in `arena`, which
    // outlives every use of `cfg` below.
    unsafe {
        zend_build_cfg(&raw mut arena.0, op_array, 0, &raw mut cfg);
        zend_cfg_build_predecessors(&raw mut arena.0, &raw mut cfg);
        zend_cfg_compute_dominators_tree(op_array, &raw mut cfg);
        zend_cfg_identify_loops(op_array, &raw mut cfg);
    }
    let count = usize::try_from(cfg.blocks_count)
        .map_err(|error| return format!("negative block count {}: {error}", cfg.blocks_count))?;
    // SAFETY: zend_build_cfg allocated `blocks_count` blocks in the arena, which is still alive.
    let blocks = unsafe { slice::from_raw_parts(cfg.blocks, count) };
    let loop_headers = blocks
        .iter()
        .filter(|block| return block.flags & ZEND_BB_LOOP_HEADER != 0)
        .count();
    drop(arena);
    return Ok(format!("{count} blocks, loop headers: {loop_headers}"));
}

fn line(name: &str, result: Result<impl AsRef<str>, String>) -> String {
    return match result {
        Ok(detail) => format!("{name}: {}", detail.as_ref()),
        Err(problem) => format!("{name}: FAILED: {problem}"),
    };
}

unsafe fn report(execute_data: *mut zend_execute_data) -> String {
    let lines = [
        format!("vm kind: {}", vm_kind().to_string_lossy()),
        line("probe label", probe_label()),
        line("compile hooks", compile_hooks()),
        // SAFETY: forwarded from the caller.
        line("engine globals", unsafe { globals(execute_data) }),
        line("resource handle", resource_handle()),
        line("extension registration", extension_registration()),
        // SAFETY: forwarded from the caller.
        line("caller cfg", unsafe { caller_cfg(execute_data) }),
    ];
    return lines.join("\n");
}

unsafe extern "C" fn engine_selftest(
    execute_data: *mut zend_execute_data,
    return_value: *mut zval,
) {
    guard(EntryPoint::EngineSelftest, (), || {
        // SAFETY: the engine calls this handler with the frame and return slot of this call.
        unsafe {
            if accepts_no_arguments(execute_data) {
                return_string(return_value, report(execute_data).as_bytes());
            }
        }
    });
}

#[expect(
    clippy::panic,
    reason = "this function exists to inject a panic at an engine entry point"
)]
unsafe extern "C" fn selftest_panic(
    execute_data: *mut zend_execute_data,
    _return_value: *mut zval,
) {
    guard(EntryPoint::SelftestPanic, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { accepts_no_arguments(execute_data) } {
            return;
        }
        panic!("deliberate panic injected by opcov\\selftest_panic()");
    });
}

unsafe extern "C" fn selftest_force_watch(
    execute_data: *mut zend_execute_data,
    _return_value: *mut zval,
) {
    guard(EntryPoint::SelftestForceWatch, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if unsafe { accepts_no_arguments(execute_data) } {
            crate::branches::FORCE_WATCH.store(true, core::sync::atomic::Ordering::Relaxed);
        }
    });
}

unsafe extern "C" fn selftest_panic_in_probe(
    execute_data: *mut zend_execute_data,
    _return_value: *mut zval,
) {
    guard(EntryPoint::SelftestPanicInProbe, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if unsafe { accepts_no_arguments(execute_data) } {
            crate::probes::INJECT_PANIC.store(true, core::sync::atomic::Ordering::Relaxed);
        }
    });
}

unsafe extern "C" fn selftest_fail_snapshot(
    execute_data: *mut zend_execute_data,
    _return_value: *mut zval,
) {
    guard(EntryPoint::SelftestFailSnapshot, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if unsafe { accepts_no_arguments(execute_data) } {
            crate::compiler::FAIL_NEXT_SNAPSHOT.store(true, core::sync::atomic::Ordering::Relaxed);
        }
    });
}
