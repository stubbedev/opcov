use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::classify::{Diagnostic, Node, classify};
use super::key::{FilePath, FunctionKey};
use super::opcode::Opcode;
use crate::kind::EdgeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(u32);

impl BlockId {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        return Self(index);
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0;
    }

    pub(super) fn index(self) -> Option<usize> {
        return usize::try_from(self.0).ok();
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "BB{}", self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpIndex(u32);

impl OpIndex {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        return Self(index);
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0;
    }
}

impl fmt::Display for OpIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{:04}", self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Line(NonZeroU32);

impl Line {
    #[must_use]
    pub fn new(number: u32) -> Option<Self> {
        return NonZeroU32::new(number).map(Self);
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0.get();
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{}", self.0);
    }
}

pub trait Flag: Copy + Eq + fmt::Debug + Serialize + for<'de> Deserialize<'de> + 'static {
    const ALL: &'static [Self];

    fn bit(self) -> u8;
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FlagSet<F> {
    bits: u8,
    flag: PhantomData<F>,
}

impl<F: Flag> FlagSet<F> {
    #[must_use]
    pub const fn empty() -> Self {
        return Self {
            bits: 0,
            flag: PhantomData,
        };
    }

    #[must_use]
    pub fn with(self, flag: F) -> Self {
        return Self {
            bits: self.bits | flag.bit(),
            flag: PhantomData,
        };
    }

    #[must_use]
    pub fn contains(self, flag: F) -> bool {
        return self.bits & flag.bit() != 0;
    }

    pub fn iter(self) -> impl Iterator<Item = F> {
        return F::ALL
            .iter()
            .copied()
            .filter(move |&flag| return self.contains(flag));
    }

    fn from_masks(raw: u32, masks: &[(u32, F)]) -> Self {
        return masks
            .iter()
            .filter(|&&(mask, _)| return raw & mask == mask)
            .map(|&(_, flag)| return flag)
            .collect();
    }
}

impl<F: Flag> Default for FlagSet<F> {
    fn default() -> Self {
        return Self::empty();
    }
}

impl<F: Flag> fmt::Debug for FlagSet<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.debug_set().entries(self.iter()).finish();
    }
}

impl<F: Flag> FromIterator<F> for FlagSet<F> {
    fn from_iter<I: IntoIterator<Item = F>>(flags: I) -> Self {
        return flags.into_iter().fold(Self::empty(), Self::with);
    }
}

impl<F: Flag> Serialize for FlagSet<F> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.collect_seq(self.iter());
    }
}

impl<'de, F: Flag> Deserialize<'de> for FlagSet<F> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let flags = Vec::<F>::deserialize(deserializer)?;
        let set: Self = flags.iter().copied().collect();
        if !set.iter().eq(flags.iter().copied()) {
            return Err(de::Error::custom(format!(
                "flags {flags:?} must be unique and listed in the order {:?}",
                F::ALL
            )));
        }
        return Ok(set);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BlockFlag {
    Start,
    Catch,
    Finally,
    Reachable,
}

impl Flag for BlockFlag {
    const ALL: &'static [Self] = &[Self::Start, Self::Catch, Self::Finally, Self::Reachable];

    fn bit(self) -> u8 {
        return match self {
            Self::Start => 1,
            Self::Catch => 2,
            Self::Finally => 4,
            Self::Reachable => 8,
        };
    }
}

pub type BlockFlags = FlagSet<BlockFlag>;

const ZEND_BB_START: u32 = 1;
const ZEND_BB_CATCH: u32 = 1 << 6;
const ZEND_BB_FINALLY: u32 = 1 << 7;
const ZEND_BB_REACHABLE: u32 = 1 << 31;

impl FlagSet<BlockFlag> {
    #[must_use]
    pub fn from_zend_block_flags(raw: u32) -> Self {
        return Self::from_masks(
            raw,
            &[
                (ZEND_BB_START, BlockFlag::Start),
                (ZEND_BB_CATCH, BlockFlag::Catch),
                (ZEND_BB_FINALLY, BlockFlag::Finally),
                (ZEND_BB_REACHABLE, BlockFlag::Reachable),
            ],
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FunctionFlag {
    Generator,
    Abstract,
    StaticClosure,
    ReturnsReference,
}

impl Flag for FunctionFlag {
    const ALL: &'static [Self] = &[
        Self::Generator,
        Self::Abstract,
        Self::StaticClosure,
        Self::ReturnsReference,
    ];

    fn bit(self) -> u8 {
        return match self {
            Self::Generator => 1,
            Self::Abstract => 2,
            Self::StaticClosure => 4,
            Self::ReturnsReference => 8,
        };
    }
}

pub type FunctionFlags = FlagSet<FunctionFlag>;

const ZEND_ACC_STATIC: u32 = 1 << 4;
const ZEND_ACC_ABSTRACT: u32 = 1 << 6;
const ZEND_ACC_RETURN_REFERENCE: u32 = 1 << 12;
const ZEND_ACC_CLOSURE: u32 = 1 << 22;
const ZEND_ACC_GENERATOR: u32 = 1 << 24;

impl FlagSet<FunctionFlag> {
    #[must_use]
    pub fn from_zend_fn_flags(raw: u32) -> Self {
        return Self::from_masks(
            raw,
            &[
                (ZEND_ACC_GENERATOR, FunctionFlag::Generator),
                (ZEND_ACC_ABSTRACT, FunctionFlag::Abstract),
                (
                    ZEND_ACC_STATIC | ZEND_ACC_CLOSURE,
                    FunctionFlag::StaticClosure,
                ),
                (ZEND_ACC_RETURN_REFERENCE, FunctionFlag::ReturnsReference),
            ],
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Op {
    pub opcode: Opcode,
    pub line: Line,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockInput {
    pub flags: BlockFlags,
    pub ops: Vec<Op>,
    pub successors: Vec<BlockId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopInput {
    pub header: BlockId,
    pub body_blocks: BTreeSet<BlockId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionInput {
    pub key: FunctionKey,
    pub file: FilePath,
    pub line_start: Line,
    pub line_end: Line,
    pub flags: FunctionFlags,
    pub blocks: Vec<BlockInput>,
    pub loops: Vec<LoopInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfgError {
    NoBlocks,
    TooManyBlocks {
        count: usize,
    },
    EmptyBlock {
        block: BlockId,
    },
    OpIndexOverflow {
        block: BlockId,
    },
    BlockIdOutOfOrder {
        expected: BlockId,
        found: BlockId,
    },
    OpRangeGap {
        block: BlockId,
        expected: OpIndex,
        found: OpIndex,
    },
    OpRangeMismatch {
        block: BlockId,
        op_start: OpIndex,
        op_end: OpIndex,
        op_count: usize,
    },
    LineRangeMismatch {
        block: BlockId,
        line_start: Line,
        line_end: Line,
    },
    StartFlagMissing,
    StartFlagMisplaced {
        block: BlockId,
    },
    InvertedFunctionLines {
        line_start: Line,
        line_end: Line,
    },
    EdgeEndpointMissing {
        from: BlockId,
        to: BlockId,
    },
    EdgeFromUnreachable {
        from: BlockId,
        to: BlockId,
    },
    DuplicateEdge {
        from: BlockId,
        to: BlockId,
        kind: EdgeKind,
    },
    EdgeOutOfOrder {
        from: BlockId,
        to: BlockId,
        kind: EdgeKind,
    },
    LoopBlockMissing {
        header: BlockId,
        block: BlockId,
    },
    LoopHeaderInBody {
        header: BlockId,
    },
    DuplicateLoopHeader {
        header: BlockId,
    },
    LoopOutOfOrder {
        header: BlockId,
    },
}

impl fmt::Display for CfgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match *self {
            Self::NoBlocks => write!(f, "the function has no basic blocks"),
            Self::TooManyBlocks { count } => {
                write!(f, "{count} basic blocks exceed the u32 block id range")
            }
            Self::EmptyBlock { block } => write!(f, "block {block} has no opcodes"),
            Self::OpIndexOverflow { block } => write!(
                f,
                "the opcode indexes of block {block} exceed the u32 range"
            ),
            Self::BlockIdOutOfOrder { expected, found } => {
                write!(f, "block {found} is listed where block {expected} belongs")
            }
            Self::OpRangeGap {
                block,
                expected,
                found,
            } => {
                write!(
                    f,
                    "block {block} starts at opcode {found} instead of {expected}"
                )
            }
            Self::OpRangeMismatch {
                block,
                op_start,
                op_end,
                op_count,
            } => write!(
                f,
                "block {block} spans opcodes {op_start}..={op_end} but lists {op_count} opcodes"
            ),
            Self::LineRangeMismatch {
                block,
                line_start,
                line_end,
            } => write!(
                f,
                "block {block} declares lines {line_start}-{line_end}, which differ from its opcodes' lines"
            ),
            Self::StartFlagMissing => write!(f, "the entry block BB0 lacks the Start flag"),
            Self::StartFlagMisplaced { block } => {
                write!(f, "block {block} has the Start flag but is not BB0")
            }
            Self::InvertedFunctionLines {
                line_start,
                line_end,
            } => {
                write!(
                    f,
                    "the function ends on line {line_end} before it starts on line {line_start}"
                )
            }
            Self::EdgeEndpointMissing { from, to } => {
                write!(f, "edge {from} -> {to} references a missing block")
            }
            Self::EdgeFromUnreachable { from, to } => {
                write!(
                    f,
                    "edge {from} -> {to} leaves a block the engine marked unreachable"
                )
            }
            Self::DuplicateEdge { from, to, kind } => {
                write!(f, "edge {from} -> {to} ({kind:?}) is listed twice")
            }
            Self::EdgeOutOfOrder { from, to, kind } => {
                write!(
                    f,
                    "edge {from} -> {to} ({kind:?}) is out of (from, to, kind) order"
                )
            }
            Self::LoopBlockMissing { header, block } => {
                write!(
                    f,
                    "the loop headed by {header} references missing block {block}"
                )
            }
            Self::LoopHeaderInBody { header } => {
                write!(f, "the loop header {header} is listed in its own body")
            }
            Self::DuplicateLoopHeader { header } => {
                write!(f, "two loops share the header {header}")
            }
            Self::LoopOutOfOrder { header } => {
                write!(f, "the loop headed by {header} is out of header order")
            }
        };
    }
}

impl std::error::Error for CfgError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError {
    key: FunctionKey,
    error: CfgError,
}

impl SnapshotError {
    #[must_use]
    pub const fn key(&self) -> &FunctionKey {
        return &self.key;
    }

    #[must_use]
    pub const fn error(&self) -> CfgError {
        return self.error;
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            f,
            "cannot build the CFG snapshot of {}: {}",
            self.key, self.error
        );
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        return Some(&self.error);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BlockRepr")]
pub struct Block {
    id: BlockId,
    op_start: OpIndex,
    op_end: OpIndex,
    line_start: Line,
    line_end: Line,
    flags: BlockFlags,
    ops: Vec<Op>,
}

#[derive(Deserialize)]
struct BlockRepr {
    id: BlockId,
    op_start: OpIndex,
    op_end: OpIndex,
    line_start: Line,
    line_end: Line,
    flags: BlockFlags,
    ops: Vec<Op>,
}

impl Block {
    fn new(
        id: BlockId,
        op_start: OpIndex,
        flags: BlockFlags,
        ops: Vec<Op>,
    ) -> Result<Self, CfgError> {
        let Some((first, rest)) = ops.split_first() else {
            return Err(CfgError::EmptyBlock { block: id });
        };
        let (line_start, line_end) =
            rest.iter()
                .fold((first.line, first.line), |(low, high), op| {
                    return (low.min(op.line), high.max(op.line));
                });
        let op_end = u32::try_from(rest.len())
            .ok()
            .and_then(|extra| return op_start.0.checked_add(extra))
            .map(OpIndex)
            .ok_or(CfgError::OpIndexOverflow { block: id })?;
        return Ok(Self {
            id,
            op_start,
            op_end,
            line_start,
            line_end,
            flags,
            ops,
        });
    }

    #[must_use]
    pub const fn id(&self) -> BlockId {
        return self.id;
    }

    #[must_use]
    pub const fn op_start(&self) -> OpIndex {
        return self.op_start;
    }

    #[must_use]
    pub const fn op_end(&self) -> OpIndex {
        return self.op_end;
    }

    #[must_use]
    pub const fn line_start(&self) -> Line {
        return self.line_start;
    }

    #[must_use]
    pub const fn line_end(&self) -> Line {
        return self.line_end;
    }

    #[must_use]
    pub const fn flags(&self) -> BlockFlags {
        return self.flags;
    }

    #[must_use]
    pub fn ops(&self) -> &[Op] {
        return &self.ops;
    }

    #[must_use]
    pub fn terminator(&self) -> Option<Opcode> {
        return self.ops.last().map(|op| return op.opcode);
    }

    #[must_use]
    pub fn is_reachable(&self) -> bool {
        return self.flags.contains(BlockFlag::Reachable);
    }

    fn next_op(&self) -> Result<OpIndex, CfgError> {
        return self
            .op_end
            .0
            .checked_add(1)
            .map(OpIndex)
            .ok_or(CfgError::OpIndexOverflow { block: self.id });
    }
}

impl TryFrom<BlockRepr> for Block {
    type Error = CfgError;

    fn try_from(repr: BlockRepr) -> Result<Self, Self::Error> {
        let op_count = repr.ops.len();
        let block = Self::new(repr.id, repr.op_start, repr.flags, repr.ops)?;
        if block.op_end != repr.op_end {
            return Err(CfgError::OpRangeMismatch {
                block: repr.id,
                op_start: repr.op_start,
                op_end: repr.op_end,
                op_count,
            });
        }
        if (block.line_start, block.line_end) != (repr.line_start, repr.line_end) {
            return Err(CfgError::LineRangeMismatch {
                block: repr.id,
                line_start: repr.line_start,
                line_end: repr.line_end,
            });
        }
        return Ok(block);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub from: BlockId,
    pub to: BlockId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LoopShape {
    Foreach,
    PreTest,
    PostTest,
    Unstructured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "LoopRepr")]
pub struct Loop {
    header: BlockId,
    body_blocks: BTreeSet<BlockId>,
    kind: LoopShape,
}

#[derive(Deserialize)]
struct LoopRepr {
    header: BlockId,
    body_blocks: BTreeSet<BlockId>,
    kind: LoopShape,
}

impl Loop {
    pub(super) fn new(
        header: BlockId,
        body_blocks: BTreeSet<BlockId>,
        kind: LoopShape,
    ) -> Result<Self, CfgError> {
        if body_blocks.contains(&header) {
            return Err(CfgError::LoopHeaderInBody { header });
        }
        return Ok(Self {
            header,
            body_blocks,
            kind,
        });
    }

    #[must_use]
    pub const fn header(&self) -> BlockId {
        return self.header;
    }

    #[must_use]
    pub const fn body_blocks(&self) -> &BTreeSet<BlockId> {
        return &self.body_blocks;
    }

    #[must_use]
    pub const fn kind(&self) -> LoopShape {
        return self.kind;
    }

    pub(super) fn members(&self) -> impl Iterator<Item = BlockId> {
        return std::iter::once(self.header).chain(self.body_blocks.iter().copied());
    }
}

impl TryFrom<LoopRepr> for Loop {
    type Error = CfgError;

    fn try_from(repr: LoopRepr) -> Result<Self, Self::Error> {
        return Self::new(repr.header, repr.body_blocks, repr.kind);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FunctionRepr")]
pub struct Function {
    key: FunctionKey,
    file: FilePath,
    line_start: Line,
    line_end: Line,
    flags: FunctionFlags,
    blocks: Vec<Block>,
    edges: Vec<Edge>,
    loops: Vec<Loop>,
}

#[derive(Deserialize)]
struct FunctionRepr {
    key: FunctionKey,
    file: FilePath,
    line_start: Line,
    line_end: Line,
    flags: FunctionFlags,
    blocks: Vec<Block>,
    edges: Vec<Edge>,
    loops: Vec<Loop>,
}

impl Function {
    #[must_use]
    pub const fn key(&self) -> &FunctionKey {
        return &self.key;
    }

    #[must_use]
    pub const fn file(&self) -> &FilePath {
        return &self.file;
    }

    #[must_use]
    pub const fn line_start(&self) -> Line {
        return self.line_start;
    }

    #[must_use]
    pub const fn line_end(&self) -> Line {
        return self.line_end;
    }

    #[must_use]
    pub const fn flags(&self) -> FunctionFlags {
        return self.flags;
    }

    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        return &self.blocks;
    }

    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        return &self.edges;
    }

    #[must_use]
    pub fn loops(&self) -> &[Loop] {
        return &self.loops;
    }

    #[must_use]
    pub fn block(&self, id: BlockId) -> Option<&Block> {
        return id.index().and_then(|index| return self.blocks.get(index));
    }
}

fn validate_blocks(blocks: &[Block]) -> Result<(), CfgError> {
    if blocks.is_empty() {
        return Err(CfgError::NoBlocks);
    }
    let mut next_op = OpIndex(0);
    for (index, block) in blocks.iter().enumerate() {
        let expected = u32::try_from(index)
            .map(BlockId)
            .ok()
            .ok_or(CfgError::TooManyBlocks {
                count: blocks.len(),
            })?;
        if block.id != expected {
            return Err(CfgError::BlockIdOutOfOrder {
                expected,
                found: block.id,
            });
        }
        if block.op_start != next_op {
            return Err(CfgError::OpRangeGap {
                block: block.id,
                expected: next_op,
                found: block.op_start,
            });
        }
        let is_start = block.flags.contains(BlockFlag::Start);
        if index == 0 && !is_start {
            return Err(CfgError::StartFlagMissing);
        }
        if index != 0 && is_start {
            return Err(CfgError::StartFlagMisplaced { block: block.id });
        }
        next_op = block.next_op()?;
    }
    return Ok(());
}

fn validate_edges(blocks: &[Block], edges: &[Edge]) -> Result<(), CfgError> {
    let lookup = |id: BlockId| return id.index().and_then(|index| return blocks.get(index));
    let mut previous: Option<&Edge> = None;
    for edge in edges {
        let (Some(from), Some(_)) = (lookup(edge.from), lookup(edge.to)) else {
            return Err(CfgError::EdgeEndpointMissing {
                from: edge.from,
                to: edge.to,
            });
        };
        if !from.is_reachable() {
            return Err(CfgError::EdgeFromUnreachable {
                from: edge.from,
                to: edge.to,
            });
        }
        match previous.map(|before| return before.cmp(edge)) {
            Some(Ordering::Equal) => {
                return Err(CfgError::DuplicateEdge {
                    from: edge.from,
                    to: edge.to,
                    kind: edge.kind,
                });
            }
            Some(Ordering::Greater) => {
                return Err(CfgError::EdgeOutOfOrder {
                    from: edge.from,
                    to: edge.to,
                    kind: edge.kind,
                });
            }
            Some(Ordering::Less) | None => {}
        }
        previous = Some(edge);
    }
    return Ok(());
}

pub(super) fn check_loop_blocks(
    block_count: usize,
    header: BlockId,
    members: impl Iterator<Item = BlockId>,
) -> Result<(), CfgError> {
    for block in members {
        if !block
            .index()
            .is_some_and(|index| return index < block_count)
        {
            return Err(CfgError::LoopBlockMissing { header, block });
        }
    }
    return Ok(());
}

fn validate_loops(block_count: usize, loops: &[Loop]) -> Result<(), CfgError> {
    let mut previous: Option<BlockId> = None;
    for current in loops {
        check_loop_blocks(block_count, current.header, current.members())?;
        match previous.map(|before| return before.cmp(&current.header)) {
            Some(Ordering::Equal) => {
                return Err(CfgError::DuplicateLoopHeader {
                    header: current.header,
                });
            }
            Some(Ordering::Greater) => {
                return Err(CfgError::LoopOutOfOrder {
                    header: current.header,
                });
            }
            Some(Ordering::Less) | None => {}
        }
        previous = Some(current.header);
    }
    return Ok(());
}

impl TryFrom<FunctionRepr> for Function {
    type Error = CfgError;

    fn try_from(repr: FunctionRepr) -> Result<Self, Self::Error> {
        if repr.line_start > repr.line_end {
            return Err(CfgError::InvertedFunctionLines {
                line_start: repr.line_start,
                line_end: repr.line_end,
            });
        }
        validate_blocks(&repr.blocks)?;
        validate_edges(&repr.blocks, &repr.edges)?;
        validate_loops(repr.blocks.len(), &repr.loops)?;
        return Ok(Self {
            key: repr.key,
            file: repr.file,
            line_start: repr.line_start,
            line_end: repr.line_end,
            flags: repr.flags,
            blocks: repr.blocks,
            edges: repr.edges,
            loops: repr.loops,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Analysis {
    pub function: Function,
    pub diagnostics: Vec<Diagnostic>,
}

fn build_blocks(inputs: Vec<BlockInput>) -> Result<(Vec<Block>, Vec<Vec<BlockId>>), CfgError> {
    if u32::try_from(inputs.len()).is_err() {
        return Err(CfgError::TooManyBlocks {
            count: inputs.len(),
        });
    }
    let mut blocks = Vec::with_capacity(inputs.len());
    let mut successors = Vec::with_capacity(inputs.len());
    let mut next_op = OpIndex(0);
    for (id, input) in (0..).map(BlockId).zip(inputs) {
        let block = Block::new(id, next_op, input.flags, input.ops)?;
        next_op = block.next_op()?;
        blocks.push(block);
        successors.push(input.successors);
    }
    return Ok((blocks, successors));
}

fn analyze(input: FunctionInput) -> Result<Analysis, CfgError> {
    let (blocks, successors) = build_blocks(input.blocks)?;
    let nodes: Vec<Node<'_>> = blocks
        .iter()
        .zip(&successors)
        .map(|(block, block_successors)| {
            return Node {
                block,
                successors: block_successors,
            };
        })
        .collect();
    let classified = classify(&nodes, &input.loops)?;
    let function = Function::try_from(FunctionRepr {
        key: input.key,
        file: input.file,
        line_start: input.line_start,
        line_end: input.line_end,
        flags: input.flags,
        blocks,
        edges: classified.edges,
        loops: classified.loops,
    })?;
    return Ok(Analysis {
        function,
        diagnostics: classified.diagnostics,
    });
}

impl TryFrom<FunctionInput> for Analysis {
    type Error = SnapshotError;

    fn try_from(input: FunctionInput) -> Result<Self, Self::Error> {
        let key = input.key.clone();
        return analyze(input).map_err(|error| return SnapshotError { key, error });
    }
}
