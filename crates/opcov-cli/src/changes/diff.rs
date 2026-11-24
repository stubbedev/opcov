use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

use opcov_core::cfg::Line;
use opcov_core::encoding;

use super::{Change, ChangeError, ChangeSet, FileChange, LineRange};

#[derive(Debug, Clone, Copy)]
enum Block {
    Unchanged,
    Removed { before: Line },
    Added,
}

#[derive(Debug, Clone, Copy)]
struct Hunk {
    old: u32,
    new: u32,
    next: u32,
    block: Block,
}

enum Target {
    None,
    Deleted,
    File(PathBuf),
}

struct Parser<'s> {
    source: &'s str,
    strip: usize,
    line: usize,
    target: Target,
    changes: BTreeSet<Change>,
    hunk: Option<Hunk>,
    set: ChangeSet,
}

fn count(text: &str) -> Option<u32> {
    let canonical = !text.is_empty() && text.bytes().all(|byte| return byte.is_ascii_digit());
    return canonical.then(|| return text.parse().ok()).flatten();
}

fn side(text: &str, sign: char) -> Option<(u32, u32)> {
    let range = text.strip_prefix(sign)?;
    return match range.split_once(',') {
        Some((start, length)) => Some((count(start)?, count(length)?)),
        None => Some((count(range)?, 1)),
    };
}

fn octal(digits: &[u8]) -> Option<u8> {
    return digits.iter().try_fold(0_u8, |value, digit| {
        let digit = digit.checked_sub(b'0').filter(|digit| return *digit < 8)?;
        return value.checked_mul(8)?.checked_add(digit);
    });
}

fn unquote(quoted: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(quoted.len());
    let mut rest = quoted;
    loop {
        match rest {
            [b'"', ..] => return Some(out),
            [b'\\', b'0'..=b'7', b'0'..=b'7', b'0'..=b'7', tail @ ..] => {
                out.push(octal(rest.get(1..4)?)?);
                rest = tail;
            }
            [b'\\', escape, tail @ ..] => {
                out.push(match escape {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 0x0b,
                    b'"' | b'\\' => *escape,
                    _ => return None,
                });
                rest = tail;
            }
            [byte, tail @ ..] => {
                out.push(*byte);
                rest = tail;
            }
            [] => return None,
        }
    }
}

impl Parser<'_> {
    fn fail(&self, message: impl Into<String>) -> ChangeError {
        return ChangeError {
            source: self.source.to_owned(),
            line: self.line,
            message: message.into(),
        };
    }

    fn finish_file(&mut self) {
        let changes = std::mem::take(&mut self.changes);
        if let Target::File(path) = std::mem::replace(&mut self.target, Target::None)
            && !changes.is_empty()
        {
            self.set.add(path, FileChange::Parts(changes));
        }
    }

    fn path(&self, raw: &[u8]) -> Result<PathBuf, ChangeError> {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let bytes = match raw.strip_prefix(b"\"") {
            Some(quoted) => unquote(quoted)
                .ok_or_else(|| return self.fail("unterminated or invalid quoted path"))?,
            None => raw
                .split(|byte| return *byte == b'\t')
                .next()
                .unwrap_or_default()
                .to_vec(),
        };
        let shown = encoding::encode(&bytes);
        let mut components = bytes.split(|&byte| return byte == b'/');
        for _ in 0..self.strip {
            if components.next().is_none() {
                return Err(self.fail(format!(
                    "cannot strip {} leading components from {shown:?}",
                    self.strip
                )));
            }
        }
        let stripped: Vec<&[u8]> = components.collect();
        if stripped.iter().all(|component| return component.is_empty()) {
            return Err(self.fail(format!(
                "stripping {} leading components leaves no path in {shown:?}",
                self.strip
            )));
        }
        return Ok(PathBuf::from(OsStr::from_bytes(&stripped.join(&b'/'))));
    }

    fn new_side(&mut self, raw: &[u8]) -> Result<(), ChangeError> {
        self.finish_file();
        let trimmed = raw.strip_suffix(b"\r").unwrap_or(raw);
        let is_null = trimmed == b"/dev/null" || trimmed.starts_with(b"/dev/null\t");
        self.target = if is_null {
            Target::Deleted
        } else {
            Target::File(self.path(raw)?)
        };
        return Ok(());
    }

    fn hunk_header(&mut self, text: &[u8]) -> Result<(), ChangeError> {
        let invalid = || return self.fail("malformed hunk header");
        let text = std::str::from_utf8(text).map_err(|_utf8| return invalid())?;
        let mut parts = text.split(' ');
        let (Some("@@"), Some(old), Some(new)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(invalid());
        };
        let ((_, old), (start, new)) = (
            side(old, '-').ok_or_else(invalid)?,
            side(new, '+').ok_or_else(invalid)?,
        );
        if matches!(self.target, Target::None) {
            return Err(self.fail("hunk before a `+++` file header"));
        }
        let next = if new == 0 {
            start.saturating_add(1)
        } else {
            start
        };
        self.hunk = Some(Hunk {
            old,
            new,
            next,
            block: Block::Unchanged,
        });
        return Ok(());
    }

    fn line_at(&self, number: u32) -> Result<Line, ChangeError> {
        return Line::new(number).ok_or_else(|| return self.fail("hunk refers to line 0"));
    }

    fn close_block(&mut self, block: Block) {
        if let Block::Removed { before } = block
            && !matches!(self.target, Target::Deleted)
        {
            self.changes.insert(Change::Deletion { before });
        }
    }

    fn body(&mut self, mut hunk: Hunk, line: &[u8]) -> Result<(), ChangeError> {
        match line.first() {
            Some(b'\\') => return Ok(()),
            Some(b'+') if hunk.new > 0 => {
                if !matches!(self.target, Target::Deleted) {
                    let at = self.line_at(hunk.next)?;
                    self.changes.insert(Change::Lines(LineRange::single(at)));
                }
                hunk.block = Block::Added;
                hunk.new = hunk.new.saturating_sub(1);
                hunk.next = hunk.next.saturating_add(1);
            }
            Some(b'-') if hunk.old > 0 => {
                if matches!(hunk.block, Block::Unchanged) {
                    hunk.block = Block::Removed {
                        before: self.line_at(hunk.next)?,
                    };
                }
                hunk.old = hunk.old.saturating_sub(1);
            }
            Some(b' ') | None if hunk.old > 0 && hunk.new > 0 => {
                self.close_block(hunk.block);
                hunk.block = Block::Unchanged;
                hunk.old = hunk.old.saturating_sub(1);
                hunk.new = hunk.new.saturating_sub(1);
                hunk.next = hunk.next.saturating_add(1);
            }
            _ => return Err(self.fail("line does not fit the hunk's line counts")),
        }
        if hunk.old > 0 || hunk.new > 0 {
            self.hunk = Some(hunk);
        } else {
            self.close_block(hunk.block);
            self.hunk = None;
        }
        return Ok(());
    }

    fn feed(&mut self, line: &[u8]) -> Result<(), ChangeError> {
        if let Some(hunk) = self.hunk {
            return self.body(hunk, line);
        }
        if let Some(raw) = line.strip_prefix(b"+++ ") {
            return self.new_side(raw);
        }
        if line.starts_with(b"@@ ") {
            return self.hunk_header(line);
        }
        if line.starts_with(b"diff ") || line.starts_with(b"--- ") {
            self.finish_file();
        }
        return Ok(());
    }
}

pub(crate) fn parse_diff(
    source: &str,
    input: &[u8],
    strip: usize,
) -> Result<ChangeSet, ChangeError> {
    let mut parser = Parser {
        source,
        strip,
        line: 0,
        target: Target::None,
        changes: BTreeSet::new(),
        hunk: None,
        set: ChangeSet::default(),
    };
    let body = input.strip_suffix(b"\n").unwrap_or(input);
    if !body.is_empty() {
        for line in body.split(|byte| return *byte == b'\n') {
            parser.line = parser.line.saturating_add(1);
            parser.feed(line)?;
        }
    }
    if parser.hunk.is_some() {
        return Err(parser.fail("the diff ends inside a hunk"));
    }
    parser.finish_file();
    return Ok(parser.set);
}
