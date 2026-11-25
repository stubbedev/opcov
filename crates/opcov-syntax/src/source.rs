use std::fmt;

use mago_allocator::prelude::LocalArena;
use mago_database::file::FileId;
use mago_span::HasSpan;
use mago_syntax::parser::parse_file_content;
use opcov_core::cfg::{
    ClassRef, FilePath, FunctionKey, HookKind, Identifier, KeyError, Line, LineSpan, QualifiedName,
};
use opcov_core::encoding;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

mod collect;
mod construct;
mod matching;
mod outline;
mod position;

pub use construct::{BranchKind, Construct, ConstructKind, ConstructOutcome, Location, Unmatched};
pub use matching::{MatchedOutcome, Matching, OutcomeRef, outcome_refs};
pub use outline::{Divergence, Outline};
pub use position::{Column, Position, Span};

use collect::{
    Placement, RawClass, RawClosure, RawConstruct, RawDeclaration, RawFunction, RawKey, collect,
};
use matching::Site;
use outline::{Body, OutlineError, outline};
use position::{ByteSpan, LineIndex, LineRange, OffsetOutOfRange, next_token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    TooLarge { bytes: usize },
    Parse { position: Position, message: String },
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
    bytes: ByteSpan,
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

    fn name<T>(
        &self,
        bytes: &[u8],
        parse: fn(&[u8]) -> Result<T, KeyError>,
    ) -> Result<T, SyntaxError> {
        return parse(bytes).map_err(|error| return self.fail(SyntaxErrorKind::InvalidName(error)));
    }

    fn class(
        &self,
        class: Option<&RawClass>,
        at: u32,
        anonymous: &[(LineSpan, u32)],
    ) -> Result<ClassRef, SyntaxError> {
        return match class {
            None => Err(self.fail(SyntaxErrorKind::MemberOutsideClass {
                line: self.line(at)?,
            })),
            Some(RawClass::Named(name)) => {
                Ok(ClassRef::Named(self.name(name, QualifiedName::from_bytes)?))
            }
            Some(RawClass::Anonymous(span)) => {
                let lines = self.line_span(*span)?;
                Ok(ClassRef::Anonymous {
                    file: self.path.clone(),
                    lines,
                    ordinal: self.count(ordinal_in(anonymous, &lines, span.start))?,
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
            name: encoding::encode(hook).into_owned(),
            line: self.line(at)?,
        }));
    }

    fn arrow_end(&self, body_end: u32) -> Result<u32, SyntaxError> {
        return next_token(self.source, body_end).ok_or_else(|| {
            return self.fail(SyntaxErrorKind::OffsetOutOfRange { offset: body_end });
        });
    }

    fn closure_span(&self, closure: RawClosure) -> Result<ByteSpan, SyntaxError> {
        return Ok(match closure {
            RawClosure::Braced { start, end } => ByteSpan::new(start, end),
            RawClosure::Arrow { start, body_end } => {
                ByteSpan::new(start, self.arrow_end(body_end)?)
            }
        });
    }

    fn declaration(
        &self,
        raw: &RawDeclaration,
        anonymous: &[(LineSpan, u32)],
    ) -> Result<FunctionKey, SyntaxError> {
        return Ok(match raw {
            RawDeclaration::Main => FunctionKey::Main {
                file: self.path.clone(),
            },
            RawDeclaration::Function { name } => FunctionKey::Function {
                name: self.name(name, QualifiedName::from_bytes)?,
            },
            RawDeclaration::Method { class, method, at } => FunctionKey::Method {
                class: self.class(class.as_ref(), *at, anonymous)?,
                method: self.name(method, Identifier::from_bytes)?,
            },
            RawDeclaration::Hook {
                class,
                property,
                hook,
                at,
            } => {
                let class = self.class(class.as_ref(), *at, anonymous)?;
                let Some(property) = property else {
                    return Err(self.fail(SyntaxErrorKind::HookOutsideProperty {
                        line: self.line(*at)?,
                    }));
                };
                FunctionKey::PropertyHook {
                    class,
                    property: self.name(property, Identifier::from_bytes)?,
                    hook: self.hook(hook, *at)?,
                }
            }
        });
    }

    fn rank_closures(
        &self,
        closures: Vec<(RawClosure, Placement, RawFunction)>,
        anonymous: &[(LineSpan, u32)],
    ) -> Result<Vec<Keyed>, SyntaxError> {
        let mut ranked = closures
            .into_iter()
            .map(|(closure, placement, raw)| {
                let lines = self.line_span(self.closure_span(closure)?)?;
                let root = self.declaration(&placement.root, anonymous)?;
                return Ok(((lines, placement.deferred, root, placement.order), raw));
            })
            .collect::<Result<Vec<_>, SyntaxError>>()?;
        ranked.sort_unstable_by(|left, right| return left.0.cmp(&right.0));
        let mut keyed = Vec::with_capacity(ranked.len());
        let mut previous = None;
        let mut ordinal = 0_u32;
        for ((lines, ..), raw) in ranked {
            ordinal = if previous == Some(lines) {
                ordinal.saturating_add(1)
            } else {
                0
            };
            previous = Some(lines);
            keyed.push(Keyed {
                key: FunctionKey::Closure {
                    file: self.path.clone(),
                    lines,
                    ordinal,
                },
                raw,
            });
        }
        return Ok(keyed);
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
            bytes: keyed.raw.span,
            constructs,
            sites,
        });
    }

    fn outline(&self, functions: &[FunctionSource]) -> Result<Outline, SyntaxError> {
        let bodies = functions.iter().map(|function| {
            return Body {
                bytes: function.bytes,
                key: &function.key,
            };
        });
        return outline(self.source, bodies, |offset| {
            return self.index.line(offset).ok();
        })
        .map_err(|error| {
            return match error {
                OutlineError::Offset(offset) => {
                    self.fail(SyntaxErrorKind::OffsetOutOfRange { offset })
                }
                OutlineError::Lex { offset, message } => match self.index.position(offset) {
                    Ok(position) => self.fail(SyntaxErrorKind::Parse { position, message }),
                    Err(failure) => self.located(failure),
                },
            };
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
        let mut keyed = Vec::with_capacity(functions.len());
        let mut closures = Vec::new();
        for raw in functions {
            match &raw.key {
                RawKey::Declaration(declaration) => keyed.push(Keyed {
                    key: self.declaration(declaration, &anonymous)?,
                    raw,
                }),
                RawKey::Closure { closure, placement } => {
                    closures.push((*closure, placement.clone(), raw));
                }
            }
        }
        keyed.extend(self.rank_closures(closures, &anonymous)?);
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
        return Self::parse_with(path, source, |_, _| return Ok(())).map(|(file, ())| return file);
    }

    pub fn parse_outlined(path: FilePath, source: &[u8]) -> Result<(Self, Outline), SyntaxError> {
        return Self::parse_with(path, source, |builder, functions| {
            return builder.outline(functions);
        });
    }

    fn parse_with<T>(
        path: FilePath,
        source: &[u8],
        extra: impl FnOnce(&Builder<'_>, &[FunctionSource]) -> Result<T, SyntaxError>,
    ) -> Result<(Self, T), SyntaxError> {
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
        let extra = extra(&builder, &functions)?;
        return Ok((Self { path, functions }, extra));
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
