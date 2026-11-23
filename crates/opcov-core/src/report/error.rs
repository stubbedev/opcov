use std::error::Error;
use std::fmt;

use super::model::{LoopId, OutcomeId, PathMode, SourcePath, Span, TestIndex, schema_id};
use crate::cfg::{FunctionKey, KeyError, Line};
use crate::kind::EdgeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collection {
    Tests,
    Hits,
    PathList,
    Files,
    Lines,
    Functions,
    Outcomes,
    Loops,
}

impl fmt::Display for Collection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(match self {
            Self::Tests => "tests",
            Self::Hits => "hit list",
            Self::PathList => "path list",
            Self::Files => "files",
            Self::Lines => "lines",
            Self::Functions => "functions",
            Self::Outcomes => "outcomes",
            Self::Loops => "loops",
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitSite {
    Line(Line),
    Outcome {
        function: FunctionKey,
        id: OutcomeId,
    },
    LoopZero {
        function: FunctionKey,
        id: LoopId,
    },
    LoopSecond {
        function: FunctionKey,
        id: LoopId,
    },
}

impl fmt::Display for HitSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Line(line) => write!(formatter, "line {line}"),
            Self::Outcome { function, id } => write!(formatter, "outcome {id} of `{function}`"),
            Self::LoopZero { function, id } => {
                write!(formatter, "zero list of loop {id} of `{function}`")
            }
            Self::LoopSecond { function, id } => {
                write!(formatter, "second list of loop {id} of `{function}`")
            }
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    NotAscending {
        collection: Collection,
        previous: String,
        next: String,
    },
    DuplicateKey {
        collection: Collection,
        key: String,
    },
    InvalidLineKey {
        key: String,
    },
    InvalidSpan {
        span: [u32; 4],
    },
    LineOutsideSpan {
        line: Line,
        span: Span,
    },
    InvalidSha256 {
        value: String,
    },
    InvalidId {
        value: String,
        prefix: char,
    },
    NotAnOutcome {
        kind: EdgeKind,
    },
    EmptyTestName,
    FunctionKey(KeyError),
    TooManyTests {
        count: usize,
    },
    InvalidPath {
        path: String,
    },
    InvalidRoot {
        root: String,
    },
    MissingRoot,
    UnexpectedRoot {
        root: String,
    },
    PathModeMismatch {
        path: SourcePath,
        mode: PathMode,
    },
    TestIndexOutOfRange {
        path: SourcePath,
        site: HitSite,
        index: TestIndex,
        tests: usize,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::NotAscending {
                collection,
                previous,
                next,
            } => write!(
                formatter,
                "{collection} must be strictly ascending and unique, but {next} follows {previous}"
            ),
            Self::DuplicateKey { collection, key } => {
                write!(formatter, "{collection} has the key {key:?} more than once")
            }
            Self::InvalidLineKey { key } => write!(
                formatter,
                "line key {key:?} is not a canonical decimal line number starting at 1"
            ),
            Self::InvalidSpan { span } => write!(
                formatter,
                "span {span:?} must be [line_start, col_start, line_end, col_end], 1-based, with the start before the end"
            ),
            Self::LineOutsideSpan { line, span } => {
                write!(formatter, "line {line} lies outside its span {span}")
            }
            Self::InvalidSha256 { value } => write!(
                formatter,
                "sha256 {value:?} is not 64 lowercase hexadecimal digits"
            ),
            Self::InvalidId { value, prefix } => write!(
                formatter,
                "id {value:?} is not `{prefix}` followed by a canonical decimal number"
            ),
            Self::NotAnOutcome { kind } => {
                write!(formatter, "edge kind {kind:?} is not an outcome")
            }
            Self::EmptyTestName => formatter.write_str("test names must not be empty"),
            Self::FunctionKey(error) => write!(formatter, "invalid function key: {error}"),
            Self::TooManyTests { count } => write!(
                formatter,
                "{count} tests exceed the {} that test indexes can address",
                u32::MAX
            ),
            Self::InvalidPath { path } => write!(
                formatter,
                "path {path:?} is not normalized: it is empty or has an empty, `.` or `..` segment"
            ),
            Self::InvalidRoot { root } => {
                write!(formatter, "root {root:?} is not a normalized absolute path")
            }
            Self::MissingRoot => formatter.write_str("paths in mode `relative` require a `root`"),
            Self::UnexpectedRoot { root } => write!(
                formatter,
                "paths in mode `absolute` must not have a root, found {root:?}"
            ),
            Self::PathModeMismatch { path, mode } => {
                write!(formatter, "path {path:?} does not match path mode `{mode}`")
            }
            Self::TestIndexOutOfRange {
                path,
                site,
                index,
                tests,
            } => write!(
                formatter,
                "{path}: {site} references test {index}, but the report has {tests} tests"
            ),
        };
    }
}

impl Error for ModelError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    Unsupported { found: String },
    Invalid { found: String },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Unsupported { found } => write!(
                formatter,
                "unsupported report schema version {found:?}: this opcov reads {:?}",
                schema_id()
            ),
            Self::Invalid { found } => write!(
                formatter,
                "invalid report schema id {found:?}: expected {:?}",
                schema_id()
            ),
        };
    }
}

impl Error for SchemaError {}

#[derive(Debug)]
pub enum ReadError {
    Schema(SchemaError),
    Decode(serde_json::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Schema(error) => write!(formatter, "{error}"),
            Self::Decode(error) => write!(formatter, "{error}"),
        };
    }
}

impl Error for ReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return match self {
            Self::Schema(error) => Some(error),
            Self::Decode(error) => Some(error),
        };
    }
}
