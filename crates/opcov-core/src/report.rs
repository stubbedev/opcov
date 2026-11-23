#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

mod decode;
mod encode;
mod error;
mod model;

pub use error::{Collection, HitSite, ModelError, ReadError, SchemaError};
pub use model::{
    Column, FileCoverage, Function, Generator, Header, HitSet, Id, IdSorted, Identified, Loop,
    LoopId, Outcome, OutcomeId, OutcomeKind, PathBase, PathMode, Report, Root, Settings, Sha256,
    SourcePath, Span, TestIndex, TestTable, VmKind, schema_id,
};
