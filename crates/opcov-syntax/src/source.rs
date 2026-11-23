use std::fmt;
use std::str::FromStr;

use mago_allocator::prelude::LocalArena;
use mago_database::file::FileId;
use mago_span::HasSpan;
use mago_syntax::parser::parse_file_content;
use opcov_core::cfg::{ClassRef, FilePath, FunctionKey, HookKind, KeyError, Line, LineSpan};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

mod collect;
mod construct;
mod matching;
mod position;

pub use construct::{BranchKind, Construct, ConstructKind, ConstructOutcome, Location, Unmatched};
pub use matching::{MatchedOutcome, Matching, OutcomeRef, outcome_refs};
pub use position::{Column, Position, Span};

use collect::{RawClass, RawConstruct, RawFunction, RawKey, collect};
use matching::Site;
use position::{ByteSpan, LineIndex, LineRange, OffsetOutOfRange, next_token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    TooLarge { bytes: usize },
    Parse { position: Position, message: String },
    NonUtf8Name { name: String, valid_up_to: usize },
    InvalidName(KeyError),
    UnknownPropertyHook { name: String, line: Line },
    MemberOutsideClass { line: Line },
    HookOutsideProperty { line: Line },
    OffsetOutOfRange { offset: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    path: FilePath,
    kind: SyntaxErrorKind,
}

impl SyntaxError {
    #[must_use]
    pub const fn path(&self) -> &FilePath {
        return &self.path;
    }

    #[must_use]
    pub const fn kind(&self) -> &SyntaxErrorKind {
        return &self.kind;
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = &self.path;
        return match &self.kind {
            SyntaxErrorKind::TooLarge { bytes } => {
                write!(
                    f,
                    "cannot map `{path}`: {bytes} bytes exceed the 4 GiB limit"
                )
            }
            SyntaxErrorKind::Parse { position, message } => {
                write!(f, "cannot parse `{path}` at {position}: {message}")
            }
            SyntaxErrorKind::NonUtf8Name { name, valid_up_to } => write!(
                f,
                "cannot map `{path}`: the name `{name}` is not valid UTF-8 after byte {valid_up_to}"
            ),
            SyntaxErrorKind::InvalidName(error) => write!(f, "cannot map `{path}`: {error}"),
            SyntaxErrorKind::UnknownPropertyHook { name, line } => write!(
                f,
                "cannot map `{path}`: line {line} declares the property hook `{name}`, which is neither `get` nor `set`"
            ),
            SyntaxErrorKind::MemberOutsideClass { line } => {
                write!(
                    f,
                    "cannot map `{path}`: line {line} declares a member outside a class"
                )
            }
            SyntaxErrorKind::HookOutsideProperty { line } => {
                write!(
                    f,
                    "cannot map `{path}`: line {line} declares a hook outside a property"
                )
            }
            SyntaxErrorKind::OffsetOutOfRange { offset } => {
                write!(
                    f,
                    "cannot map `{path}`: byte offset {offset} lies outside the file"
                )
            }
        };
    }
}

impl std::error::Error for SyntaxError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    NotFound { key: FunctionKey },
    Ambiguous { key: FunctionKey, count: usize },
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::NotFound { key } => write!(f, "the source declares no function `{key}`"),
            Self::Ambiguous { key, count } => {
                write!(
                    f,
                    "the source declares {count} functions with the key `{key}`"
                )
            }
        };
    }
}

impl std::error::Error for LookupError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSource {
    key: FunctionKey,
    span: Span,
    constructs: Vec<Construct>,
    sites: Vec<Site<Span, LineRange>>,
}

impl FunctionSource {
    #[must_use]
    pub const fn key(&self) -> &FunctionKey {
        return &self.key;
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        return self.span;
    }

    #[must_use]
    pub fn constructs(&self) -> &[Construct] {
        return &self.constructs;
    }

    #[must_use]
    pub fn match_outcomes(&self, outcomes: &[OutcomeRef]) -> Matching {
        return Matching::new(&self.constructs, &self.sites, outcomes);
    }
}

impl Serialize for FunctionSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("FunctionSource", 3)?;
        state.serialize_field("key", &self.key)?;
        state.serialize_field("span", &self.span)?;
        state.serialize_field("constructs", &self.constructs)?;
        return state.end();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    path: FilePath,
    functions: Vec<FunctionSource>,
}

struct Builder<'s> {
    path: &'s FilePath,
    source: &'s [u8],
    index: LineIndex,
}

fn ordinal_in<K: Ord>(sorted: &[(K, u32)], key: &K, start: u32) -> usize {
    let before = sorted.partition_point(|(other, other_start)| {
        return (other, *other_start) < (key, start);
    });
    let group = sorted.partition_point(|(other, _)| return other < key);
    return before.saturating_sub(group);
}

struct Keyed {
    key: FunctionKey,
    raw: RawFunction,
}

struct Ranks {
    anonymous: Vec<(LineSpan, u32)>,
    closures: Vec<(LineSpan, u32)>,
}

impl Builder<'_> {
    fn fail(&self, kind: SyntaxErrorKind) -> SyntaxError {
        return SyntaxError {
            path: self.path.clone(),
            kind,
        };
    }

    fn located(&self, error: OffsetOutOfRange) -> SyntaxError {
        return self.fail(SyntaxErrorKind::OffsetOutOfRange {
            offset: error.offset,
        });
    }

    fn line(&self, offset: u32) -> Result<Line, SyntaxError> {
        return self
            .index
            .line(offset)
            .map_err(|error| return self.located(error));
    }

    fn span(&self, span: ByteSpan) -> Result<Span, SyntaxError> {
        return self
            .index
            .span(span)
            .map_err(|error| return self.located(error));
    }

    fn lines(&self, span: ByteSpan) -> Result<LineRange, SyntaxError> {
        return self
            .index
            .lines(span)
            .map_err(|error| return self.located(error));
    }

    fn line_span(&self, span: ByteSpan) -> Result<LineSpan, SyntaxError> {
        let first = self.line(span.start)?;
        let last = self.line(span.end)?;
        return LineSpan::new(first, last).ok_or_else(|| {
            return self.fail(SyntaxErrorKind::OffsetOutOfRange { offset: span.end });
        });
    }

    fn count(&self, ordinal: usize) -> Result<u32, SyntaxError> {
        return u32::try_from(ordinal).ok().ok_or_else(|| {
            return self.fail(SyntaxErrorKind::TooLarge {
                bytes: self.source.len(),
            });
        });
    }

    fn text<'b>(&self, bytes: &'b [u8]) -> Result<&'b str, SyntaxError> {
        return std::str::from_utf8(bytes).map_err(|error| {
            return self.fail(SyntaxErrorKind::NonUtf8Name {
                name: String::from_utf8_lossy(bytes).into_owned(),
                valid_up_to: error.valid_up_to(),
            });
        });
    }

    fn name<T: FromStr<Err = KeyError>>(&self, bytes: &[u8]) -> Result<T, SyntaxError> {
        return self
            .text(bytes)?
            .parse()
            .map_err(|error| return self.fail(SyntaxErrorKind::InvalidName(error)));
    }

    fn class(
        &self,
        class: Option<&RawClass>,
        at: u32,
        ranks: &Ranks,
    ) -> Result<ClassRef, SyntaxError> {
        return match class {
            None => Err(self.fail(SyntaxErrorKind::MemberOutsideClass {
                line: self.line(at)?,
            })),
            Some(RawClass::Named(name)) => Ok(ClassRef::Named(self.name(name)?)),
            Some(RawClass::Anonymous(span)) => {
                let lines = self.line_span(*span)?;
                Ok(ClassRef::Anonymous {
                    file: self.path.clone(),
                    lines,
                    ordinal: self.count(ordinal_in(&ranks.anonymous, &lines, span.start))?,
                })
            }
        };
    }

    fn hook(&self, hook: &[u8], at: u32) -> Result<HookKind, SyntaxError> {
        if hook.eq_ignore_ascii_case(b"get") {
            return Ok(HookKind::Get);
        }
        if hook.eq_ignore_ascii_case(b"set") {
            return Ok(HookKind::Set);
        }
        return Err(self.fail(SyntaxErrorKind::UnknownPropertyHook {
            name: String::from_utf8_lossy(hook).into_owned(),
            line: self.line(at)?,
        }));
    }

    fn arrow_end(&self, body_end: u32) -> Result<u32, SyntaxError> {
        return next_token(self.source, body_end).ok_or_else(|| {
            return self.fail(SyntaxErrorKind::OffsetOutOfRange { offset: body_end });
        });
    }

    fn closure_span(&self, raw: &RawKey) -> Result<Option<ByteSpan>, SyntaxError> {
        return Ok(match raw {
            RawKey::Closure { start, end } => Some(ByteSpan::new(*start, *end)),
            RawKey::Arrow { start, body_end } => {
                Some(ByteSpan::new(*start, self.arrow_end(*body_end)?))
            }
            RawKey::Main
            | RawKey::Function { .. }
            | RawKey::Method { .. }
            | RawKey::Hook { .. } => None,
        });
    }

    fn closure(&self, span: ByteSpan, ranks: &Ranks) -> Result<FunctionKey, SyntaxError> {
        let lines = self.line_span(span)?;
        return Ok(FunctionKey::Closure {
            file: self.path.clone(),
            lines,
            ordinal: self.count(ordinal_in(&ranks.closures, &lines, span.start))?,
        });
    }

    fn key(&self, raw: &RawKey, ranks: &Ranks) -> Result<FunctionKey, SyntaxError> {
        return Ok(match raw {
            RawKey::Main => FunctionKey::Main {
                file: self.path.clone(),
            },
            RawKey::Function { name } => FunctionKey::Function {
                name: self.name(name)?,
            },
            RawKey::Method { class, method, at } => FunctionKey::Method {
                class: self.class(class.as_ref(), *at, ranks)?,
                method: self.name(method)?,
            },
            RawKey::Hook {
                class,
                property,
                hook,
                at,
            } => {
                let class = self.class(class.as_ref(), *at, ranks)?;
                let Some(property) = property else {
                    return Err(self.fail(SyntaxErrorKind::HookOutsideProperty {
                        line: self.line(*at)?,
                    }));
                };
                FunctionKey::PropertyHook {
                    class,
                    property: self.name(property)?,
                    hook: self.hook(hook, *at)?,
                }
            }
            RawKey::Closure { start, end } => self.closure(ByteSpan::new(*start, *end), ranks)?,
            RawKey::Arrow { start, body_end } => {
                self.closure(ByteSpan::new(*start, self.arrow_end(*body_end)?), ranks)?
            }
        });
    }

    fn function(&self, keyed: Keyed) -> Result<FunctionSource, SyntaxError> {
        let mut constructs = keyed
            .raw
            .constructs
            .into_iter()
            .map(|RawConstruct { span, shape }| {
                return Ok(Construct::new(
                    self.span(span)?,
                    shape.try_map(|part| return self.span(part))?,
                ));
            })
            .collect::<Result<Vec<Construct>, SyntaxError>>()?;
        constructs.sort_by_key(|construct| return (construct.span(), construct.kind()));
        let sites = keyed
            .raw
            .sites
            .into_iter()
            .map(|site| {
                return site.try_map(
                    |span| return self.span(span),
                    |claim| return self.lines(claim),
                );
            })
            .collect::<Result<Vec<_>, SyntaxError>>()?;
        return Ok(FunctionSource {
            key: keyed.key,
            span: self.span(keyed.raw.span)?,
            constructs,
            sites,
        });
    }

    fn build(
        &self,
        functions: Vec<RawFunction>,
        anonymous: &[ByteSpan],
    ) -> Result<Vec<FunctionSource>, SyntaxError> {
        let mut anonymous = anonymous
            .iter()
            .map(|span| return Ok((self.line_span(*span)?, span.start)))
            .collect::<Result<Vec<(LineSpan, u32)>, SyntaxError>>()?;
        anonymous.sort_unstable();
        let mut closures = Vec::new();
        for function in &functions {
            if let Some(span) = self.closure_span(&function.key)? {
                closures.push((self.line_span(span)?, span.start));
            }
        }
        closures.sort_unstable();
        let ranks = Ranks {
            anonymous,
            closures,
        };
        let mut keyed = functions
            .into_iter()
            .map(|raw| {
                let key = self.key(&raw.key, &ranks)?;
                return Ok(Keyed { key, raw });
            })
            .collect::<Result<Vec<Keyed>, SyntaxError>>()?;
        keyed.sort_by(|left, right| {
            return (left.raw.span.start, right.raw.span.end, &left.key).cmp(&(
                right.raw.span.start,
                left.raw.span.end,
                &right.key,
            ));
        });
        return keyed
            .into_iter()
            .map(|function| return self.function(function))
            .collect();
    }
}

impl SourceFile {
    pub fn parse(path: FilePath, source: &[u8]) -> Result<Self, SyntaxError> {
        let too_large = || {
            return SyntaxError {
                path: path.clone(),
                kind: SyntaxErrorKind::TooLarge {
                    bytes: source.len(),
                },
            };
        };
        let index = LineIndex::new(source).ok_or_else(too_large)?;
        let length = u32::try_from(source.len()).ok().ok_or_else(too_large)?;
        let builder = Builder {
            path: &path,
            source,
            index,
        };
        let arena = LocalArena::new();
        let program = parse_file_content(&arena, FileId::zero(), source);
        if let Some(error) = program.errors.first() {
            let position = builder
                .index
                .position(error.span().start.offset)
                .map_err(|failure| return builder.located(failure))?;
            return Err(builder.fail(SyntaxErrorKind::Parse {
                position,
                message: error.to_string(),
            }));
        }
        let collected = collect(program, length);
        let functions = builder.build(collected.functions, &collected.anonymous_classes)?;
        return Ok(Self { path, functions });
    }

    #[must_use]
    pub const fn path(&self) -> &FilePath {
        return &self.path;
    }

    #[must_use]
    pub fn functions(&self) -> &[FunctionSource] {
        return &self.functions;
    }

    pub fn function(&self, key: &FunctionKey) -> Result<&FunctionSource, LookupError> {
        let mut found = self
            .functions
            .iter()
            .filter(|function| return &function.key == key);
        let Some(first) = found.next() else {
            return Err(LookupError::NotFound { key: key.clone() });
        };
        let others = found.count();
        if others > 0 {
            return Err(LookupError::Ambiguous {
                key: key.clone(),
                count: others.saturating_add(1),
            });
        }
        return Ok(first);
    }
}
