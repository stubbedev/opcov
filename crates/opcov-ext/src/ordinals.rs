use std::collections::HashMap;

use opcov_sys::{ZEND_ACC_ANON_CLASS, ZEND_ACC_CLOSURE, zend_class_entry, zend_op_array};

use crate::zend;

type Group = (Vec<u8>, u32, u32);

#[derive(Debug, Default)]
pub(crate) struct Ordinals {
    closures: HashMap<usize, u32>,
    classes: HashMap<usize, u32>,
}

fn assign(next: &mut HashMap<Group, u32>, group: Group) -> u32 {
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

impl Ordinals {
    pub(crate) unsafe fn rank(op_arrays: &[*mut zend_op_array]) -> Self {
        let mut ordinals = Self::default();
        let mut next_closure: HashMap<Group, u32> = HashMap::new();
        let mut anonymous: Vec<(u32, usize, Group)> = Vec::new();
        for &op_array in op_arrays {
            // SAFETY: the caller passes live, compiled op arrays.
            let array = unsafe { &*op_array };
            // SAFETY: a compiled op array's file name is a live zend_string.
            let file = unsafe { zend::bytes(array.filename) }.unwrap_or_default();
            if array.fn_flags & ZEND_ACC_CLOSURE != 0
                && !ordinals.closures.contains_key(&op_array.addr())
            {
                let group = (file.to_vec(), array.line_start, array.line_end);
                let ordinal = assign(&mut next_closure, group);
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
        let mut next_class: HashMap<Group, u32> = HashMap::new();
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
