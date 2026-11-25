#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

mod combine;
mod decode;
mod encode;
mod error;
mod model;
mod scan;
mod stitch;
mod streaming;

pub use combine::{
    ConflictSite, EntryConflict, HeaderField, HeaderMismatch, InputId, LoopField, MergeError,
    OutcomeField, StaleEntry, merge,
};
pub use decode::{RawEntry, ReportReader};
pub use encode::{ReportWriter, WriteError};
pub use error::{Collection, EntryError, HitSite, ModelError, ReadError, SchemaError};
pub use model::{
    Column, FileCoverage, Function, Generator, Header, HitSet, Id, IdSorted, Identified, Loop,
    LoopId, MeasuredShape, Outcome, OutcomeId, OutcomeKind, PathBase, PathMode, Report, Root,
    Settings, Sha256, SourcePath, Span, TestIndex, TestTable, VmKind, line_hits, schema_id,
};
pub use scan::Found;
pub use stitch::stitch_streams;
pub use streaming::{StreamMergeError, merge_streams};
