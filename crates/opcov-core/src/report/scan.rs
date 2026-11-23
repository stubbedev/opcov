use std::fmt;
use std::io::{self, Read};

use super::error::ReadError;

const CHUNK: usize = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Byte(u8),
    End,
}

impl fmt::Display for Found {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Byte(byte) if byte.is_ascii_graphic() => {
                write!(formatter, "`{}`", char::from(*byte))
            }
            Self::Byte(byte) => write!(formatter, "byte 0x{byte:02x}"),
            Self::End => formatter.write_str("the end of the input"),
        };
    }
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Container {
        depth: u32,
        in_string: bool,
        escaped: bool,
    },
    Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Continue,
    Include,
    Exclude,
}

const fn is_whitespace(byte: u8) -> bool {
    return matches!(byte, b' ' | b'\t' | b'\n' | b'\r');
}

const fn structural(byte: u8) -> bool {
    return matches!(byte, b'"' | b'\\' | b'{' | b'}' | b'[' | b']');
}

const fn delimiter(byte: u8) -> bool {
    return is_whitespace(byte) || matches!(byte, b',' | b'}' | b']');
}

impl Mode {
    const fn marked(self, byte: u8) -> bool {
        return match self {
            Self::Container { escaped: true, .. } => true,
            Self::Container { .. } => structural(byte),
            Self::Scalar => delimiter(byte),
        };
    }

    const fn step(&mut self, byte: u8) -> Step {
        let Self::Container {
            depth,
            in_string,
            escaped,
        } = self
        else {
            return if delimiter(byte) {
                Step::Exclude
            } else {
                Step::Continue
            };
        };
        if *in_string {
            if *escaped {
                *escaped = false;
            } else if byte == b'\\' {
                *escaped = true;
            } else if byte == b'"' {
                *in_string = false;
                if *depth == 0 {
                    return Step::Include;
                }
            }
            return Step::Continue;
        }
        match byte {
            b'"' => *in_string = true,
            b'{' | b'[' => *depth = depth.saturating_add(1),
            b'}' | b']' => {
                *depth = depth.saturating_sub(1);
                if *depth == 0 {
                    return Step::Include;
                }
            }
            _ => {}
        }
        return Step::Continue;
    }
}

#[derive(Debug)]
pub(super) struct Scanner<R> {
    reader: R,
    buffer: Vec<u8>,
    start: usize,
    end: usize,
    consumed: u64,
}

impl<R: Read> Scanner<R> {
    pub(super) fn new(reader: R) -> Self {
        return Self {
            reader,
            buffer: vec![0; CHUNK],
            start: 0,
            end: 0,
            consumed: 0,
        };
    }

    pub(super) fn offset(&self) -> u64 {
        return self
            .consumed
            .saturating_add(u64::try_from(self.start).unwrap_or(u64::MAX));
    }

    fn fill(&mut self) -> Result<bool, ReadError> {
        if self.start < self.end {
            return Ok(true);
        }
        self.consumed = self
            .consumed
            .saturating_add(u64::try_from(self.end).unwrap_or(u64::MAX));
        self.start = 0;
        self.end = 0;
        loop {
            match self.reader.read(&mut self.buffer) {
                Ok(read) => {
                    self.end = read;
                    return Ok(read > 0);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(ReadError::Io(error)),
            }
        }
    }

    fn available(&self) -> &[u8] {
        return self.buffer.get(self.start..self.end).unwrap_or_default();
    }

    fn advance(&mut self, count: usize) {
        self.start = self.start.saturating_add(count).min(self.end);
    }

    pub(super) fn peek(&mut self) -> Result<Found, ReadError> {
        loop {
            if !self.fill()? {
                return Ok(Found::End);
            }
            let skipped = self
                .available()
                .iter()
                .position(|&byte| return !is_whitespace(byte));
            match skipped {
                Some(at) => {
                    self.advance(at);
                    return Ok(self
                        .available()
                        .first()
                        .copied()
                        .map_or(Found::End, Found::Byte));
                }
                None => self.advance(self.end),
            }
        }
    }

    pub(super) fn unexpected(&self, expected: &'static str, found: Found) -> ReadError {
        return ReadError::Syntax {
            offset: self.offset(),
            expected,
            found,
        };
    }

    pub(super) fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), ReadError> {
        let found = self.peek()?;
        if found != Found::Byte(byte) {
            return Err(self.unexpected(expected, found));
        }
        self.advance(1);
        return Ok(());
    }

    pub(super) fn string(&mut self, out: &mut Vec<u8>) -> Result<(), ReadError> {
        let found = self.peek()?;
        if found != Found::Byte(b'"') {
            return Err(self.unexpected("a string", found));
        }
        return self.capture(
            out,
            Mode::Container {
                depth: 0,
                in_string: false,
                escaped: false,
            },
        );
    }

    pub(super) fn value(&mut self, out: &mut Vec<u8>) -> Result<(), ReadError> {
        let mode = match self.peek()? {
            Found::Byte(b'{' | b'[' | b'"') => Mode::Container {
                depth: 0,
                in_string: false,
                escaped: false,
            },
            Found::Byte(b'}' | b']' | b',' | b':') | Found::End => {
                let found = self.peek()?;
                return Err(self.unexpected("a value", found));
            }
            Found::Byte(_) => Mode::Scalar,
        };
        return self.capture(out, mode);
    }

    fn capture(&mut self, out: &mut Vec<u8>, mut mode: Mode) -> Result<(), ReadError> {
        out.clear();
        loop {
            if !self.fill()? {
                return match mode {
                    Mode::Scalar if !out.is_empty() => Ok(()),
                    Mode::Scalar | Mode::Container { .. } => {
                        Err(self.unexpected("the end of the value", Found::End))
                    }
                };
            }
            let chunk = self.available();
            let mut taken = chunk.len();
            let mut finished = false;
            for (at, &byte) in chunk.iter().enumerate() {
                if !mode.marked(byte) {
                    continue;
                }
                match mode.step(byte) {
                    Step::Continue => {}
                    Step::Include => {
                        taken = at.saturating_add(1);
                        finished = true;
                        break;
                    }
                    Step::Exclude => {
                        taken = at;
                        finished = true;
                        break;
                    }
                }
            }
            out.extend_from_slice(chunk.get(..taken).unwrap_or_default());
            self.advance(taken);
            if finished {
                return Ok(());
            }
        }
    }

    pub(super) fn end(&mut self) -> Result<(), ReadError> {
        let found = self.peek()?;
        if found != Found::End {
            return Err(self.unexpected("the end of the input", found));
        }
        return Ok(());
    }
}
