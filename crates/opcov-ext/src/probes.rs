use core::{ffi::c_int, slice};
use std::sync::OnceLock;

use opcov_sys::{
    IS_UNUSED, ZEND_NOP, ZEND_USER_OPCODE, ZEND_USER_OPCODE_CONTINUE, ZEND_USER_OPCODE_DISPATCH,
    user_opcode_handler_t, zend_execute_data, zend_get_user_opcode_handler, zend_op, zend_op_array,
    zend_set_user_opcode_handler, zend_vm_set_opcode_handler,
};

use crate::{
    branches::{After, Frame},
    compiler,
    engine::{Engine, FiredRef, Handler, Reserved, Role, Roles, with_engine},
    guard::{COLLECTION, EntryPoint, guard},
    threads, zend,
};

const OPCODES: usize = 256;

static PREVIOUS: OnceLock<[user_opcode_handler_t; OPCODES]> = OnceLock::new();

#[cfg(feature = "engine-selftest")]
pub(crate) static INJECT_PANIC: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Opcodes([u64; 4]);

impl Opcodes {
    const ALL: Self = Self([u64::MAX; 4]);

    fn locate(opcode: u8) -> (usize, u64) {
        return (
            usize::from(opcode >> 6),
            1_u64.wrapping_shl(u32::from(opcode & 63)),
        );
    }

    pub(crate) fn insert(&mut self, opcode: u8) {
        let (word, bit) = Self::locate(opcode);
        if let Some(bits) = self.0.get_mut(word) {
            *bits |= bit;
        }
    }

    fn contains(self, opcode: u8) -> bool {
        let (word, bit) = Self::locate(opcode);
        return self.0.get(word).is_some_and(|bits| return bits & bit != 0);
    }

    fn probed(self) -> impl Iterator<Item = u8> {
        return (0..=u8::MAX)
            .filter(move |&opcode| return opcode != ZEND_USER_OPCODE && self.contains(opcode));
    }
}

fn set(opcodes: Opcodes, handler: impl Fn(u8) -> user_opcode_handler_t) {
    for opcode in opcodes.probed() {
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
        for opcode in Opcodes::ALL.probed() {
            if let Some(entry) = table.get_mut(usize::from(opcode)) {
                // SAFETY: reads the process-wide user opcode handler table for a valid opcode.
                *entry = unsafe { zend_get_user_opcode_handler(opcode) };
            }
        }
        return table;
    });
    set(Opcodes::ALL, |_| return Some(probe));
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

pub(crate) fn unregister() {
    set(Opcodes::ALL, previous);
}

pub(crate) fn with_real_handlers<R>(opcodes: Opcodes, body: impl FnOnce() -> R) -> Option<R> {
    return threads::with_handler_table(|| {
        set(opcodes, previous);
        let result = body();
        set(opcodes, |_| return Some(probe));
        return result;
    });
}

pub(crate) unsafe fn opcodes_of(op_array: *const zend_op_array, present: &mut Opcodes) {
    // SAFETY: the caller passes a live op array.
    for op in unsafe { ops(op_array) } {
        present.insert(op.opcode);
    }
}

unsafe fn ops<'a>(op_array: *const zend_op_array) -> &'a mut [zend_op] {
    // SAFETY: the caller passes a live op array whose `last` opcodes start at `opcodes`.
    let (opcodes, last) = unsafe { ((*op_array).opcodes, (*op_array).last) };
    let Ok(last) = usize::try_from(last) else {
        return &mut [];
    };
    if opcodes.is_null() {
        return &mut [];
    }
    // SAFETY: as above; the owner thread is the only writer of the opcodes.
    return unsafe { slice::from_raw_parts_mut(opcodes, last) };
}

pub(crate) unsafe fn reset_handlers(op_array: *const zend_op_array) {
    // SAFETY: forwarded from the caller.
    for op in unsafe { ops(op_array) } {
        // SAFETY: a live opcode of the op array; the caller removed opcov's user handler for its
        // opcode, so the VM picks the opcode's real handler.
        unsafe { zend_vm_set_opcode_handler(op) };
    }
}

pub(crate) unsafe fn plain_handlers(
    opcodes: *mut zend_op,
    producers: &[(usize, u8)],
) -> Option<Vec<(usize, Handler)>> {
    let mut present = Opcodes::default();
    for &(op, _) in producers {
        // SAFETY: the caller passes live opcodes and producer indexes below `last`.
        present.insert(unsafe { (*opcodes.wrapping_add(op)).opcode });
    }
    return with_real_handlers(present, || {
        return producers
            .iter()
            .map(|&(op, plain_type)| {
                let opcode = opcodes.wrapping_add(op);
                // SAFETY: the caller passes live opcodes and producer indexes below `last`; opcov's
                // user handler is removed for the producers' opcodes, so the VM picks the real
                // handler for the operand types, which are put back right after.
                unsafe {
                    let smart_type = (*opcode).result_type;
                    let smart = Handler::of(opcode);
                    (*opcode).result_type = plain_type;
                    zend_vm_set_opcode_handler(opcode);
                    let plain = Handler::of(opcode);
                    (*opcode).result_type = smart_type;
                    smart.install(opcode);
                    return (op, plain);
                }
            })
            .collect();
    });
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
    unsafe fn hit(&mut self, op_array: *const zend_op_array, frame: Frame) -> Hit {
        // SAFETY: forwarded from the caller.
        let reserved = unsafe { Reserved::read(op_array, self.slot) };
        let id = match reserved {
            Reserved::Unknown if self.label.is_some() => return Hit::Adopt,
            Reserved::Unknown | Reserved::NoProbes => return Hit::Done(Outcome::Forward),
            Reserved::Unit(id) => id,
        };
        let Some(unit) = self.units.get_mut(id) else {
            return Hit::Done(Outcome::Forward);
        };
        let Some(opcodes) = unit.opcodes else {
            return Hit::Done(Outcome::Forward);
        };
        let op = frame
            .opline
            .addr()
            .checked_sub(opcodes.addr())
            .and_then(|bytes| return bytes.checked_div(size_of::<zend_op>()))
            .filter(|&op| return op < unit.last);
        let Some(probe) = op.and_then(|op| return unit.probe_at(op)) else {
            return Hit::Done(Outcome::Forward);
        };
        let Some(armed) = unit.probes.get(probe).map(|slot| return slot.armed) else {
            return Hit::Done(Outcome::Forward);
        };
        if armed.is_empty() {
            return Hit::Done(Outcome::Forward);
        }
        #[cfg(feature = "engine-selftest")]
        inject_panic();
        let recording = self.request.is_some() && COLLECTION.disabled().is_none();
        if armed == Roles::of(Role::Line) && self.pending.is_empty() {
            // SAFETY: the unit is alive while its probes fire, and the probe is one of its own.
            unsafe { unit.disarm(probe, Role::Line) };
            if recording && let Ok(number) = u32::try_from(probe) {
                self.record(FiredRef {
                    unit: id,
                    hit: crate::engine::Hit::Line(number),
                });
            }
            return Hit::Done(Outcome::Continue);
        }
        // SAFETY: the frame is the one the VM is executing and the probe belongs to its unit.
        let after = unsafe { self.trigger(recording, id, probe, frame) };
        return Hit::Done(match after {
            After::Restored => Outcome::Continue,
            After::Armed => Outcome::Forward,
        });
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
    if function.is_null() || opline.is_null() || !unsafe { zend::is_user_code(function) } {
        return Outcome::Forward;
    }
    // SAFETY: user code is an op array.
    let op_array = unsafe { &raw mut (*function).op_array };
    let frame = Frame {
        execute_data,
        opline,
    };
    // SAFETY: `op_array` and `opline` belong to the running frame.
    let first = with_engine(|engine| return unsafe { engine.hit(op_array, frame) });
    return match first {
        Some(Hit::Done(outcome)) => outcome,
        // SAFETY: `op_array` is the running frame's live op array.
        Some(Hit::Adopt) if unsafe { compiler::adopt(op_array) } => {
            // SAFETY: as above.
            match with_engine(|engine| return unsafe { engine.hit(op_array, frame) }) {
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
