use std::fmt;
use std::num::NonZeroU32;

use opcov_core::cfg::Line;
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Column(NonZeroU32);

impl Column {
    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0.get();
    }
}

impl fmt::Display for Column {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{}", self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    line: Line,
    column: Column,
}

impl Position {
    #[must_use]
    pub const fn line(self) -> Line {
        return self.line;
    }

    #[must_use]
    pub const fn column(self) -> Column {
        return self.column;
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{}:{}", self.line, self.column);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    start: Position,
    end: Position,
}

impl Span {
    #[must_use]
    pub const fn start(self) -> Position {
        return self.start;
    }

    #[must_use]
    pub const fn end(self) -> Position {
        return self.end;
    }

    pub(super) const fn join(first: Self, last: Self) -> Self {
        return Self {
            start: first.start,
            end: last.end,
        };
    }

    #[must_use]
    pub fn contains_line(self, line: Line) -> bool {
        return self.start.line <= line && line <= self.end.line;
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{}-{}", self.start, self.end);
    }
}

impl Serialize for Span {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(4)?;
        tuple.serialize_element(&self.start.line.get())?;
        tuple.serialize_element(&self.start.column.get())?;
        tuple.serialize_element(&self.end.line.get())?;
        tuple.serialize_element(&self.end.column.get())?;
        return tuple.end();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct ByteSpan {
    pub(super) start: u32,
    pub(super) end: u32,
}

impl ByteSpan {
    pub(super) const fn new(start: u32, end: u32) -> Self {
        return Self { start, end };
    }

    pub(super) const fn of(span: mago_span::Span) -> Self {
        return Self::new(span.start.offset, span.end.offset);
    }

    pub(super) const fn between(first: mago_span::Span, last: mago_span::Span) -> Self {
        return Self::new(first.start.offset, last.end.offset);
    }

    const fn last_byte(self) -> u32 {
        return match self.end.checked_sub(1) {
            Some(last) if last >= self.start => last,
            _ => self.start,
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct LineRange {
    first: Line,
    last: Line,
}

impl LineRange {
    pub(super) fn contains(self, line: Line) -> bool {
        return self.first <= line && line <= self.last;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OffsetOutOfRange {
    pub(super) offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    pub(super) fn new(source: &[u8]) -> Option<Self> {
        if u32::try_from(source.len()).ok()? == u32::MAX {
            return None;
        }
        let following = source.iter().skip(1).map(Some).chain([None]);
        let mut starts = vec![0];
        for ((after, &byte), next) in (1..=u32::MAX).zip(source).zip(following) {
            let breaks = byte == b'\n' || (byte == b'\r' && next != Some(&b'\n'));
            if breaks {
                starts.push(after);
            }
        }
        return Some(Self { starts });
    }

    pub(super) fn position(&self, offset: u32) -> Result<Position, OffsetOutOfRange> {
        let error = OffsetOutOfRange { offset };
        let count = self.starts.partition_point(|&start| return start <= offset);
        let line = u32::try_from(count).ok().and_then(Line::new).ok_or(error)?;
        let column = count
            .checked_sub(1)
            .and_then(|index| return self.starts.get(index))
            .and_then(|&start| return offset.checked_sub(start))
            .and_then(|delta| return delta.checked_add(1))
            .and_then(NonZeroU32::new)
            .map(Column)
            .ok_or(error)?;
        return Ok(Position { line, column });
    }

    pub(super) fn line(&self, offset: u32) -> Result<Line, OffsetOutOfRange> {
        return self.position(offset).map(Position::line);
    }

    pub(super) fn span(&self, span: ByteSpan) -> Result<Span, OffsetOutOfRange> {
        return Ok(Span {
            start: self.position(span.start)?,
            end: self.position(span.end)?,
        });
    }

    pub(super) fn lines(&self, span: ByteSpan) -> Result<LineRange, OffsetOutOfRange> {
        return Ok(LineRange {
            first: self.line(span.start)?,
            last: self.line(span.last_byte())?,
        });
    }
}

fn whitespace_length(rest: &[u8]) -> usize {
    return rest
        .iter()
        .take_while(|&&byte| return matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        .count();
}

fn line_comment_length(rest: &[u8]) -> usize {
    return rest
        .windows(2)
        .position(|pair| return pair.first() == Some(&b'\n') || pair == b"?>")
        .unwrap_or_else(|| {
            return if rest.last() == Some(&b'\n') {
                rest.len().saturating_sub(1)
            } else {
                rest.len()
            };
        });
}

fn comment_length(rest: &[u8]) -> usize {
    if rest.starts_with(b"/*") {
        return rest
            .windows(2)
            .skip(2)
            .position(|pair| return pair == b"*/")
            .map_or(rest.len(), |found| return found.saturating_add(4));
    }
    if rest.starts_with(b"//") || (rest.starts_with(b"#") && !rest.starts_with(b"#[")) {
        return line_comment_length(rest);
    }
    return 0;
}

pub(super) fn next_token(source: &[u8], from: u32) -> Option<u32> {
    let mut offset = usize::try_from(from).ok()?;
    loop {
        let rest = source.get(offset..)?;
        let skipped = match whitespace_length(rest) {
            0 => comment_length(rest),
            length => length,
        };
        if skipped == 0 {
            return u32::try_from(offset).ok();
        }
        offset = offset.checked_add(skipped)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(index: &LineIndex, offset: u32) -> (u32, u32) {
        let position = index.position(offset).unwrap();
        return (position.line().get(), position.column().get());
    }

    #[test]
    fn lines_break_on_lf_crlf_and_lone_cr() {
        let index = LineIndex::new(b"a\nb\r\nc\rd").unwrap();
        assert_eq!(at(&index, 0), (1, 1), "first byte");
        assert_eq!(at(&index, 2), (2, 1), "after LF");
        assert_eq!(at(&index, 3), (2, 2), "the CR of CRLF stays on its line");
        assert_eq!(at(&index, 5), (3, 1), "after CRLF");
        assert_eq!(at(&index, 7), (4, 1), "after a lone CR");
        assert_eq!(at(&index, 8), (4, 2), "end of input is addressable");
    }

    #[test]
    fn columns_count_bytes() {
        let index = LineIndex::new("é = 1;".as_bytes()).unwrap();
        assert_eq!(at(&index, 3), (1, 4), "the two-byte é occupies two columns");
    }

    #[test]
    fn next_token_skips_whitespace_and_comments() {
        let source = b"$x /* a\n */ // b\n # c\n  ;";
        let found = next_token(source, 2).unwrap();
        assert_eq!(
            source.get(usize::try_from(found).unwrap()),
            Some(&b';'),
            "lands on the terminator"
        );
        assert_eq!(
            next_token(b"$x #[A]", 2),
            Some(3),
            "an attribute is a token, not a comment"
        );
        assert_eq!(
            next_token(b"$x // c ?>", 2),
            Some(8),
            "a line comment ends before ?>"
        );
        assert_eq!(next_token(b"$x  ", 2), Some(4), "end of input");
    }
}
