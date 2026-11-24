use core::slice;
use std::collections::BTreeSet;

use opcov_core::cfg::{FunctionKey, Opcode};
use opcov_sys::{
    IS_UNUSED, ZEND_ACC_ANON_CLASS, ZEND_ACC_CLOSURE, zend_class_entry, zend_op, zend_op_array,
    znode_op,
};

use crate::{
    engine::Map,
    snapshot::{self, Names, PHP},
    zend,
};

pub(crate) type Group = (Vec<u8>, u32, u32);

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

const fn operand(kind: u8, value: znode_op) -> Option<u32> {
    if kind == IS_UNUSED {
        return None;
    }
    // SAFETY: every member of an operand union is a plain u32, so reading `num` reads the bits.
    return Some(unsafe { value.num });
}

fn same_op(left: &zend_op, right: &zend_op) -> bool {
    let operands = |op: &zend_op| {
        return (
            operand(op.op1_type, op.op1),
            operand(op.op2_type, op.op2),
            operand(op.result_type, op.result),
        );
    };
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
                // SAFETY: both are live, compiled op arrays of the same parent.
                unsafe { pair(&mut twins, original, child) };
            }
        }
    }
    return twins;
}

unsafe fn pair(
    twins: &mut Map<usize, usize>,
    original: *mut zend_op_array,
    copy: *mut zend_op_array,
) {
    let canonical = twins
        .get(&original.addr())
        .copied()
        .unwrap_or_else(|| return original.addr());
    twins.insert(copy.addr(), canonical);
    // SAFETY: the caller passes live, compiled op arrays.
    let (originals, copies) = unsafe { (nested(&*original), nested(&*copy)) };
    for (&original, &copy) in originals.iter().zip(copies) {
        // SAFETY: an op array's dynamic function definitions are live, compiled op arrays.
        unsafe { pair(twins, original, copy) };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Root {
    Declaration(FunctionKey),
    Adopted,
}

struct Member {
    group: Group,
    root: Root,
    op_array: usize,
}

unsafe fn root_of(
    op_array: *const zend_op_array,
    names: &mut Names,
    ordinals: &Ordinals,
) -> Option<Root> {
    // SAFETY: the caller passes a live, compiled op array.
    if unsafe { (*op_array).fn_flags } & ZEND_ACC_CLOSURE != 0 {
        return Some(Root::Adopted);
    }
    // SAFETY: forwarded from the caller; a root is never a closure, so it needs no closure rank.
    let declaration = unsafe { snapshot::name(op_array, names, ordinals) }.ok()?;
    return Some(Root::Declaration(declaration.key));
}

impl Ordinals {
    pub(crate) unsafe fn rank(
        op_arrays: &[*mut zend_op_array],
        taken: impl Fn(&Group) -> u32,
    ) -> Self {
        let mut ordinals = Self::default();
        // SAFETY: forwarded from the caller.
        unsafe { ordinals.rank_classes(op_arrays) };
        // SAFETY: forwarded from the caller.
        let twins = unsafe { frameless_twins(op_arrays) };
        let children: BTreeSet<usize> = op_arrays
            .iter()
            // SAFETY: the caller passes live, compiled op arrays.
            .flat_map(|&array| return unsafe { nested(&*array) })
            .map(|child| return child.addr())
            .collect();
        let mut names = Names::default();
        let mut declaration: Option<*mut zend_op_array> = None;
        let mut root: Option<Option<Root>> = None;
        let mut seen = BTreeSet::new();
        let mut members = Vec::new();
        for &op_array in op_arrays {
            if !children.contains(&op_array.addr()) {
                declaration = Some(op_array);
                root = None;
            }
            // SAFETY: the caller passes live, compiled op arrays.
            let array = unsafe { &*op_array };
            if array.fn_flags & ZEND_ACC_CLOSURE == 0
                || twins.contains_key(&op_array.addr())
                || !seen.insert(op_array.addr())
            {
                continue;
            }
            let Some(declaration) = declaration else {
                continue;
            };
            let resolved = root.get_or_insert_with(|| {
                // SAFETY: forwarded from the caller.
                return unsafe { root_of(declaration, &mut names, &ordinals) };
            });
            let Some(root) = resolved.clone() else {
                continue;
            };
            // SAFETY: a compiled op array's file name is a live zend_string.
            let file = unsafe { zend::bytes(array.filename) }.unwrap_or_default();
            members.push(Member {
                group: (file.to_vec(), array.line_start, array.line_end),
                root,
                op_array: op_array.addr(),
            });
        }
        members.sort_by(|left, right| {
            return (&left.group, &left.root).cmp(&(&right.group, &right.root));
        });
        let mut next: Map<Group, u32> = Map::default();
        for member in members {
            let slot = next
                .entry(member.group)
                .or_insert_with_key(|group| return taken(group));
            ordinals.closures.insert(member.op_array, *slot);
            *slot = slot.saturating_add(1);
        }
        for &op_array in op_arrays {
            let canonical = twins
                .get(&op_array.addr())
                .and_then(|original| return ordinals.closures.get(original))
                .copied();
            if let Some(ordinal) = canonical {
                ordinals.closures.insert(op_array.addr(), ordinal);
            }
        }
        return ordinals;
    }

    unsafe fn rank_classes(&mut self, op_arrays: &[*mut zend_op_array]) {
        let mut anonymous: Vec<(u32, usize, Group)> = Vec::new();
        for &op_array in op_arrays {
            // SAFETY: the caller passes live, compiled op arrays.
            let scope = unsafe { (*op_array).scope };
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
            self.classes.insert(class, ordinal);
        }
    }

    pub(crate) fn closure(&self, op_array: *const zend_op_array) -> Option<u32> {
        return self.closures.get(&op_array.addr()).copied();
    }

    pub(crate) fn class(&self, class: *const zend_class_entry) -> Option<u32> {
        return self.classes.get(&class.addr()).copied();
    }
}
