#![expect(
    unsafe_code,
    reason = "the PHP extension boundary: exported module entry, engine callbacks and calls into opcov-sys"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "rustc's unreachable_pub, which the workspace denies, requires pub(crate) in private modules"
)]

#[cfg(panic = "abort")]
compile_error!(
    "opcov-ext must be built with panic = \"unwind\": every engine entry point catches panics so that PHP survives them"
);

mod api;
mod branches;
mod code;
mod compiler;
mod conflicts;
mod engine;
mod guard;
mod hits;
mod info;
mod ini;
mod lines;
mod ordinals;
mod php;
mod probes;
mod report;
mod select;
#[cfg(feature = "engine-selftest")]
mod selftest;
mod snapshot;
mod threads;
mod zend;

use core::{
    ffi::{CStr, c_int},
    ptr,
};

use conflicts::Conflicts;
use engine::{Engine, OffReason, with_engine};
use guard::{EntryPoint, guard};
use opcov_sys::{
    ArgInfo, FunctionTable, MAY_BE_STRING, Module, ModuleDefinition, SUCCESS,
    ZEND_COMPILE_NO_JUMPTABLES, function_entry, opcov_parse_parameters_none, opcov_zval_stringl,
    zend_execute_data, zend_extension, zend_function_entry, zend_get_resource_handle,
    zend_module_entry, zend_register_extension, zend_result, zval,
};

const VERSION: &CStr =
    match CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
        Ok(version) => version,
        Err(_) => panic!("the crate version must not contain NUL"),
    };

static VERSION_ARGINFO: ArgInfo<0> = ArgInfo::returning(MAY_BE_STRING);

const VERSION_FUNCTION: zend_function_entry =
    function_entry(c"opcov\\version", version, &VERSION_ARGINFO);

#[cfg(not(feature = "engine-selftest"))]
static FUNCTIONS: FunctionTable<9> = FunctionTable::new([
    VERSION_FUNCTION,
    api::ENSURE_CAN_COLLECT,
    api::START,
    api::STOP,
    api::RESET,
    api::EXECUTABLE,
    api::GET_STATUS,
    api::GET_DEBUG_INFO,
    api::WRITE_REPORT,
]);

#[cfg(feature = "engine-selftest")]
static FUNCTIONS: FunctionTable<15> = FunctionTable::new([
    VERSION_FUNCTION,
    api::ENSURE_CAN_COLLECT,
    api::START,
    api::STOP,
    api::RESET,
    api::EXECUTABLE,
    api::GET_STATUS,
    api::GET_DEBUG_INFO,
    api::WRITE_REPORT,
    selftest::ENGINE_SELFTEST_FUNCTION,
    selftest::SELFTEST_PANIC_FUNCTION,
    selftest::SELFTEST_PANIC_IN_PROBE_FUNCTION,
    selftest::SELFTEST_FORCE_WATCH_FUNCTION,
    selftest::SELFTEST_FAIL_SNAPSHOT_FUNCTION,
    api::SELFTEST_CFG_INPUTS,
]);

static MODULE: Module = Module::new(ModuleDefinition {
    name: c"opcov",
    version: VERSION,
    functions: &FUNCTIONS,
    module_startup: Some(module_startup),
    module_shutdown: Some(module_shutdown),
    request_startup: Some(request_startup),
    request_shutdown: Some(request_shutdown),
    info: Some(info::print),
});

#[unsafe(no_mangle)]
pub extern "C" fn get_module() -> *mut zend_module_entry {
    return guard(EntryPoint::GetModule, ptr::null_mut(), || {
        return MODULE.as_ptr();
    });
}

unsafe fn accepts_no_arguments(execute_data: *mut zend_execute_data) -> bool {
    // SAFETY: the caller passes the frame the engine handed to an internal function.
    return unsafe { opcov_parse_parameters_none(execute_data) };
}

unsafe fn return_string(return_value: *mut zval, value: &[u8]) {
    // SAFETY: the caller passes the engine's return slot; the engine copies `value` into a new
    // zend_string before this returns.
    guard::record(unsafe { opcov_zval_stringl(return_value, value.as_ptr().cast(), value.len()) });
}

unsafe extern "C" fn version(execute_data: *mut zend_execute_data, return_value: *mut zval) {
    guard(EntryPoint::Version, (), || {
        // SAFETY: the engine calls this handler with the frame and return slot of this call.
        unsafe {
            if accepts_no_arguments(execute_data) {
                return_string(return_value, VERSION.to_bytes());
            }
        }
    });
}

const TRACKER: &CStr = c"opcov op_array tracker";

fn startup(module_type: c_int, module_number: c_int) {
    if !ini::register(module_type, module_number) {
        api::STARTUP.get_or_init(|| return OffReason::IniRegistrationFailed);
        return;
    }
    php::register_classes();
    if !ini::enabled() {
        api::STARTUP.get_or_init(|| return OffReason::NotEnabledAtStartup);
        return;
    }
    // SAFETY: claims one op_array.reserved slot under a NUL-terminated name, at module startup.
    let Ok(slot) = usize::try_from(unsafe { zend_get_resource_handle(TRACKER.as_ptr()) }) else {
        api::STARTUP.get_or_init(|| return OffReason::NoReservedSlot);
        return;
    };
    let mut tracker = zend_extension {
        name: TRACKER.as_ptr(),
        version: VERSION.as_ptr(),
        author: c"".as_ptr(),
        URL: c"".as_ptr(),
        copyright: c"".as_ptr(),
        op_array_dtor: Some(compiler::op_array_dtor),
        resource_number: -1,
        ..zend_extension::default()
    };
    // SAFETY: the engine copies the struct; its strings are 'static and its only hook is the
    // op_array destructor.
    unsafe { zend_register_extension(&raw mut tracker, ptr::null_mut()) };
    engine::install(Engine::new(slot));
}

unsafe extern "C" fn module_startup(module_type: c_int, module_number: c_int) -> zend_result {
    guard(EntryPoint::ModuleStartup, (), || {
        startup(module_type, module_number);
    });
    return SUCCESS;
}

unsafe extern "C" fn module_shutdown(module_type: c_int, module_number: c_int) -> zend_result {
    guard(EntryPoint::ModuleShutdown, (), || {
        let installed = with_engine(|engine| return engine.label.is_some()).unwrap_or(false);
        if installed {
            threads::with_handler_table(probes::unregister);
        }
        engine::uninstall();
        ini::unregister(module_type, module_number);
    });
    return SUCCESS;
}

fn install(engine: &mut Engine) {
    if engine.label.is_some() {
        return;
    }
    let Some(label) = threads::with_handler_table(probes::register_with_previous) else {
        return;
    };
    engine.label = Some(label);
    if !compiler::hook() {
        engine.set_problem(OffReason::CompileHooksMissing);
    }
}

fn activate() {
    if api::STARTUP.get().is_some() || !threads::claim() {
        return;
    }
    if !ini::enabled() {
        with_engine(|engine| engine.request = None);
        return;
    }
    let mut request = select::configure();
    let conflicts = Conflicts::of(request.environment.conflicts());
    let probing = conflicts.is_none();
    let measuring = probing && request.selection.is_some();
    if let Some(conflicts) = conflicts {
        request.problem = Some(OffReason::Conflicts(conflicts));
    }
    with_engine(|engine| {
        engine.request = Some(request);
        engine.collecting = false;
        if probing {
            install(engine);
        }
    });
    if measuring {
        let globals = zend::compiler_globals();
        // SAFETY: the compiler globals are live during the request; measured files compile without
        // jump tables unless the compile hook decides otherwise per file.
        unsafe { (*globals).compiler_options |= ZEND_COMPILE_NO_JUMPTABLES };
    }
}

unsafe extern "C" fn request_startup(_module_type: c_int, _module_number: c_int) -> zend_result {
    guard(EntryPoint::RequestStartup, (), activate);
    return SUCCESS;
}

unsafe extern "C" fn request_shutdown(_module_type: c_int, _module_number: c_int) -> zend_result {
    guard(EntryPoint::RequestShutdown, (), || {
        if threads::foreign() {
            return;
        }
        let pattern = with_engine(|engine| {
            engine.collecting = false;
            engine.rearm_all();
            return engine
                .request
                .take()
                .and_then(|request| return request.pattern);
        })
        .flatten();
        if let Some(pattern) = pattern {
            pattern.release();
        }
    });
    return SUCCESS;
}
