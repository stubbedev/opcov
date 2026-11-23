use core::{fmt, slice};
use std::collections::{BTreeMap, BTreeSet};

use opcov_core::cfg::{
    Analysis, BlockFlags, BlockId, BlockInput, ClassRef, FilePath, FunctionFlags, FunctionInput,
    FunctionKey, KeyError, Line, LineSpan, LoopInput, Op, Opcode, PhpVersion, SnapshotError,
};
use opcov_sys::{
    PHP_VERSION_ID, ZEND_ACC_ANON_CLASS, ZEND_ACC_CLOSURE, ZEND_BB_CATCH, ZEND_BB_FINALLY,
    ZEND_BB_FINALLY_END, ZEND_BB_LOOP_HEADER, ZEND_BB_REACHABLE, ZEND_BB_RECV_ENTRY, ZEND_BB_START,
    opcov_arena_create, opcov_arena_destroy, zend_basic_block, zend_build_cfg, zend_cfg,
    zend_cfg_build_predecessors, zend_cfg_compute_dominators_tree, zend_cfg_identify_loops,
    zend_op, zend_op_array,
};

use crate::{
    lines::{Instr, LineBlock, OperandType},
    ordinals::Ordinals,
    zend,
};

pub(crate) const PHP: PhpVersion = match PHP_VERSION_ID / 100 {
    804 => PhpVersion::Php84,
    805 => PhpVersion::Php85,
    806 => PhpVersion::Php86,
    _ => panic!("opcov-sys only builds against PHP 8.4, 8.5 and 8.6"),
};

const ARENA_SIZE: usize = 64 * 1024;
const PROTECTED_ENTRY: u32 =
    ZEND_BB_START | ZEND_BB_RECV_ENTRY | ZEND_BB_CATCH | ZEND_BB_FINALLY | ZEND_BB_FINALLY_END;

#[derive(Debug)]
pub(crate) enum SnapshotFailure {
    NotUtf8 { what: &'static str },
    Key(KeyError),
    LineZero { op: usize },
    Count { what: &'static str },
    InvertedSpan { start: Line, end: Line },
    Unranked { what: &'static str },
    Cfg(SnapshotError),
}

impl fmt::Display for SnapshotFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::NotUtf8 { what } => write!(f, "the {what} is missing or not UTF-8"),
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

unsafe fn class_ref(
    op_array: &zend_op_array,
    ordinals: &Ordinals,
) -> Result<Option<ClassRef>, SnapshotFailure> {
    let scope = op_array.scope;
    if scope.is_null() {
        return Ok(None);
    }
    // SAFETY: a compiled op array's scope is a live class entry.
    let class = unsafe { &*scope };
    if class.ce_flags & ZEND_ACC_ANON_CLASS == 0 {
        // SAFETY: a class entry's name is a live zend_string.
        let name = unsafe { zend::text(class.name) }
            .ok_or(SnapshotFailure::NotUtf8 { what: "class name" })?;
        return Ok(Some(ClassRef::Named(
            name.parse().map_err(SnapshotFailure::Key)?,
        )));
    }
    // SAFETY: an anonymous class is a user class, whose `info.user` arm is the active one.
    let user = unsafe { class.info.user };
    // SAFETY: a user class's file name is a live zend_string.
    let file = unsafe { zend::text(user.filename) }.ok_or(SnapshotFailure::NotUtf8 {
        what: "class file name",
    })?;
    return Ok(Some(ClassRef::Anonymous {
        file: file.parse().map_err(SnapshotFailure::Key)?,
        lines: line_span(user.line_start, user.line_end)?,
        ordinal: ordinals.class(scope).ok_or(SnapshotFailure::Unranked {
            what: "anonymous class",
        })?,
    }));
}

unsafe fn function_key(
    op_array: &zend_op_array,
    file: &FilePath,
    ordinals: &Ordinals,
) -> Result<FunctionKey, SnapshotFailure> {
    if op_array.function_name.is_null() {
        return Ok(FunctionKey::Main { file: file.clone() });
    }
    if op_array.fn_flags & ZEND_ACC_CLOSURE != 0 {
        return Ok(FunctionKey::Closure {
            file: file.clone(),
            lines: line_span(op_array.line_start, op_array.line_end)?,
            ordinal: ordinals
                .closure(op_array)
                .ok_or(SnapshotFailure::Unranked { what: "closure" })?,
        });
    }
    // SAFETY: checked non-null above; a compiled op array's function name is a live zend_string.
    let name = unsafe { zend::text(op_array.function_name) }.ok_or(SnapshotFailure::NotUtf8 {
        what: "function name",
    })?;
    // SAFETY: forwarded from the caller.
    let text = unsafe { class_ref(op_array, ordinals)? }.map_or_else(
        || return name.to_owned(),
        |class| return format!("{class}::{name}"),
    );
    return text.parse().map_err(SnapshotFailure::Key);
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
    // SAFETY: allocates a fresh arena; it is destroyed below before this function returns.
    let mut arena = unsafe { opcov_arena_create(ARENA_SIZE) };
    let mut cfg = zend_cfg::default();
    // SAFETY: `op_array` is a live, compiled op array; the CFG is allocated in `arena`, which
    // outlives every read of `cfg` below.
    unsafe {
        zend_build_cfg(&raw mut arena, op_array, 0, &raw mut cfg);
        zend_cfg_build_predecessors(&raw mut arena, &raw mut cfg);
        zend_cfg_compute_dominators_tree(op_array, &raw mut cfg);
        zend_cfg_identify_loops(op_array, &raw mut cfg);
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
    ordinals: &Ordinals,
) -> Result<Snapshot, SnapshotFailure> {
    // SAFETY: the caller passes a live, compiled op array.
    let array = unsafe { &*op_array };
    // SAFETY: a compiled op array's file name is a live zend_string.
    let file: FilePath = unsafe { zend::text(array.filename) }
        .ok_or(SnapshotFailure::NotUtf8 { what: "file name" })?
        .parse()
        .map_err(SnapshotFailure::Key)?;
    // SAFETY: forwarded from the caller.
    let key = unsafe { function_key(array, &file, ordinals)? };
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
        key,
        file,
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
