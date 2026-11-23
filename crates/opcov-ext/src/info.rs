use std::ffi::{CStr, CString};

use opcov_sys::{
    PHP_API_VERSION, VmKind, ZTS, display_ini_entries, php_info_print_table_end,
    php_info_print_table_row, php_info_print_table_start, zend_module_entry, zend_vm_kind,
};

use crate::{
    VERSION,
    api::STARTUP,
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

fn row(key: &CStr, value: &CStr) {
    // SAFETY: both arguments are NUL-terminated strings that outlive the call, matching the two
    // columns announced by the first argument.
    unsafe { php_info_print_table_row(2, key.as_ptr(), value.as_ptr()) };
}

pub(crate) unsafe extern "C" fn print(module: *mut zend_module_entry) {
    guard(EntryPoint::Info, (), || {
        let collection = COLLECTION.disabled().map_or_else(
            || {
                return STARTUP.get().map_or_else(
                    || return c"available".to_owned(),
                    |reason| return c_text(&format!("off: {reason}")),
                );
            },
            |disabled| return c_text(&format!("disabled: {disabled}")),
        );
        // SAFETY: the engine calls the info hook while it prints module information, which is when
        // the table functions may be used.
        unsafe { php_info_print_table_start() };
        row(c"Version", VERSION);
        row(c"PHP API", &c_text(&PHP_API_VERSION.to_string()));
        row(c"Thread safety", if ZTS { c"ZTS" } else { c"NTS" });
        row(c"VM kind", &vm_kind());
        row(c"Collection", &collection);
        // SAFETY: closes the table opened above, then prints the module's registered INI entries.
        unsafe {
            php_info_print_table_end();
            display_ini_entries(module);
        }
    });
}
