use std::fmt;
use std::io::{self, Write};

#[derive(Debug)]
pub(crate) enum XmlError {
    Io(io::Error),
    Unrepresentable { character: char, text: String },
}

impl fmt::Display for XmlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Unrepresentable { character, text } => write!(
                formatter,
                "cannot write {text:?} to XML: the character U+{:04X} is not allowed in XML 1.0",
                u32::from(*character)
            ),
        };
    }
}

impl From<io::Error> for XmlError {
    fn from(error: io::Error) -> Self {
        return Self::Io(error);
    }
}

pub(crate) enum Value<'v> {
    Text(&'v str),
    Display(&'v dyn fmt::Display),
}

pub(crate) type Attribute<'v> = (&'static str, Value<'v>);

const fn allowed(character: char) -> bool {
    return matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..);
}

fn escaped(text: &str, attribute: bool) -> Result<String, XmlError> {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if !allowed(character) {
            return Err(XmlError::Unrepresentable {
                character,
                text: text.to_owned(),
            });
        }
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\'' if attribute => out.push_str("&apos;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            other => out.push(other),
        }
    }
    return Ok(out);
}

#[derive(Debug)]
pub(crate) struct XmlWriter<W: Write> {
    out: W,
    depth: usize,
}

impl<W: Write> XmlWriter<W> {
    pub(crate) fn begin(mut out: W, doctype: Option<&str>) -> Result<Self, XmlError> {
        out.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        if let Some(doctype) = doctype {
            writeln!(out, "{doctype}")?;
        }
        return Ok(Self { out, depth: 0 });
    }

    pub(crate) const fn fragment(out: W, depth: usize) -> Self {
        return Self { out, depth };
    }

    pub(crate) const fn depth(&self) -> usize {
        return self.depth;
    }

    pub(crate) fn finish(self) -> W {
        return self.out;
    }

    pub(crate) fn raw(&mut self, bytes: &[u8]) -> Result<(), XmlError> {
        self.out.write_all(bytes)?;
        return Ok(());
    }

    fn indent(&mut self) -> Result<(), XmlError> {
        for _ in 0..self.depth {
            self.out.write_all(b"  ")?;
        }
        return Ok(());
    }

    fn tag(&mut self, name: &str, attributes: &[Attribute<'_>]) -> Result<(), XmlError> {
        self.indent()?;
        write!(self.out, "<{name}")?;
        for (key, value) in attributes {
            let text = match value {
                Value::Text(text) => escaped(text, true)?,
                Value::Display(shown) => escaped(&shown.to_string(), true)?,
            };
            write!(self.out, " {key}=\"{text}\"")?;
        }
        return Ok(());
    }

    pub(crate) fn open(
        &mut self,
        name: &'static str,
        attributes: &[Attribute<'_>],
    ) -> Result<(), XmlError> {
        self.tag(name, attributes)?;
        self.out.write_all(b">\n")?;
        self.depth = self.depth.saturating_add(1);
        return Ok(());
    }

    pub(crate) fn close(&mut self, name: &'static str) -> Result<(), XmlError> {
        self.depth = self.depth.saturating_sub(1);
        self.indent()?;
        writeln!(self.out, "</{name}>")?;
        return Ok(());
    }

    pub(crate) fn element(
        &mut self,
        name: &'static str,
        attributes: &[Attribute<'_>],
        body: impl FnOnce(&mut Self) -> Result<(), XmlError>,
    ) -> Result<(), XmlError> {
        self.open(name, attributes)?;
        body(self)?;
        return self.close(name);
    }

    pub(crate) fn empty(
        &mut self,
        name: &'static str,
        attributes: &[Attribute<'_>],
    ) -> Result<(), XmlError> {
        self.tag(name, attributes)?;
        self.out.write_all(b"/>\n")?;
        return Ok(());
    }

    pub(crate) fn text_element(&mut self, name: &'static str, text: &str) -> Result<(), XmlError> {
        self.indent()?;
        let text = escaped(text, false)?;
        writeln!(self.out, "<{name}>{text}</{name}>")?;
        return Ok(());
    }
}
