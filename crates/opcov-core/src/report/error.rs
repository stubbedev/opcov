use std::error::Error;
use std::fmt;
use std::io;

use super::model::{LoopId, OutcomeId, PathMode, Root, SourcePath, Span, TestIndex, schema_id};
use super::scan::Found;
use crate::cfg::{FunctionKey, KeyError, Line};
use crate::encoding::NotCanonical;
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
    UnstructuredLoop,
    EmptyTestName,
    FunctionKey(KeyError),
    TooManyTests {
        count: usize,
    },
    Encoding(NotCanonical),
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
    OutsideRoot {
        path: String,
        root: Root,
    },
    KeyFileMismatch {
        path: SourcePath,
        key: Box<FunctionKey>,
    },
    HitOnNonExecutableLine {
        line: Line,
        test: TestIndex,
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
            Self::UnstructuredLoop => formatter.write_str(
                "an unstructured loop has no zero or second iteration to report; loops are Foreach, PreTest or PostTest",
            ),
            Self::EmptyTestName => formatter.write_str("test names must not be empty"),
            Self::FunctionKey(error) => write!(formatter, "invalid function key: {error}"),
            Self::TooManyTests { count } => write!(
                formatter,
                "{count} tests exceed the {} that test indexes can address",
                u32::MAX
            ),
            Self::Encoding(error) => write!(formatter, "path {error}"),
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
            Self::OutsideRoot { path, root } => write!(
                formatter,
                "measured file {path} lies outside the report root {}",
                root.as_str()
            ),
            Self::KeyFileMismatch { path, key } => write!(
                formatter,
                "{path}: function key `{key}` names a different file than its entry"
            ),
            Self::HitOnNonExecutableLine { line, test } => write!(
                formatter,
                "test {test} hit line {line}, which is not an executable line of its file"
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
pub enum EntryError {
    Decode(serde_json::Error),
    Model(Box<ModelError>),
}

impl fmt::Display for EntryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Decode(error) => write!(formatter, "{error}"),
            Self::Model(error) => write!(formatter, "{error}"),
        };
    }
}

impl Error for EntryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return match self {
            Self::Decode(error) => Some(error),
            Self::Model(error) => Some(&**error),
        };
    }
}

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    Syntax {
        offset: u64,
        expected: &'static str,
        found: Found,
    },
    Key {
        offset: u64,
        error: serde_json::Error,
    },
    Member {
        expected: &'static str,
        found: Option<String>,
    },
    Trailing {
        member: String,
    },
    Value {
        member: &'static str,
        error: serde_json::Error,
    },
    Schema(SchemaError),
    Files(Box<ModelError>),
    Entry {
        path: SourcePath,
        error: Box<EntryError>,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Syntax {
                offset,
                expected,
                found,
            } => write!(
                formatter,
                "expected {expected} at byte {offset}, found {found}"
            ),
            Self::Key { offset, error } => {
                write!(
                    formatter,
                    "invalid member name before byte {offset}: {error}"
                )
            }
            Self::Member {
                expected,
                found: Some(found),
            } => write!(formatter, "expected member `{expected}`, found `{found}`"),
            Self::Member {
                expected,
                found: None,
            } => write!(formatter, "missing member `{expected}`"),
            Self::Trailing { member } => {
                write!(formatter, "unexpected member `{member}` after `files`")
            }
            Self::Value { member, error } => write!(formatter, "member `{member}`: {error}"),
            Self::Schema(error) => write!(formatter, "{error}"),
            Self::Files(error) => write!(formatter, "files: {error}"),
            Self::Entry { path, error } => write!(formatter, "files entry {path:?}: {error}"),
        };
    }
}

impl Error for ReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return match self {
            Self::Io(error) => Some(error),
            Self::Key { error, .. } | Self::Value { error, .. } => Some(error),
            Self::Schema(error) => Some(error),
            Self::Files(error) => Some(&**error),
            Self::Entry { error, .. } => Some(&**error),
            Self::Syntax { .. } | Self::Member { .. } | Self::Trailing { .. } => None,
        };
    }
}
