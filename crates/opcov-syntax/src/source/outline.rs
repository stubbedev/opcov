use mago_database::file::FileId;
use mago_syntax::lexer::Lexer;
use mago_syntax::settings::LexerSettings;
use mago_syntax::token::TokenKind;
use mago_syntax_core::input::Input;
use opcov_core::cfg::{ClassRef, FunctionKey, HookKind, Identifier, Line};

use super::position::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    Named(FunctionKey),
    Closure,
    AnonymousMethod(Identifier),
    AnonymousHook(Identifier, HookKind),
}

impl Shape {
    fn of(key: &FunctionKey) -> Self {
        return match key {
            FunctionKey::Closure { .. } => Self::Closure,
            FunctionKey::Method {
                class: ClassRef::Anonymous { .. },
                method,
            } => Self::AnonymousMethod(method.clone()),
            FunctionKey::PropertyHook {
                class: ClassRef::Anonymous { .. },
                property,
                hook,
            } => Self::AnonymousHook(property.clone(), *hook),
            named @ (FunctionKey::Main { .. }
            | FunctionKey::Function { .. }
            | FunctionKey::Method { .. }
            | FunctionKey::PropertyHook { .. }) => Self::Named(named.clone()),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Token(TokenKind, Box<[u8]>),
    Function(Shape),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divergence {
    here: Option<Line>,
    there: Option<Line>,
}

impl Divergence {
    #[must_use]
    pub const fn here(self) -> Option<Line> {
        return self.here;
    }

    #[must_use]
    pub const fn there(self) -> Option<Line> {
        return self.there;
    }
}

#[derive(Debug, Clone)]
pub struct Outline {
    pieces: Vec<(Piece, Line)>,
}

impl Outline {
    #[must_use]
    pub fn diverges_from(&self, other: &Self) -> Option<Divergence> {
        let mut mine = self.pieces.iter();
        let mut theirs = other.pieces.iter();
        loop {
            match (mine.next(), theirs.next()) {
                (None, None) => return None,
                (Some((left, _)), Some((right, _))) if left == right => {}
                (here, there) => {
                    return Some(Divergence {
                        here: here.map(|(_, line)| return *line),
                        there: there.map(|(_, line)| return *line),
                    });
                }
            }
        }
    }
}

impl PartialEq for Outline {
    fn eq(&self, other: &Self) -> bool {
        return self.diverges_from(other).is_none();
    }
}

impl Eq for Outline {}

pub(super) struct Body<'k> {
    pub(super) bytes: ByteSpan,
    pub(super) key: &'k FunctionKey,
}

pub(super) enum OutlineError {
    Lex { offset: u32, message: String },
    Offset(u32),
}

pub(super) fn outline<'k>(
    source: &[u8],
    bodies: impl Iterator<Item = Body<'k>>,
    line: impl Fn(u32) -> Option<Line>,
) -> Result<Outline, OutlineError> {
    let mut outermost: Vec<Body<'k>> = Vec::new();
    for body in bodies {
        if matches!(body.key, FunctionKey::Main { .. }) {
            continue;
        }
        if outermost
            .last()
            .is_some_and(|last| return body.bytes.start < last.bytes.end)
        {
            continue;
        }
        outermost.push(body);
    }
    let at = |offset: u32| return line(offset).ok_or(OutlineError::Offset(offset));
    let mut pieces = Vec::new();
    let mut next = outermost.iter().peekable();
    let placed = |body: &Body<'_>, pieces: &mut Vec<(Piece, Line)>| {
        pieces.push((Piece::Function(Shape::of(body.key)), at(body.bytes.start)?));
        return Ok(());
    };
    let mut lexer = Lexer::new(Input::new(FileId::zero(), source), LexerSettings::default());
    while let Some(token) = lexer.advance() {
        let token = token.map_err(|error| {
            return OutlineError::Lex {
                offset: mago_span::HasSpan::span(&error).start.offset,
                message: error.to_string(),
            };
        })?;
        let offset = token.start.offset;
        while let Some(body) = next.next_if(|body| return body.bytes.end <= offset) {
            placed(body, &mut pieces)?;
        }
        if next
            .peek()
            .is_some_and(|body| return body.bytes.start <= offset)
        {
            continue;
        }
        if token.kind.is_trivia() && token.kind != TokenKind::DocBlockComment {
            continue;
        }
        pieces.push((Piece::Token(token.kind, token.value.into()), at(offset)?));
    }
    for body in next {
        placed(body, &mut pieces)?;
    }
    return Ok(Outline { pieces });
}
