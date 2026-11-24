use core::{fmt, slice};
use std::collections::{BTreeMap, BTreeSet};

use opcov_core::cfg::{
    Analysis, BlockFlags, BlockId, BlockInput, ClassRef, FilePath, FunctionFlags, FunctionInput,
    FunctionKey, KeyError, Line, LineSpan, LoopInput, Op, Opcode, PhpVersion, QualifiedName,
    SnapshotError,
};
use opcov_sys::{
    OPCOV_ENGINE_VERSION_ID, ZEND_ACC_ANON_CLASS, ZEND_ACC_CLOSURE, ZEND_BB_CATCH, ZEND_BB_FINALLY,
    ZEND_BB_FINALLY_END, ZEND_BB_LOOP_HEADER, ZEND_BB_REACHABLE, ZEND_BB_RECV_ENTRY, ZEND_BB_START,
    ZEND_MODULE_API_NO, opcov_arena_create_caught, opcov_arena_destroy, opcov_build_cfg,
    zend_arena, zend_basic_block, zend_cfg, zend_class_entry, zend_op, zend_op_array, zend_string,
};

use crate::{
    engine::Map,
    guard::caught,
    lines::{Instr, LineBlock, OperandType},
    ordinals::Ordinals,
    zend,
};

pub(crate) const PHP: PhpVersion = match PhpVersion::from_version_id(OPCOV_ENGINE_VERSION_ID) {
    Some(version) => version,
    None => panic!("opcov-sys reported an engine version that opcov-core does not model"),
};

const _: () = assert!(
    PHP.module_api() == ZEND_MODULE_API_NO,
    "opcov-core's module API number for this PHP differs from the engine's headers"
);

const ARENA_SIZE: usize = 64 * 1024;
const PROTECTED_ENTRY: u32 =
    ZEND_BB_START | ZEND_BB_RECV_ENTRY | ZEND_BB_CATCH | ZEND_BB_FINALLY | ZEND_BB_FINALLY_END;

#[derive(Debug)]
pub(crate) enum SnapshotFailure {
    Missing {
        what: &'static str,
    },
    Key(KeyError),
    LineZero {
        op: usize,
    },
    Count {
        what: &'static str,
    },
    InvertedSpan {
        start: Line,
        end: Line,
    },
    Unranked {
        what: &'static str,
    },
    Cfg(SnapshotError),
    Bailout,
    #[cfg(feature = "engine-selftest")]
    Injected,
}

impl fmt::Display for SnapshotFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Missing { what } => write!(f, "the {what} is missing"),
            Self::Key(error) => write!(f, "cannot name the function: {error}"),
            Self::LineZero { op } => write!(f, "opcode {op} has line 0"),
            Self::Count { what } => write!(f, "the engine reported an impossible {what}"),
            Self::InvertedSpan { start, end } => {
                write!(
                    f,
                    "the declaration ends on line {end} before it starts on line {start}"
                )
            }
            Self::Unranked { what } => write!(
                f,
                "the {what} was not ranked among the declarations sharing its line span"
            ),
            Self::Cfg(error) => write!(f, "{error}"),
            Self::Bailout => write!(f, "a fatal error interrupted building the CFG"),
            #[cfg(feature = "engine-selftest")]
            Self::Injected => write!(f, "failure injected by opcov\\selftest_fail_snapshot()"),
        };
    }
}

#[derive(Debug)]
pub(crate) struct Snapshot {
    pub(crate) analysis: Analysis,
    pub(crate) instrs: Vec<Instr>,
    pub(crate) blocks: Vec<LineBlock>,
}

struct RawBlock {
    flags: u32,
    start: usize,
    len: usize,
    successors: Vec<usize>,
    loop_header: Option<usize>,
}

fn count(value: impl TryInto<usize>, what: &'static str) -> Result<usize, SnapshotFailure> {
    return value
        .try_into()
        .map_err(|_overflow| return SnapshotFailure::Count { what });
}

fn line_span(start: u32, end: u32) -> Result<LineSpan, SnapshotFailure> {
    let (start, end) = (line(start, 0)?, line(end, 0)?);
    return LineSpan::new(start, end).ok_or(SnapshotFailure::InvertedSpan { start, end });
}

#[derive(Debug, Default)]
pub(crate) struct Names {
    file: Option<(usize, FilePath)>,
    classes: Map<usize, ClassRef>,
}

impl Names {
    unsafe fn file(&mut self, name: *const zend_string) -> Result<FilePath, SnapshotFailure> {
        if let Some((cached, file)) = &self.file
            && *cached == name.addr()
        {
            return Ok(file.clone());
        }
        // SAFETY: the caller passes a compiled op array's file name, a live zend_string.
        let file = FilePath::from_bytes(
            unsafe { zend::bytes(name) }.ok_or(SnapshotFailure::Missing { what: "file name" })?,
        )
        .map_err(SnapshotFailure::Key)?;
        self.file = Some((name.addr(), file.clone()));
        return Ok(file);
    }

    unsafe fn class(
        &mut self,
        scope: *const zend_class_entry,
        ordinals: &Ordinals,
    ) -> Result<ClassRef, SnapshotFailure> {
        if let Some(class) = self.classes.get(&scope.addr()) {
            return Ok(class.clone());
        }
        // SAFETY: forwarded from the caller.
        let class = unsafe { class_ref(scope, ordinals)? };
        self.classes.insert(scope.addr(), class.clone());
        return Ok(class);
    }
}

unsafe fn class_ref(
    scope: *const zend_class_entry,
    ordinals: &Ordinals,
) -> Result<ClassRef, SnapshotFailure> {
    // SAFETY: the caller passes a compiled op array's scope, a live class entry.
    let class = unsafe { &*scope };
    if class.ce_flags & ZEND_ACC_ANON_CLASS == 0 {
        // SAFETY: a class entry's name is a live zend_string.
        let name = unsafe { zend::bytes(class.name) }
            .ok_or(SnapshotFailure::Missing { what: "class name" })?;
        return Ok(ClassRef::Named(
            QualifiedName::from_bytes(name).map_err(SnapshotFailure::Key)?,
        ));
    }
    // SAFETY: an anonymous class is a user class, whose `info.user` arm is the active one.
    let user = unsafe { class.info.user };
    // SAFETY: a user class's file name is a live zend_string.
    let file = unsafe { zend::bytes(user.filename) }.ok_or(SnapshotFailure::Missing {
        what: "class file name",
    })?;
    return Ok(ClassRef::Anonymous {
        file: FilePath::from_bytes(file).map_err(SnapshotFailure::Key)?,
        lines: line_span(user.line_start, user.line_end)?,
        ordinal: ordinals.class(scope).ok_or(SnapshotFailure::Unranked {
            what: "anonymous class",
        })?,
    });
}

#[derive(Debug)]
pub(crate) struct Named {
    pub(crate) file: FilePath,
    pub(crate) key: FunctionKey,
}

pub(crate) unsafe fn name(
    op_array: *const zend_op_array,
    names: &mut Names,
    ordinals: &Ordinals,
) -> Result<Named, SnapshotFailure> {
    // SAFETY: the caller passes a live, compiled op array.
    let array = unsafe { &*op_array };
    // SAFETY: a compiled op array's file name is a live zend_string.
    let file = unsafe { names.file(array.filename)? };
    let key = if array.function_name.is_null() {
        FunctionKey::Main { file: file.clone() }
    } else if array.fn_flags & ZEND_ACC_CLOSURE != 0 {
        FunctionKey::Closure {
            file: file.clone(),
            lines: line_span(array.line_start, array.line_end)?,
            ordinal: ordinals
                .closure(op_array)
                .ok_or(SnapshotFailure::Unranked { what: "closure" })?,
        }
    } else {
        // SAFETY: checked non-null above; a compiled op array's function name is a live
        // zend_string.
        let name = unsafe { zend::bytes(array.function_name) }.ok_or(SnapshotFailure::Missing {
            what: "function name",
        })?;
        if array.scope.is_null() {
            FunctionKey::Function {
                name: QualifiedName::from_bytes(name).map_err(SnapshotFailure::Key)?,
            }
        } else {
            // SAFETY: a compiled op array's scope is a live class entry.
            let class = unsafe { names.class(array.scope, ordinals)? };
            FunctionKey::member(class, name).map_err(SnapshotFailure::Key)?
        }
    };
    return Ok(Named { file, key });
}

fn line(raw: u32, op: usize) -> Result<Line, SnapshotFailure> {
    return Line::new(raw).ok_or(SnapshotFailure::LineZero { op });
}

fn instrs(opcodes: &[zend_op]) -> Result<Vec<Instr>, SnapshotFailure> {
    return opcodes
        .iter()
        .enumerate()
        .map(|(op, raw)| {
            return Ok(Instr {
                opcode: Opcode::decode(PHP, raw.opcode),
                op1: OperandType::new(raw.op1_type),
                op2: OperandType::new(raw.op2_type),
                result: OperandType::new(raw.result_type),
                line: line(raw.lineno, op)?,
            });
        })
        .collect();
}

unsafe fn raw_block(block: &zend_basic_block) -> Result<RawBlock, SnapshotFailure> {
    let successor_count = count(block.successors_count, "successor count")?;
    let successors = if successor_count == 0 {
        &[][..]
    } else {
        // SAFETY: zend_build_cfg points `successors` at `successors_count` block indexes in the
        // block itself or in the arena, which is alive while the caller copies them.
        unsafe { slice::from_raw_parts(block.successors, successor_count) }
    };
    return Ok(RawBlock {
        flags: block.flags,
        start: count(block.start, "block start")?,
        len: count(block.len, "block length")?,
        successors: successors
            .iter()
            .map(|&successor| return count(successor, "successor"))
            .collect::<Result<_, _>>()?,
        loop_header: usize::try_from(block.loop_header).ok(),
    });
}

unsafe fn raw_cfg(op_array: *const zend_op_array) -> Result<Vec<RawBlock>, SnapshotFailure> {
    let mut arena: *mut zend_arena = core::ptr::null_mut();
    // SAFETY: allocates a fresh arena; it is destroyed below before this function returns. A fatal
    // error on the way is caught inside the call.
    caught(unsafe { opcov_arena_create_caught(ARENA_SIZE, &raw mut arena) })
        .map_err(|_bailout| return SnapshotFailure::Bailout)?;
    let mut cfg = zend_cfg::default();
    // SAFETY: `op_array` is a live, compiled op array; the CFG is allocated in `arena`, which
    // outlives every read of `cfg` below. A fatal error on the way is caught inside the call.
    let built = caught(unsafe { opcov_build_cfg(&raw mut arena, op_array, &raw mut cfg) });
    if built.is_err() {
        // SAFETY: the arena was created above and nothing refers to it any more.
        unsafe { opcov_arena_destroy(arena) };
        return Err(SnapshotFailure::Bailout);
    }
    let blocks = count(cfg.blocks_count, "block count").and_then(|block_count| {
        if block_count == 0 {
            return Ok(Vec::new());
        }
        // SAFETY: zend_build_cfg allocated `blocks_count` blocks in the arena, which is alive.
        let blocks = unsafe { slice::from_raw_parts(cfg.blocks, block_count) };
        return blocks
            .iter()
            // SAFETY: each block belongs to the live CFG.
            .map(|block| return unsafe { raw_block(block) })
            .collect();
    });
    // SAFETY: the arena (and every segment chained onto it) was created above and nothing refers
    // to it any more.
    unsafe { opcov_arena_destroy(arena) };
    return blocks;
}

fn loops(raw: &[RawBlock]) -> Vec<LoopInput> {
    let mut bodies: BTreeMap<usize, BTreeSet<BlockId>> = raw
        .iter()
        .enumerate()
        .filter(|(_, block)| return block.flags & ZEND_BB_LOOP_HEADER != 0)
        .map(|(index, _)| return (index, BTreeSet::new()))
        .collect();
    for (index, block) in raw.iter().enumerate() {
        let Ok(member) = u32::try_from(index).map(BlockId::new) else {
            continue;
        };
        let mut header = block.loop_header;
        for _ in 0..raw.len() {
            let Some(current) = header else {
                break;
            };
            if let Some(body) = bodies.get_mut(&current) {
                body.insert(member);
            }
            header = raw.get(current).and_then(|outer| return outer.loop_header);
        }
    }
    return bodies
        .into_iter()
        .filter_map(|(header, body_blocks)| {
            return u32::try_from(header).ok().map(|id| {
                return LoopInput {
                    header: BlockId::new(id),
                    body_blocks,
                };
            });
        })
        .collect();
}

fn block_input(raw: &RawBlock, instrs: &[Instr]) -> Result<BlockInput, SnapshotFailure> {
    let end = raw
        .start
        .checked_add(raw.len)
        .ok_or(SnapshotFailure::Count {
            what: "block range",
        })?;
    let ops = instrs
        .get(raw.start..end)
        .ok_or(SnapshotFailure::Count {
            what: "block range",
        })?
        .iter()
        .map(|instr| {
            return Op {
                opcode: instr.opcode,
                line: instr.line,
            };
        })
        .collect();
    let successors = raw
        .successors
        .iter()
        .map(|&successor| {
            return u32::try_from(successor)
                .map(BlockId::new)
                .map_err(|_overflow| return SnapshotFailure::Count { what: "successor" });
        })
        .collect::<Result<_, _>>()?;
    return Ok(BlockInput {
        flags: BlockFlags::from_zend_block_flags(raw.flags),
        ops,
        successors,
    });
}

fn line_blocks(raw: &[RawBlock]) -> Vec<LineBlock> {
    let mut previous: Option<&RawBlock> = None;
    return raw
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let falls_from_previous =
                previous.is_some_and(|before| return before.successors.contains(&index));
            previous = Some(block);
            return LineBlock {
                reachable: block.flags & ZEND_BB_REACHABLE != 0,
                protected_entry: block.flags & PROTECTED_ENTRY != 0,
                falls_from_previous,
                ops: block.start..block.start.saturating_add(block.len),
            };
        })
        .collect();
}

pub(crate) unsafe fn take(
    op_array: *const zend_op_array,
    named: Named,
) -> Result<Snapshot, SnapshotFailure> {
    // SAFETY: the caller passes a live, compiled op array.
    let array = unsafe { &*op_array };
    let op_count = count(array.last, "opcode count")?;
    let opcodes = if op_count == 0 {
        &[][..]
    } else {
        // SAFETY: a compiled op array owns `last` opcodes at `opcodes`.
        unsafe { slice::from_raw_parts(array.opcodes, op_count) }
    };
    let instrs = instrs(opcodes)?;
    // SAFETY: forwarded from the caller.
    let raw = unsafe { raw_cfg(op_array)? };
    let input = FunctionInput {
        key: named.key,
        file: named.file,
        line_start: line(array.line_start, 0)?,
        line_end: line(array.line_end, 0)?,
        flags: FunctionFlags::from_zend_fn_flags(array.fn_flags),
        blocks: raw
            .iter()
            .map(|block| return block_input(block, &instrs))
            .collect::<Result<_, _>>()?,
        loops: loops(&raw),
    };
    let analysis = Analysis::try_from(input).map_err(SnapshotFailure::Cfg)?;
    return Ok(Snapshot {
        analysis,
        blocks: line_blocks(&raw),
        instrs,
    });
}
