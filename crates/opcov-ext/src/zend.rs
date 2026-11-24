use core::slice;

use opcov_sys::{
    HashTable, opcov_compiler_globals, opcov_executor_globals, opcov_map_ptr_at, zend_class_entry,
    zend_compiler_globals, zend_executor_globals, zend_function, zend_string,
};

pub(crate) const unsafe fn bytes<'a>(string: *const zend_string) -> Option<&'a [u8]> {
    if string.is_null() {
        return None;
    }
    // SAFETY: the caller passes a live zend_string; its `len` bytes follow the header in `val`, and
    // the pointer is derived from the whole allocation.
    return Some(unsafe {
        slice::from_raw_parts((&raw const (*string).val).cast::<u8>(), (*string).len)
    });
}

pub(crate) fn compiler_globals() -> *mut zend_compiler_globals {
    // SAFETY: returns the current thread's compiler globals, valid for the whole request.
    return unsafe { opcov_compiler_globals() };
}

pub(crate) fn executor_globals() -> *mut zend_executor_globals {
    // SAFETY: returns the current thread's executor globals, valid for the whole request.
    return unsafe { opcov_executor_globals() };
}

pub(crate) const unsafe fn map_used(map: *const HashTable) -> u32 {
    // SAFETY: the caller passes a live hash table.
    return unsafe { (*map).nNumUsed };
}

pub(crate) unsafe fn map_values<T>(
    map: *const HashTable,
    from: u32,
) -> impl Iterator<Item = *mut T> {
    // SAFETY: forwarded from the caller.
    let used = unsafe { map_used(map) };
    return (from..used).filter_map(move |index| {
        // SAFETY: `index` is below nNumUsed of a live map whose values are pointers.
        let value = unsafe { opcov_map_ptr_at(map, index) };
        return (!value.is_null()).then(|| return value.cast::<T>());
    });
}

pub(crate) unsafe fn function_table_used() -> u32 {
    // SAFETY: the compiler globals hold a live function table during a request.
    return unsafe { map_used((*compiler_globals()).function_table) };
}

pub(crate) unsafe fn class_table_used() -> u32 {
    // SAFETY: the compiler globals hold a live class table during a request.
    return unsafe { map_used((*compiler_globals()).class_table) };
}

pub(crate) unsafe fn new_functions(from: u32) -> impl Iterator<Item = *mut zend_function> {
    // SAFETY: the compiler globals hold a live function table during a request.
    return unsafe { map_values((*compiler_globals()).function_table, from) };
}

pub(crate) unsafe fn new_classes(from: u32) -> impl Iterator<Item = *mut zend_class_entry> {
    // SAFETY: the compiler globals hold a live class table during a request.
    return unsafe { map_values((*compiler_globals()).class_table, from) };
}

pub(crate) fn c_text(text: &str) -> std::ffi::CString {
    let escaped = text.replace('\0', "\\0").into_bytes();
    // SAFETY: every NUL byte was just replaced by the two bytes `\0`.
    return unsafe { std::ffi::CString::from_vec_unchecked(escaped) };
}
