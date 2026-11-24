use core::ffi::{CStr, c_int};

use opcov_sys::{
    IniTable, ZEND_INI_PERDIR, ZEND_INI_SYSTEM, ini_entry, opcov_ini_long, opcov_ini_string,
    zend_register_ini_entries_ex, zend_unregister_ini_entries_ex,
};

pub(crate) const ENABLED: &CStr = c"opcov.enabled";
pub(crate) const COLLECT_PATHS: &CStr = c"opcov.collect_paths";
pub(crate) const EXCLUDE_PATHS: &CStr = c"opcov.exclude_paths";
pub(crate) const EXCLUDE_PATTERN: &CStr = c"opcov.exclude_pattern";

const STARTUP_OR_DIRECTORY: u8 = ZEND_INI_SYSTEM | ZEND_INI_PERDIR;

static ENTRIES: IniTable<4> = IniTable::new([
    ini_entry(ENABLED, c"0", STARTUP_OR_DIRECTORY),
    ini_entry(COLLECT_PATHS, c"", STARTUP_OR_DIRECTORY),
    ini_entry(EXCLUDE_PATHS, c"", STARTUP_OR_DIRECTORY),
    ini_entry(EXCLUDE_PATTERN, c"", STARTUP_OR_DIRECTORY),
]);

pub(crate) fn register(module_type: c_int, module_number: c_int) -> bool {
    // SAFETY: called from module startup with the engine's module number and type; the table is
    // 'static and terminated by a null entry.
    let result =
        unsafe { zend_register_ini_entries_ex(ENTRIES.as_ptr(), module_number, module_type) };
    return result == opcov_sys::SUCCESS;
}

pub(crate) fn unregister(module_type: c_int, module_number: c_int) {
    // SAFETY: called from module shutdown with the module number the entries were registered under.
    unsafe { zend_unregister_ini_entries_ex(module_number, module_type) };
}

pub(crate) fn enabled() -> bool {
    // SAFETY: reads a registered INI entry by its NUL-terminated name and length.
    return unsafe { opcov_ini_long(ENABLED.as_ptr(), ENABLED.count_bytes()) } != 0;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotUtf8(pub(crate) &'static CStr);

fn with_value<R>(name: &CStr, read: impl FnOnce(&[u8]) -> R) -> R {
    // SAFETY: reads a registered INI entry by its NUL-terminated name and length; the engine keeps
    // the returned string alive until the entry changes, which cannot happen while `read` runs.
    let value = unsafe { opcov_ini_string(name.as_ptr(), name.count_bytes()) };
    if value.is_null() {
        return read(&[]);
    }
    // SAFETY: a non-null INI value is a NUL-terminated string owned by the engine.
    return read(unsafe { CStr::from_ptr(value) }.to_bytes());
}

pub(crate) fn refresh_string(name: &'static CStr, current: &mut String) -> Result<(), NotUtf8> {
    return with_value(name, |value| {
        if value == current.as_bytes() {
            return Ok(());
        }
        let text = str::from_utf8(value).map_err(|_utf8| return NotUtf8(name))?;
        current.clear();
        current.push_str(text);
        return Ok(());
    });
}

pub(crate) fn string(name: &'static CStr) -> Result<String, NotUtf8> {
    return with_value(name, |value| {
        return str::from_utf8(value)
            .map(str::to_owned)
            .map_err(|_utf8| return NotUtf8(name));
    });
}
