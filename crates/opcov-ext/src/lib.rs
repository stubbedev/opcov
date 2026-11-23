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

mod guard;
mod info;
#[cfg(feature = "engine-selftest")]
mod selftest;

use core::{ffi::CStr, ptr};

use guard::{EntryPoint, guard};
use opcov_sys::{
    ArgInfo, FunctionTable, MAY_BE_STRING, Module, ModuleDefinition, function_entry,
    opcov_parse_parameters_none, opcov_zval_stringl, zend_execute_data, zend_function_entry,
    zend_module_entry, zval,
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
static FUNCTIONS: FunctionTable<1> = FunctionTable::new([VERSION_FUNCTION]);

#[cfg(feature = "engine-selftest")]
static FUNCTIONS: FunctionTable<3> = FunctionTable::new([
    VERSION_FUNCTION,
    selftest::ENGINE_SELFTEST_FUNCTION,
    selftest::SELFTEST_PANIC_FUNCTION,
]);

static MODULE: Module = Module::new(ModuleDefinition {
    name: c"opcov",
    version: VERSION,
    functions: &FUNCTIONS,
    module_startup: None,
    module_shutdown: None,
    request_startup: None,
    request_shutdown: None,
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
    unsafe { opcov_zval_stringl(return_value, value.as_ptr().cast(), value.len()) };
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
