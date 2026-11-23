use core::slice;

use opcov_core::cfg::Opcode;
use opcov_sys::{ZEND_ACC_ANON_CLASS, ZEND_ACC_CLOSURE, zend_class_entry, zend_op, zend_op_array};

use crate::{engine::Map, snapshot::PHP, zend};

type Group = (Vec<u8>, u32, u32);

#[derive(Debug, Default)]
pub(crate) struct Ordinals {
    closures: Map<usize, u32>,
    classes: Map<usize, u32>,
}

fn assign(next: &mut Map<Group, u32>, group: Group) -> u32 {
    let slot = next.entry(group).or_insert(0);
    let ordinal = *slot;
    *slot = slot.saturating_add(1);
    return ordinal;
}

unsafe fn declaration_counter(class: &zend_class_entry) -> Option<u32> {
    // SAFETY: a class entry's name is a live zend_string.
    let name = unsafe { zend::bytes(class.name) }?;
    let position = name.iter().rposition(|&byte| return byte == b'$')?;
    let digits = str::from_utf8(name.get(position.checked_add(1)?..)?).ok()?;
    return u32::from_str_radix(digits, 16).ok();
}

unsafe fn opcodes<'a>(array: &zend_op_array) -> &'a [zend_op] {
    let Ok(count) = usize::try_from(array.last) else {
        return &[];
    };
    if array.opcodes.is_null() || count == 0 {
        return &[];
    }
    // SAFETY: the caller passes a live, compiled op array, which owns `last` opcodes.
    return unsafe { slice::from_raw_parts(array.opcodes, count) };
}

unsafe fn nested<'a>(array: &zend_op_array) -> &'a [*mut zend_op_array] {
    let Ok(count) = usize::try_from(array.num_dynamic_func_defs) else {
        return &[];
    };
    if array.dynamic_func_defs.is_null() || count == 0 {
        return &[];
    }
    // SAFETY: the caller passes a live op array, which owns `num_dynamic_func_defs` entries.
    return unsafe { slice::from_raw_parts(array.dynamic_func_defs, count) };
}

fn same_op(left: &zend_op, right: &zend_op) -> bool {
    // SAFETY: every member of an operand union is a plain u32, so reading `num` reads the bits.
    let operands = |op: &zend_op| return unsafe { (op.op1.num, op.op2.num, op.result.num) };
    return (
        left.opcode,
        left.op1_type,
        left.op2_type,
        left.result_type,
        left.extended_value,
        left.lineno,
    ) == (
        right.opcode,
        right.op1_type,
        right.op2_type,
        right.result_type,
        right.extended_value,
        right.lineno,
    ) && operands(left) == operands(right);
}

unsafe fn same_closure(left: *const zend_op_array, right: *const zend_op_array) -> bool {
    // SAFETY: the caller passes live, compiled op arrays.
    let (left, right) = unsafe { (&*left, &*right) };
    if (left.line_start, left.line_end, left.last, left.last_literal)
        != (
            right.line_start,
            right.line_end,
            right.last,
            right.last_literal,
        )
        || left.fn_flags & right.fn_flags & ZEND_ACC_CLOSURE == 0
    {
        return false;
    }
    // SAFETY: as above.
    let (left, right) = unsafe { (opcodes(left), opcodes(right)) };
    return left
        .iter()
        .zip(right)
        .all(|(left, right)| return same_op(left, right));
}

unsafe fn frameless_twins(op_arrays: &[*mut zend_op_array]) -> Map<usize, usize> {
    let mut twins = Map::default();
    for &parent in op_arrays {
        // SAFETY: the caller passes live, compiled op arrays.
        let array = unsafe { &*parent };
        // SAFETY: as above.
        let frameless = unsafe { opcodes(array) }
            .iter()
            .any(|op| return Opcode::decode(PHP, op.opcode) == Opcode::JmpFrameless);
        if !frameless {
            continue;
        }
        // SAFETY: as above.
        let children = unsafe { nested(array) };
        for (position, &child) in children.iter().enumerate() {
            let earlier = children.get(..position).unwrap_or_default();
            // SAFETY: a parent's dynamic function definitions are live, compiled op arrays.
            let original = earlier
                .iter()
                .find(|&&candidate| return unsafe { same_closure(candidate, child) });
            if let Some(&original) = original {
                let canonical = twins
                    .get(&original.addr())
                    .copied()
                    .unwrap_or_else(|| return original.addr());
                twins.insert(child.addr(), canonical);
            }
        }
    }
    return twins;
}

impl Ordinals {
    pub(crate) unsafe fn rank(op_arrays: &[*mut zend_op_array]) -> Self {
        let mut ordinals = Self::default();
        let mut next_closure: Map<Group, u32> = Map::default();
        let mut anonymous: Vec<(u32, usize, Group)> = Vec::new();
        // SAFETY: forwarded from the caller.
        let twins = unsafe { frameless_twins(op_arrays) };
        for &op_array in op_arrays {
            // SAFETY: the caller passes live, compiled op arrays.
            let array = unsafe { &*op_array };
            // SAFETY: a compiled op array's file name is a live zend_string.
            let file = unsafe { zend::bytes(array.filename) }.unwrap_or_default();
            if array.fn_flags & ZEND_ACC_CLOSURE != 0
                && !ordinals.closures.contains_key(&op_array.addr())
            {
                let twin = twins
                    .get(&op_array.addr())
                    .and_then(|original| return ordinals.closures.get(original))
                    .copied();
                let ordinal = twin.unwrap_or_else(|| {
                    return assign(
                        &mut next_closure,
                        (file.to_vec(), array.line_start, array.line_end),
                    );
                });
                ordinals.closures.insert(op_array.addr(), ordinal);
            }
            let scope = array.scope;
            if scope.is_null() || anonymous.iter().any(|seen| return seen.1 == scope.addr()) {
                continue;
            }
            // SAFETY: a compiled op array's scope is a live class entry.
            let class = unsafe { &*scope };
            if class.ce_flags & ZEND_ACC_ANON_CLASS == 0 {
                continue;
            }
            // SAFETY: as above.
            let Some(counter) = (unsafe { declaration_counter(class) }) else {
                continue;
            };
            // SAFETY: an anonymous class is a user class, whose `info.user` arm is the active one.
            let user = unsafe { class.info.user };
            // SAFETY: a user class's file name is a live zend_string.
            let class_file = unsafe { zend::bytes(user.filename) }.unwrap_or_default();
            anonymous.push((
                counter,
                scope.addr(),
                (class_file.to_vec(), user.line_start, user.line_end),
            ));
        }
        anonymous.sort_unstable_by_key(|&(counter, _, _)| return counter);
        let mut next_class: Map<Group, u32> = Map::default();
        for (_, class, group) in anonymous {
            let ordinal = assign(&mut next_class, group);
            ordinals.classes.insert(class, ordinal);
        }
        return ordinals;
    }

    pub(crate) fn closure(&self, op_array: *const zend_op_array) -> Option<u32> {
        return self.closures.get(&op_array.addr()).copied();
    }

    pub(crate) fn class(&self, class: *const zend_class_entry) -> Option<u32> {
        return self.classes.get(&class.addr()).copied();
    }
}
