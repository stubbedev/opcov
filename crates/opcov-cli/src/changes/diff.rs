use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

use opcov_core::cfg::Line;
use opcov_core::encoding;

use super::ChangeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    Context,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HunkLine {
    pub(crate) kind: LineKind,
    pub(crate) text: Vec<u8>,
    pub(crate) terminated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub(crate) old_first: Line,
    pub(crate) new_first: Line,
    pub(crate) lines: Vec<HunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Content {
    Text(Vec<Hunk>),
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilePatch {
    pub(crate) old: Option<PathBuf>,
    pub(crate) new: Option<PathBuf>,
    pub(crate) content: Content,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Patch(Vec<FilePatch>);

impl Patch {
    pub(crate) fn files(&self) -> &[FilePatch] {
        return &self.0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Side {
    Null,
    Path(PathBuf),
}

#[derive(Debug, Default)]
struct Block {
    created: bool,
    deleted: bool,
    header: Option<PathBuf>,
    renamed_from: Option<PathBuf>,
    renamed_to: Option<PathBuf>,
    copied_to: Option<PathBuf>,
    minus: Option<Side>,
    plus: Option<Side>,
    binary: bool,
    hunks: Vec<Hunk>,
}

impl Block {
    fn into_patch(self) -> Option<FilePatch> {
        let path = |side: Option<Side>| {
            return match side {
                Some(Side::Path(path)) => Some(path),
                Some(Side::Null) | None => None,
            };
        };
        let old = if self.created || self.minus == Some(Side::Null) {
            None
        } else {
            self.renamed_from
                .or_else(|| return path(self.minus))
                .or_else(|| return self.header.clone())
        };
        let new = if self.deleted || self.plus == Some(Side::Null) {
            None
        } else {
            self.renamed_to
                .or(self.copied_to)
                .or_else(|| return path(self.plus))
                .or(self.header)
        };
        let content = if self.binary {
            Content::Binary
        } else {
            Content::Text(self.hunks)
        };
        let unchanged = old.is_some() && old == new && content == Content::Text(Vec::new());
        if unchanged || (old.is_none() && new.is_none()) {
            return None;
        }
        return Some(FilePatch { old, new, content });
    }
}

#[derive(Debug)]
struct Open {
    hunk: Hunk,
    old_left: u32,
    new_left: u32,
}

struct Parser<'s> {
    source: &'s str,
    strip: usize,
    line: usize,
    block: Option<Block>,
    open: Option<Open>,
    files: Vec<FilePatch>,
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

fn first_line(start: u32, length: u32) -> Option<Line> {
    if length == 0 {
        return Line::new(start.checked_add(1)?);
    }
    return Line::new(start);
}

fn octal(digits: &[u8]) -> Option<u8> {
    return digits.iter().try_fold(0_u8, |value, digit| {
        let digit = digit.checked_sub(b'0').filter(|digit| return *digit < 8)?;
        return value.checked_mul(8)?.checked_add(digit);
    });
}

fn unquote(quoted: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut out = Vec::with_capacity(quoted.len());
    let mut rest = quoted;
    loop {
        match rest {
            [b'"', tail @ ..] => return Some((out, tail)),
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

fn without_cr(raw: &[u8]) -> &[u8] {
    return raw.strip_suffix(b"\r").unwrap_or(raw);
}

fn is_null(raw: &[u8]) -> bool {
    let raw = without_cr(raw);
    return raw == b"/dev/null" || raw.starts_with(b"/dev/null\t");
}

fn before_tab(raw: &[u8]) -> &[u8] {
    return raw
        .split(|byte| return *byte == b'\t')
        .next()
        .unwrap_or_default();
}

fn pair(raw: &[u8], separator: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let raw = without_cr(raw);
    if let Some(quoted) = raw.strip_prefix(b"\"") {
        let (left, rest) = unquote(quoted)?;
        let rest = rest.strip_prefix(separator)?;
        let right = match rest.strip_prefix(b"\"") {
            Some(quoted) => unquote(quoted)?.0,
            None => rest.to_vec(),
        };
        return Some((left, right));
    }
    if let Some(head) = raw.strip_suffix(b"\"") {
        let start = head.iter().rposition(|byte| return *byte == b'"')?;
        let left = head.get(..start)?.strip_suffix(separator)?;
        let (right, _) = unquote(head.get(start.checked_add(1)?..)?)?;
        return Some((left.to_vec(), right));
    }
    let half = raw.len().checked_sub(separator.len())? / 2;
    let left = raw.get(..half)?;
    let right = raw.get(half..)?.strip_prefix(separator)?;
    return Some((left.to_vec(), right.to_vec()));
}

impl Parser<'_> {
    fn fail(&self, message: impl Into<String>) -> ChangeError {
        return ChangeError {
            source: self.source.to_owned(),
            line: self.line,
            message: message.into(),
        };
    }

    fn finish_block(&mut self) {
        if let Some(patch) = self.block.take().and_then(Block::into_patch) {
            self.files.push(patch);
        }
    }

    fn start_block(&mut self) -> &mut Block {
        self.finish_block();
        return self.block.insert(Block::default());
    }

    fn block(&mut self) -> &mut Block {
        return self.block.get_or_insert_with(Block::default);
    }

    fn decode(&self, raw: &[u8], plain: fn(&[u8]) -> &[u8]) -> Result<Vec<u8>, ChangeError> {
        let raw = without_cr(raw);
        return raw.strip_prefix(b"\"").map_or_else(
            || return Ok(plain(raw).to_vec()),
            |quoted| {
                return unquote(quoted)
                    .map(|(path, _)| return path)
                    .ok_or_else(|| return self.fail("unterminated or invalid quoted path"));
            },
        );
    }

    fn stripped(&self, bytes: &[u8]) -> Result<PathBuf, ChangeError> {
        let shown = encoding::encode(bytes);
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

    fn side(&self, raw: &[u8]) -> Result<Side, ChangeError> {
        if is_null(raw) {
            return Ok(Side::Null);
        }
        return Ok(Side::Path(self.stripped(&self.decode(raw, before_tab)?)?));
    }

    fn unstripped(&self, raw: &[u8]) -> Result<PathBuf, ChangeError> {
        let bytes = self.decode(raw, |raw| return raw)?;
        return Ok(PathBuf::from(OsStr::from_bytes(&bytes)));
    }

    fn header(&mut self, raw: &[u8]) {
        let header = match pair(raw, b" ") {
            Some((left, right)) => match (self.stripped(&left), self.stripped(&right)) {
                (Ok(left), Ok(right)) if left == right => Some(left),
                _ => None,
            },
            None => None,
        };
        self.start_block().header = header;
    }

    fn binary(&mut self, raw: &[u8]) -> Result<(), ChangeError> {
        let described = without_cr(raw)
            .strip_suffix(b" differ")
            .ok_or_else(|| return self.fail("malformed `Binary files` line"))?;
        let (minus, plus) = if let Some(plus) = described.strip_prefix(b"/dev/null and ") {
            (Side::Null, self.side(plus)?)
        } else if let Some(minus) = described.strip_suffix(b" and /dev/null") {
            (self.side(minus)?, Side::Null)
        } else {
            let (minus, plus) = pair(described, b" and ")
                .ok_or_else(|| return self.fail("malformed `Binary files` line"))?;
            (
                Side::Path(self.stripped(&minus)?),
                Side::Path(self.stripped(&plus)?),
            )
        };
        let block = self.block();
        block.binary = true;
        block.minus.get_or_insert(minus);
        block.plus.get_or_insert(plus);
        return Ok(());
    }

    fn minus(&mut self, raw: &[u8]) -> Result<(), ChangeError> {
        let side = self.side(raw)?;
        let reopen = self
            .block
            .as_ref()
            .is_none_or(|block| return block.minus.is_some() || !block.hunks.is_empty());
        let block = if reopen {
            self.start_block()
        } else {
            self.block()
        };
        block.minus = Some(side);
        return Ok(());
    }

    fn plus(&mut self, raw: &[u8]) -> Result<(), ChangeError> {
        let side = self.side(raw)?;
        self.block().plus = Some(side);
        return Ok(());
    }

    fn hunk_header(&mut self, text: &[u8]) -> Result<(), ChangeError> {
        let invalid = || return self.fail("malformed hunk header");
        let text = std::str::from_utf8(text).map_err(|_utf8| return invalid())?;
        let mut parts = text.split(' ');
        let (Some("@@"), Some(old), Some(new)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(invalid());
        };
        let ((old_start, old_left), (new_start, new_left)) = (
            side(old, '-').ok_or_else(invalid)?,
            side(new, '+').ok_or_else(invalid)?,
        );
        if self
            .block
            .as_ref()
            .is_none_or(|block| return block.plus.is_none())
        {
            return Err(self.fail("hunk before a `+++` file header"));
        }
        let at_zero = || return self.fail("hunk refers to line 0");
        self.open = Some(Open {
            hunk: Hunk {
                old_first: first_line(old_start, old_left).ok_or_else(at_zero)?,
                new_first: first_line(new_start, new_left).ok_or_else(at_zero)?,
                lines: Vec::new(),
            },
            old_left,
            new_left,
        });
        return Ok(());
    }

    fn body(&mut self, mut open: Open, line: &[u8]) -> Result<(), ChangeError> {
        let (kind, text) = match line.split_first() {
            Some((b'\\', _)) => {
                let last = open
                    .hunk
                    .lines
                    .last_mut()
                    .ok_or_else(|| return self.fail("no-newline marker before any line"))?;
                last.terminated = false;
                self.open = Some(open);
                return Ok(());
            }
            Some((b'+', text)) if open.new_left > 0 => {
                open.new_left = open.new_left.saturating_sub(1);
                (LineKind::Added, text)
            }
            Some((b'-', text)) if open.old_left > 0 => {
                open.old_left = open.old_left.saturating_sub(1);
                (LineKind::Removed, text)
            }
            Some((b' ', text)) if open.old_left > 0 && open.new_left > 0 => {
                open.old_left = open.old_left.saturating_sub(1);
                open.new_left = open.new_left.saturating_sub(1);
                (LineKind::Context, text)
            }
            None if open.old_left > 0 && open.new_left > 0 => {
                open.old_left = open.old_left.saturating_sub(1);
                open.new_left = open.new_left.saturating_sub(1);
                (LineKind::Context, &b""[..])
            }
            _ => return Err(self.fail("line does not fit the hunk's line counts")),
        };
        open.hunk.lines.push(HunkLine {
            kind,
            text: text.to_vec(),
            terminated: true,
        });
        self.open = Some(open);
        return Ok(());
    }

    fn close_hunk(&mut self) {
        if let Some(open) = self.open.take() {
            self.block().hunks.push(open.hunk);
        }
    }

    fn feed(&mut self, line: &[u8]) -> Result<(), ChangeError> {
        if let Some(open) = self.open.take() {
            if open.old_left > 0 || open.new_left > 0 || line.starts_with(b"\\") {
                return self.body(open, line);
            }
            self.block().hunks.push(open.hunk);
        }
        if let Some(raw) = line.strip_prefix(b"diff --git ") {
            self.header(raw);
            return Ok(());
        }
        if line.starts_with(b"diff ") {
            self.start_block();
            return Ok(());
        }
        if let Some(raw) = line.strip_prefix(b"--- ") {
            return self.minus(raw);
        }
        if let Some(raw) = line.strip_prefix(b"+++ ") {
            return self.plus(raw);
        }
        if line.starts_with(b"@@ ") {
            return self.hunk_header(line);
        }
        if let Some(raw) = line.strip_prefix(b"Binary files ") {
            return self.binary(raw);
        }
        if without_cr(line) == b"GIT binary patch" {
            self.block().binary = true;
            return Ok(());
        }
        if line.starts_with(b"new file mode ") {
            self.block().created = true;
        } else if line.starts_with(b"deleted file mode ") {
            self.block().deleted = true;
        } else if let Some(raw) = line.strip_prefix(b"rename from ") {
            let path = self.unstripped(raw)?;
            self.block().renamed_from = Some(path);
        } else if let Some(raw) = line.strip_prefix(b"rename to ") {
            let path = self.unstripped(raw)?;
            self.block().renamed_to = Some(path);
        } else if let Some(raw) = line.strip_prefix(b"copy to ") {
            let path = self.unstripped(raw)?;
            let block = self.block();
            block.created = true;
            block.copied_to = Some(path);
        }
        return Ok(());
    }
}

pub(crate) fn parse_diff(source: &str, input: &[u8], strip: usize) -> Result<Patch, ChangeError> {
    let mut parser = Parser {
        source,
        strip,
        line: 0,
        block: None,
        open: None,
        files: Vec::new(),
    };
    let body = input.strip_suffix(b"\n").unwrap_or(input);
    if !body.is_empty() {
        for line in body.split(|byte| return *byte == b'\n') {
            parser.line = parser.line.saturating_add(1);
            parser.feed(line)?;
        }
    }
    if parser
        .open
        .as_ref()
        .is_some_and(|open| return open.old_left > 0 || open.new_left > 0)
    {
        return Err(parser.fail("the diff ends inside a hunk"));
    }
    parser.close_hunk();
    parser.finish_block();
    return Ok(Patch(parser.files));
}
