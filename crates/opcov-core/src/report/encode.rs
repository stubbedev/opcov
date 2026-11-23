use std::io::{self, Write};

use serde::Serialize;

use super::model::{
    FILES, FileCoverage, GENERATOR, Header, PATHS, Report, SCHEMA, SETTINGS, SourcePath, TESTS,
    schema_id,
};

fn member<W: Write, T: Serialize + ?Sized>(out: &mut W, name: &str, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *out, name)?;
    out.write_all(b":")?;
    serde_json::to_writer(&mut *out, value)?;
    return Ok(());
}

pub(super) struct DocumentWriter<W: Write> {
    out: W,
    empty: bool,
}

impl<W: Write> DocumentWriter<W> {
    pub(super) fn begin(mut out: W, header: &Header) -> io::Result<Self> {
        out.write_all(b"{")?;
        member(&mut out, SCHEMA, &schema_id())?;
        out.write_all(b",")?;
        member(&mut out, GENERATOR, &header.generator)?;
        out.write_all(b",")?;
        member(&mut out, SETTINGS, &header.settings)?;
        out.write_all(b",")?;
        member(&mut out, PATHS, &header.paths)?;
        out.write_all(b",")?;
        member(&mut out, TESTS, &header.tests)?;
        out.write_all(b",")?;
        serde_json::to_writer(&mut out, FILES)?;
        out.write_all(b":{")?;
        return Ok(Self { out, empty: true });
    }

    pub(super) fn entry(&mut self, path: &SourcePath, file: &FileCoverage) -> io::Result<()> {
        if !self.empty {
            self.out.write_all(b",")?;
        }
        self.empty = false;
        return member(&mut self.out, path.as_str(), file);
    }

    pub(super) fn finish(mut self) -> io::Result<W> {
        self.out.write_all(b"}}\n")?;
        return Ok(self.out);
    }
}

impl Report {
    pub fn write<W: Write>(&self, out: W) -> io::Result<W> {
        let mut writer = DocumentWriter::begin(out, self.header())?;
        for (path, file) in self.files() {
            writer.entry(path, file)?;
        }
        return writer.finish();
    }
}
