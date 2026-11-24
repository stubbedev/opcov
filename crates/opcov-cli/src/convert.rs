use std::fmt;
use std::io::{self, Write};
use std::path::Path;

use clap::ValueEnum;
use opcov_core::report::{FileCoverage, Header};

use crate::failure::Failure;
use crate::source::SourceError;
use crate::stream;
use crate::view::{FileView, file_view};
use crate::xml::XmlError;

mod clover;
mod cobertura;
mod html;
mod lcov;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    Clover,
    Cobertura,
    Lcov,
    Html,
}

#[derive(Debug)]
pub(crate) enum ConvertError {
    Io(io::Error),
    Xml(XmlError),
    Source(SourceError),
    Input(Failure),
}

impl From<io::Error> for ConvertError {
    fn from(error: io::Error) -> Self {
        return Self::Io(error);
    }
}

impl From<XmlError> for ConvertError {
    fn from(error: XmlError) -> Self {
        return match error {
            XmlError::Io(error) => Self::Io(error),
            unrepresentable @ XmlError::Unrepresentable { .. } => Self::Xml(unrepresentable),
        };
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Xml(error) => write!(formatter, "{error}"),
            Self::Source(error) => write!(formatter, "{error}"),
            Self::Input(failure) => write!(formatter, "{failure}"),
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Context<'a> {
    source: &'a Path,
    header: &'a Header,
    format: Format,
}

impl Context<'_> {
    pub(crate) fn fail(&self, error: ConvertError) -> Failure {
        return match error {
            ConvertError::Input(failure) => failure,
            other => Failure::failed(format!(
                "cannot convert {} to {:?}: {other}",
                self.source.display(),
                self.format
            )),
        };
    }

    fn changed(&self) -> ConvertError {
        return ConvertError::Input(Failure::failed(format!(
            "cannot convert {}: the report changed while it was read",
            self.source.display()
        )));
    }

    const fn header(&self) -> &Header {
        return self.header;
    }

    fn each<T: Send>(
        &self,
        work: &(impl Fn(usize, &FileView<'_>, &FileCoverage) -> Result<T, ConvertError> + Sync),
        emit: &mut dyn FnMut(T) -> Result<(), ConvertError>,
    ) -> Result<(), ConvertError> {
        let paths = &self.header.paths;
        return stream::entries(
            self.source,
            self.header,
            &stream::source_files,
            &|sequence, path, file| {
                let view = file_view(paths, &path, &file);
                return work(sequence, &view, &file).map_err(|error| return self.fail(error));
            },
            &mut |value| return emit(value).map_err(|error| return self.fail(error)),
        )
        .map_err(ConvertError::Input);
    }
}

pub(crate) fn convert<W: Write>(
    source: &Path,
    header: &Header,
    format: Format,
    out: W,
) -> Result<W, Failure> {
    let context = Context {
        source,
        header,
        format,
    };
    let written = match format {
        Format::Clover => clover::write(&context, out),
        Format::Cobertura => cobertura::write(&context, out),
        Format::Lcov => lcov::write(&context, out),
        Format::Html => html::write(&context, out),
    };
    return written.map_err(|error| return context.fail(error));
}
