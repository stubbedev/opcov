#![forbid(unsafe_code)]
#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

mod source;

pub use source::{
    BranchKind, Column, Construct, ConstructKind, ConstructOutcome, FunctionSource, Location,
    LookupError, MatchedOutcome, Matching, OutcomeRef, Position, SourceFile, Span, SyntaxError,
    SyntaxErrorKind, Unmatched, outcome_refs,
};
