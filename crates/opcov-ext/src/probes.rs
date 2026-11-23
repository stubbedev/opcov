use core::ffi::c_int;
use std::sync::OnceLock;

use opcov_sys::{
    IS_UNUSED, ZEND_NOP, ZEND_USER_OPCODE, ZEND_USER_OPCODE_CONTINUE, ZEND_USER_OPCODE_DISPATCH,
    opcov_is_user_code, user_opcode_handler_t, zend_execute_data, zend_get_user_opcode_handler,
    zend_op, zend_op_array, zend_set_user_opcode_handler, zend_vm_set_opcode_handler,
};

use crate::{
    compiler,
    engine::{Engine, Handler, ProbeRef, Reserved, Role, UnitId, with_engine},
    guard::{COLLECTION, EntryPoint, guard},
    threads,
};

const OPCODES: usize = 256;

static PREVIOUS: OnceLock<[user_opcode_handler_t; OPCODES]> = OnceLock::new();

#[cfg(feature = "engine-selftest")]
pub(crate) static INJECT_PANIC: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn probed_opcodes() -> impl Iterator<Item = u8> {
    return (0..=u8::MAX).filter(|&opcode| return opcode != ZEND_USER_OPCODE);
}

fn set_all(handler: impl Fn(u8) -> user_opcode_handler_t) {
    for opcode in probed_opcodes() {
        // SAFETY: installs a handler (or none) for a valid opcode other than ZEND_USER_OPCODE; the
        // caller holds the handler-table lock.
        unsafe { zend_set_user_opcode_handler(opcode, handler(opcode)) };
    }
}

fn previous(opcode: u8) -> user_opcode_handler_t {
    return PREVIOUS
        .get()
        .and_then(|table| return table.get(usize::from(opcode)).copied())
        .flatten();
}

pub(crate) fn register_with_previous() -> Handler {
    PREVIOUS.get_or_init(|| {
        let mut table: [user_opcode_handler_t; OPCODES] = [None; OPCODES];
        for opcode in probed_opcodes() {
            if let Some(entry) = table.get_mut(usize::from(opcode)) {
                // SAFETY: reads the process-wide user opcode handler table for a valid opcode.
                *entry = unsafe { zend_get_user_opcode_handler(opcode) };
            }
        }
        return table;
    });
    register();
    let mut nop = zend_op {
        opcode: ZEND_NOP,
        op1_type: IS_UNUSED,
        op2_type: IS_UNUSED,
        result_type: IS_UNUSED,
        ..zend_op::default()
    };
    // SAFETY: a NOP with unused operands needs no operand to pick its handler; while every opcode
    // has a user handler, the VM picks the ZEND_USER_OPCODE handler, which is the probe label.
    unsafe { zend_vm_set_opcode_handler(&raw mut nop) };
    // SAFETY: `nop` is a live local opcode.
    return unsafe { Handler::of(&raw const nop) };
}

pub(crate) fn register() {
    set_all(|_| return Some(probe));
}

pub(crate) fn unregister() {
    set_all(previous);
}

pub(crate) unsafe fn reset_handlers(op_array: *mut zend_op_array) {
    // SAFETY: the caller passes a live op array.
    let (opcodes, last) = unsafe { ((*op_array).opcodes, (*op_array).last) };
    for index in 0..last {
        let Ok(offset) = usize::try_from(index) else {
            return;
        };
        // SAFETY: `offset` is below `last`, the op array's opcode count; the caller unregistered the
        // user handlers, so the VM picks each opcode's real handler.
        unsafe { zend_vm_set_opcode_handler(opcodes.wrapping_add(offset)) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Continue,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Done(Outcome),
    Adopt,
}

impl Engine {
    pub(crate) fn fire(&mut self, unit: UnitId, probe: usize) {
        let recording = self.request.is_some() && COLLECTION.disabled().is_none();
        let Some(owner) = self.units.get_mut(unit) else {
            return;
        };
        let opcodes = owner.opcodes;
        let Some(slot) = owner.probes.get_mut(probe) else {
            return;
        };
        slot.armed.remove(Role::Line);
        if slot.armed.is_empty()
            && let Some(opcodes) = opcodes
        {
            // SAFETY: the unit is alive, so its opcodes are; `op` is below `last`.
            unsafe { slot.handler.install(opcodes.wrapping_add(slot.op)) };
        }
        if recording {
            self.record(ProbeRef { unit, probe });
        }
    }

    unsafe fn hit(&mut self, op_array: *const zend_op_array, opline: *const zend_op) -> Hit {
        // SAFETY: forwarded from the caller.
        let reserved = unsafe { Reserved::read(op_array, self.slot) };
        let id = match reserved {
            Reserved::Unknown if self.label.is_some() => return Hit::Adopt,
            Reserved::Unknown | Reserved::NoProbes => return Hit::Done(Outcome::Forward),
            Reserved::Unit(id) => id,
        };
        let Some(unit) = self.units.get(id) else {
            return Hit::Done(Outcome::Forward);
        };
        let Some(opcodes) = unit.opcodes else {
            return Hit::Done(Outcome::Forward);
        };
        let op = opline
            .addr()
            .checked_sub(opcodes.addr())
            .and_then(|bytes| return bytes.checked_div(size_of::<zend_op>()))
            .filter(|&op| return op < unit.last);
        let Some(probe) = op.and_then(|op| return unit.probe_at(op)) else {
            return Hit::Done(Outcome::Forward);
        };
        let armed = unit
            .probes
            .get(probe)
            .is_some_and(|slot| return slot.armed.contains(Role::Line));
        if !armed {
            return Hit::Done(Outcome::Forward);
        }
        #[cfg(feature = "engine-selftest")]
        inject_panic();
        self.fire(id, probe);
        return Hit::Done(Outcome::Continue);
    }
}

#[cfg(feature = "engine-selftest")]
fn inject_panic() {
    assert!(
        !INJECT_PANIC.swap(false, std::sync::atomic::Ordering::Relaxed),
        "deliberate panic injected into the probe callback"
    );
}

unsafe fn fire(execute_data: *mut zend_execute_data) -> Outcome {
    if threads::foreign() {
        return Outcome::Forward;
    }
    // SAFETY: the VM passes the frame of the opcode it is executing.
    let (opline, function) = unsafe { ((*execute_data).opline, (*execute_data).func) };
    // SAFETY: a non-null frame function is live while its frame runs.
    if function.is_null() || opline.is_null() || !unsafe { opcov_is_user_code(function) } {
        return Outcome::Forward;
    }
    // SAFETY: user code is an op array.
    let op_array = unsafe { &raw mut (*function).op_array };
    // SAFETY: `op_array` and `opline` belong to the running frame.
    let first = with_engine(|engine| return unsafe { engine.hit(op_array, opline) });
    return match first {
        Some(Hit::Done(outcome)) => outcome,
        // SAFETY: `op_array` is the running frame's live op array.
        Some(Hit::Adopt) if unsafe { compiler::adopt(op_array) } => {
            // SAFETY: as above.
            match with_engine(|engine| return unsafe { engine.hit(op_array, opline) }) {
                Some(Hit::Done(outcome)) => outcome,
                Some(Hit::Adopt) | None => Outcome::Forward,
            }
        }
        Some(Hit::Adopt) | None => Outcome::Forward,
    };
}

unsafe fn forward(execute_data: *mut zend_execute_data) -> c_int {
    // SAFETY: the VM passes the frame of the opcode it is executing.
    let opline = unsafe { (*execute_data).opline };
    if opline.is_null() {
        return ZEND_USER_OPCODE_DISPATCH;
    }
    // SAFETY: checked non-null above; the opcode belongs to the running frame.
    let opcode = unsafe { (*opline).opcode };
    return previous(opcode).map_or(ZEND_USER_OPCODE_DISPATCH, |handler| {
        // SAFETY: a handler another extension registered for this opcode, called as the VM would.
        return unsafe { handler(execute_data) };
    });
}

unsafe extern "C" fn probe(execute_data: *mut zend_execute_data) -> c_int {
    // SAFETY: the VM calls this handler with the frame of the opcode it is executing.
    let outcome = guard(EntryPoint::Probe, Outcome::Forward, || unsafe {
        return fire(execute_data);
    });
    return match outcome {
        Outcome::Continue => ZEND_USER_OPCODE_CONTINUE,
        // SAFETY: as above.
        Outcome::Forward => unsafe { forward(execute_data) },
    };
}
