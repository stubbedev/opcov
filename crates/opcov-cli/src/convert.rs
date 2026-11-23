use std::fmt;
use std::io::{self, Write};

use clap::ValueEnum;
use opcov_core::report::Report;

use crate::source::SourceError;
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
        };
    }
}

pub(crate) fn convert<W: Write>(
    report: &Report,
    format: Format,
    out: W,
) -> Result<W, ConvertError> {
    return match format {
        Format::Clover => Ok(clover::write(report, out)?),
        Format::Cobertura => Ok(cobertura::write(report, out)?),
        Format::Lcov => Ok(lcov::write(report, out)?),
        Format::Html => html::write(report, out),
    };
}
