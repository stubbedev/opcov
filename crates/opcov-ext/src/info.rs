use std::ffi::{CStr, CString};

use opcov_sys::{
    PHP_API_VERSION, PHP_VERSION, VmKind, ZTS, display_ini_entries, opcov_php_version,
    php_info_print_table_end, php_info_print_table_row, php_info_print_table_start,
    zend_module_entry, zend_vm_kind,
};

use crate::{
    VERSION,
    api::STARTUP,
    code::Explained,
    conflicts::Environment,
    engine::OffReason,
    guard::{COLLECTION, EntryPoint, guard},
    zend::c_text,
};

pub(crate) fn vm_kind() -> CString {
    // SAFETY: zend_vm_kind only returns a compile-time constant of the running engine.
    let raw = unsafe { zend_vm_kind() };
    return VmKind::try_from(raw).map_or_else(
        |unknown| return c_text(&format!("unknown ({})", unknown.0)),
        |kind| return kind.name().to_owned(),
    );
}

const fn thread_safety() -> &'static str {
    return if ZTS { "ZTS" } else { "NTS" };
}

fn row(key: &CStr, value: &CStr) {
    // SAFETY: both arguments are NUL-terminated strings that outlive the call, matching the two
    // columns announced by the first argument.
    unsafe { php_info_print_table_row(2, key.as_ptr(), value.as_ptr()) };
}

fn collection() -> CString {
    if let Some(disabled) = COLLECTION.disabled() {
        return c_text(&format!("disabled: {}", OffReason::Panic(disabled.clone())));
    }
    return STARTUP.get().map_or_else(
        || return c"available".to_owned(),
        |reason| return c_text(&format!("off: {reason}")),
    );
}

fn opcache(environment: &Environment) -> CString {
    let Some(opcache) = &environment.opcache else {
        return c"not loaded".to_owned();
    };
    if !opcache.active_under(&environment.sapi) {
        return c_text(&format!("off for the {} SAPI", environment.sapi));
    }
    return c_text(&format!(
        "on (opcache.jit={}, opcache.optimization_level={})",
        opcache.jit, opcache.optimization_level
    ));
}

fn conflicts(environment: &Environment) -> CString {
    let found = environment.conflicts();
    if found.is_empty() {
        return c"none".to_owned();
    }
    let described: Vec<String> = found
        .iter()
        .map(|conflict| {
            return Explained {
                code: conflict.code(),
                text: conflict,
            }
            .to_string();
        })
        .collect();
    return c_text(&described.join("; "));
}

fn running_on(environment: &Environment) -> CString {
    // SAFETY: php_version returns a static NUL-terminated string.
    let version = unsafe { CStr::from_ptr(opcov_php_version()) };
    return c_text(&format!(
        "PHP {}, SAPI {}",
        version.to_string_lossy(),
        environment.sapi
    ));
}

fn built_for() -> CString {
    return c_text(&format!(
        "PHP {} {}, API {PHP_API_VERSION}",
        PHP_VERSION.to_string_lossy(),
        thread_safety()
    ));
}

pub(crate) unsafe extern "C" fn print(module: *mut zend_module_entry) {
    guard(EntryPoint::Info, (), || {
        let environment = Environment::current();
        let rows = [
            (c"Version", VERSION.to_owned()),
            (c"Built for", built_for()),
            (c"Running on", running_on(&environment)),
            (c"VM kind", vm_kind()),
            (c"OPcache", opcache(&environment)),
            (c"Conflicts", conflicts(&environment)),
            (c"Collection", collection()),
        ];
        // SAFETY: the engine calls the info hook while it prints module information, which is when
        // the table functions may be used.
        unsafe { php_info_print_table_start() };
        for (key, value) in &rows {
            row(key, value);
        }
        // SAFETY: closes the table opened above, then prints the module's registered INI entries.
        unsafe {
            php_info_print_table_end();
            display_ini_entries(module);
        }
    });
}
