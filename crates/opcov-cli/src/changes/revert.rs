use std::fmt;

use super::{Hunk, LineKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevertError {
    OutOfOrder { new_line: u32 },
    PastEnd { new_line: u32 },
    Mismatch { new_line: u32 },
    OldStart { expected: u32, found: u32 },
}

impl fmt::Display for RevertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::OutOfOrder { new_line } => write!(
                formatter,
                "the hunk at line {new_line} overlaps or precedes the hunk before it"
            ),
            Self::PastEnd { new_line } => {
                write!(
                    formatter,
                    "the hunk at line {new_line} lies past the end of the file"
                )
            }
            Self::Mismatch { new_line } => write!(
                formatter,
                "line {new_line} differs from the diff's context or added line"
            ),
            Self::OldStart { expected, found } => write!(
                formatter,
                "a hunk starts at old line {found}, but the lines before it make it old line {expected}"
            ),
        };
    }
}

fn lines(content: &[u8]) -> Vec<&[u8]> {
    return content
        .split_inclusive(|byte| return *byte == b'\n')
        .collect();
}

fn matches(current: &[u8], text: &[u8], terminated: bool) -> bool {
    return current.strip_suffix(b"\n").map_or_else(
        || return !terminated && current == text,
        |body| return terminated && body == text,
    );
}

fn number(index: usize) -> u32 {
    return u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
}

pub(crate) fn original(current: &[u8], hunks: &[Hunk]) -> Result<Vec<u8>, RevertError> {
    let current = lines(current);
    let mut old = Vec::with_capacity(current.iter().map(|line| return line.len()).sum());
    let mut old_lines = 0_usize;
    let mut cursor = 0_usize;
    for hunk in hunks {
        let start = usize::try_from(hunk.new_first.get().saturating_sub(1)).unwrap_or(usize::MAX);
        let new_line = hunk.new_first.get();
        if start < cursor {
            return Err(RevertError::OutOfOrder { new_line });
        }
        let unchanged = current
            .get(cursor..start)
            .ok_or(RevertError::PastEnd { new_line })?;
        for line in unchanged {
            old.extend_from_slice(line);
        }
        old_lines = old_lines.saturating_add(unchanged.len());
        cursor = start;
        let expected = number(old_lines);
        if hunk.old_first.get() != expected {
            return Err(RevertError::OldStart {
                expected,
                found: hunk.old_first.get(),
            });
        }
        for line in &hunk.lines {
            if line.kind == LineKind::Removed {
                old.extend_from_slice(&line.text);
                if line.terminated {
                    old.push(b'\n');
                }
                old_lines = old_lines.saturating_add(1);
                continue;
            }
            let mismatch = RevertError::Mismatch {
                new_line: number(cursor),
            };
            let present = current.get(cursor).ok_or(mismatch)?;
            if !matches(present, &line.text, line.terminated) {
                return Err(mismatch);
            }
            if line.kind == LineKind::Context {
                old.extend_from_slice(present);
                old_lines = old_lines.saturating_add(1);
            }
            cursor = cursor.saturating_add(1);
        }
    }
    for line in current.get(cursor..).unwrap_or_default() {
        old.extend_from_slice(line);
    }
    return Ok(old);
}
