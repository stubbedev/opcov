use core::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use opcov_sys::{
    add_assoc_zval_ex, opcov_add_index_zval, opcov_add_next_index_zval, opcov_array_init,
    opcov_register_class, opcov_string_init, opcov_string_release, opcov_zval_bool,
    opcov_zval_long, opcov_zval_null, opcov_zval_stringl, spl_ce_LogicException,
    spl_ce_RuntimeException, zend_class_entry, zend_throw_exception, zval,
};
use serde_json::Value;

const CANNOT_COLLECT: &str = "opcov\\CannotCollectException";

static CANNOT_COLLECT_CLASS: AtomicPtr<zend_class_entry> = AtomicPtr::new(ptr::null_mut());

pub(crate) fn register_classes() {
    // SAFETY: called once from module startup; RuntimeException is registered by SPL, which PHP
    // always starts before dynamically loaded extensions.
    let class = unsafe {
        opcov_register_class(
            CANNOT_COLLECT.as_ptr().cast(),
            CANNOT_COLLECT.len(),
            spl_ce_RuntimeException,
        )
    };
    CANNOT_COLLECT_CLASS.store(class, Ordering::Release);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exception {
    CannotCollect,
    Logic,
}

pub(crate) fn throw(kind: Exception, message: &str) {
    let class = match kind {
        Exception::CannotCollect => CANNOT_COLLECT_CLASS.load(Ordering::Acquire),
        // SAFETY: SPL's exception classes are registered for the whole process lifetime.
        Exception::Logic => unsafe { spl_ce_LogicException },
    };
    // SAFETY: creates a request-scoped, NUL-terminated copy of the message, so no Rust value is held
    // across the throw.
    let copy = unsafe { opcov_string_init(message.as_ptr().cast(), message.len(), false) };
    // SAFETY: `copy` is a live zend_string whose value is NUL-terminated; the engine copies it.
    unsafe {
        zend_throw_exception(class, (&raw const (*copy).val).cast(), 0);
        opcov_string_release(copy);
    }
}

pub(crate) unsafe fn array(target: *mut zval, size: usize) {
    // SAFETY: the caller passes a writable zval.
    unsafe { opcov_array_init(target, u32::try_from(size).unwrap_or(u32::MAX)) };
}

pub(crate) unsafe fn string(target: *mut zval, text: &str) {
    // SAFETY: the caller passes a writable zval; the engine copies `text`.
    unsafe { opcov_zval_stringl(target, text.as_ptr().cast(), text.len()) };
}

pub(crate) unsafe fn set(array: *mut zval, key: &str, value: &mut zval) {
    // SAFETY: the caller passes a live array zval; the array takes ownership of `value`.
    unsafe { add_assoc_zval_ex(array, key.as_ptr().cast(), key.len(), value) };
}

pub(crate) unsafe fn set_index(array: *mut zval, index: u32, value: &mut zval) {
    // SAFETY: the caller passes a live array zval; the array takes ownership of `value`.
    unsafe { opcov_add_index_zval(array, u64::from(index), value) };
}

pub(crate) unsafe fn push(array: *mut zval, value: &mut zval) {
    // SAFETY: the caller passes a live array zval; the array takes ownership of `value`.
    unsafe { opcov_add_next_index_zval(array, value) };
}

pub(crate) unsafe fn bool_value(value: bool) -> zval {
    let mut target = zval::default();
    // SAFETY: writes a scalar into a local zval.
    unsafe { opcov_zval_bool(&raw mut target, value) };
    return target;
}

pub(crate) unsafe fn emit(target: *mut zval, value: &Value) {
    // SAFETY: the caller passes a writable zval; every child is moved into its parent array.
    unsafe {
        match value {
            Value::Null => opcov_zval_null(target),
            Value::Bool(flag) => opcov_zval_bool(target, *flag),
            Value::Number(number) => match number.as_i64() {
                Some(integer) => opcov_zval_long(target, integer),
                None => string(target, &number.to_string()),
            },
            Value::String(text) => string(target, text),
            Value::Array(items) => {
                array(target, items.len());
                for item in items {
                    let mut child = zval::default();
                    emit(&raw mut child, item);
                    push(target, &mut child);
                }
            }
            Value::Object(entries) => {
                array(target, entries.len());
                for (key, item) in entries {
                    let mut child = zval::default();
                    emit(&raw mut child, item);
                    set(target, key, &mut child);
                }
            }
        }
    }
}
