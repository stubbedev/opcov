mod classify;
mod key;
mod model;
mod opcode;

pub use classify::{Diagnostic, Successor, opcode_edge_kind};
pub use key::{
    ClassRef, FilePath, FunctionKey, HookKind, Identifier, KeyError, KeyErrorReason, LineSpan,
    QualifiedName,
};
pub use model::{
    Analysis, Block, BlockFlag, BlockFlags, BlockId, BlockInput, CfgError, Edge, Flag, FlagSet,
    Function, FunctionFlag, FunctionFlags, FunctionInput, Line, Loop, LoopInput, LoopShape, Op,
    OpIndex, SnapshotError,
};
pub use opcode::{Opcode, PhpVersion};
