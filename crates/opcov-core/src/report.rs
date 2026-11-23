#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

mod combine;
mod decode;
mod encode;
mod error;
mod model;
mod streaming;

pub use combine::{
    ConflictSite, EntryConflict, HeaderField, HeaderMismatch, InputId, LoopField, MergeError,
    OutcomeField, merge,
};
pub use error::{Collection, HitSite, ModelError, ReadError, SchemaError};
pub use model::{
    Column, FileCoverage, Function, Generator, Header, HitSet, Id, IdSorted, Identified, Loop,
    LoopId, MeasuredShape, Outcome, OutcomeId, OutcomeKind, PathBase, PathMode, Report, Root,
    Settings, Sha256, SourcePath, Span, TestIndex, TestTable, VmKind, line_hits, schema_id,
};
pub use streaming::{StreamMergeError, merge_streams};
