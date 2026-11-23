use std::error::Error;
use std::fmt;
use std::io::{self, Write};

use serde::Serialize;

use super::error::{Collection, ModelError};
use super::model::{
    FILES, FileCoverage, GENERATOR, Header, PATHS, PathBase, Report, SCHEMA, SETTINGS, SourcePath,
    TESTS, check_entry, schema_id,
};

#[derive(Debug)]
pub enum WriteError {
    Model(Box<ModelError>),
    Io(io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Model(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
        };
    }
}

impl Error for WriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return match self {
            Self::Model(error) => Some(&**error),
            Self::Io(error) => Some(error),
        };
    }
}

impl From<io::Error> for WriteError {
    fn from(error: io::Error) -> Self {
        return Self::Io(error);
    }
}

impl From<ModelError> for WriteError {
    fn from(error: ModelError) -> Self {
        return Self::Model(Box::new(error));
    }
}

fn member<W: Write, T: Serialize + ?Sized>(out: &mut W, name: &str, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *out, name)?;
    out.write_all(b":")?;
    serde_json::to_writer(&mut *out, value)?;
    return Ok(());
}

#[derive(Debug)]
pub struct ReportWriter<W: Write> {
    out: W,
    paths: PathBase,
    tests: usize,
    previous: Option<SourcePath>,
}

impl<W: Write> ReportWriter<W> {
    pub fn begin(mut out: W, header: &Header) -> io::Result<Self> {
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
        return Ok(Self {
            out,
            paths: header.paths.clone(),
            tests: header.tests.len(),
            previous: None,
        });
    }

    pub fn entry(&mut self, path: &SourcePath, file: &FileCoverage) -> Result<(), WriteError> {
        if let Some(previous) = &self.previous
            && previous >= path
        {
            return Err(WriteError::Model(Box::new(ModelError::NotAscending {
                collection: Collection::Files,
                previous: format!("{previous:?}"),
                next: format!("{path:?}"),
            })));
        }
        check_entry(&self.paths, self.tests, path, file)?;
        if self.previous.is_some() {
            self.out.write_all(b",")?;
        }
        member(&mut self.out, path.as_str(), file)?;
        self.previous = Some(path.clone());
        return Ok(());
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.write_all(b"}}\n")?;
        return Ok(self.out);
    }
}

impl Report {
    pub fn write<W: Write>(&self, out: W) -> Result<W, WriteError> {
        let mut writer = ReportWriter::begin(out, self.header())?;
        for (path, file) in self.files() {
            writer.entry(path, file)?;
        }
        return Ok(writer.finish()?);
    }
}
