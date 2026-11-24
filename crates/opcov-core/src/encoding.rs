use std::borrow::Cow;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

const ESCAPE: char = '%';
const ESCAPED_ESCAPE: &str = "%25";
const HEX: &str = "0123456789ABCDEF";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCanonical {
    text: String,
}

impl NotCanonical {
    #[must_use]
    pub fn text(&self) -> &str {
        return &self.text;
    }
}

impl fmt::Display for NotCanonical {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "{:?} is not in canonical byte encoding: every `%` must start `%25` or the `%XX` escape, in uppercase hex, of a byte that is not part of valid UTF-8",
            self.text
        );
    }
}

impl Error for NotCanonical {}

#[must_use]
pub fn encode(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(text) = str::from_utf8(bytes)
        && !text.contains(ESCAPE)
    {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(bytes.len().saturating_add(bytes.len() / 2));
    for chunk in bytes.utf8_chunks() {
        for (position, piece) in chunk.valid().split(ESCAPE).enumerate() {
            if position > 0 {
                out.push_str(ESCAPED_ESCAPE);
            }
            out.push_str(piece);
        }
        for &byte in chunk.invalid() {
            out.push(ESCAPE);
            out.extend(HEX.chars().nth(usize::from(byte >> 4)));
            out.extend(HEX.chars().nth(usize::from(byte & 0x0F)));
        }
    }
    return Cow::Owned(out);
}

#[must_use]
pub fn encode_path(path: &Path) -> Cow<'_, str> {
    return encode(path.as_os_str().as_bytes());
}

fn escaped_byte(high: u8, low: u8) -> Option<u8> {
    if !high.is_ascii_hexdigit() || !low.is_ascii_hexdigit() {
        return None;
    }
    return u8::from_str_radix(str::from_utf8(&[high, low]).ok()?, 16).ok();
}

pub(crate) fn unescape(text: &str) -> Cow<'_, [u8]> {
    if !text.contains(ESCAPE) {
        return Cow::Borrowed(text.as_bytes());
    }
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if char::from(byte) == ESCAPE
            && let Some((&[high, low], after)) = tail.split_first_chunk::<2>()
            && let Some(escaped) = escaped_byte(high, low)
        {
            out.push(escaped);
            rest = after;
            continue;
        }
        out.push(byte);
        rest = tail;
    }
    return Cow::Owned(out);
}

#[expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]
pub fn decode(text: &str) -> Result<Cow<'_, [u8]>, NotCanonical> {
    let bytes = unescape(text);
    if encode(&bytes) != text {
        return Err(NotCanonical {
            text: text.to_owned(),
        });
    }
    return Ok(bytes);
}

pub(crate) fn path_of(text: &str) -> PathBuf {
    return PathBuf::from(OsStr::from_bytes(&unescape(text)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_without_the_escape_character_is_its_own_encoding() {
        for text in [
            "",
            "/srv/app/Foo.php",
            "ünïcode_fn",
            "App\\Foo::bar",
            "a:1-2#3",
        ] {
            assert!(
                matches!(encode(text.as_bytes()), Cow::Borrowed(same) if same == text),
                "{text:?} encodes to itself without copying"
            );
            assert_eq!(decode(text).unwrap(), text.as_bytes(), "{text:?} decodes");
        }
    }

    #[test]
    fn invalid_bytes_and_the_escape_character_are_escaped() {
        let cases: [(&[u8], &str); 6] = [
            (b"Caf\xe9", "Caf%E9"),
            (b"100%.php", "100%25.php"),
            (b"%E9", "%25E9"),
            (b"\xc3\xa9\xe9", "\u{e9}%E9"),
            (b"\xe2\x82", "%E2%82"),
            (b"\xff/\x80%", "%FF/%80%25"),
        ];
        for (bytes, text) in cases {
            assert_eq!(encode(bytes), text, "encoding of {bytes:?}");
            assert_eq!(decode(text).unwrap(), bytes, "decoding of {text:?}");
        }
    }

    #[test]
    fn non_canonical_escapes_are_rejected() {
        for text in [
            "%", "%4", "%e9", "%41", "%2", "a%zz", "%C3%A9", "%+F", "%%", "%0A",
        ] {
            assert_eq!(
                decode(text).unwrap_err().text(),
                text,
                "{text:?} is not canonical"
            );
        }
    }
}
