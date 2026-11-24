use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use opcov_core::cfg::{FilePath, KeyError};
use opcov_core::report::{FileCoverage, PathBase, Sha256, SourcePath};
use opcov_syntax::{SourceFile, SyntaxError};

use crate::failure::Failure;

#[derive(Debug)]
pub(crate) enum SourceError {
    Read {
        path: PathBuf,
        error: io::Error,
    },
    Stale {
        path: PathBuf,
        recorded: Sha256,
        found: Sha256,
    },
    Key {
        path: SourcePath,
        error: KeyError,
    },
    Syntax(SyntaxError),
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Read { path, error } => {
                write!(formatter, "cannot read source {}: {error}", path.display())
            }
            Self::Stale {
                path,
                recorded,
                found,
            } => write!(
                formatter,
                "source {} changed since the report was written: the report records sha256 {recorded}, the file has {found}",
                path.display()
            ),
            Self::Key { path, error } => {
                write!(
                    formatter,
                    "cannot use {path} as a function key file: {error}"
                )
            }
            Self::Syntax(error) => write!(formatter, "{error}"),
        };
    }
}

pub(crate) fn read_verified(
    paths: &PathBase,
    path: &SourcePath,
    file: &FileCoverage,
) -> Result<Vec<u8>, SourceError> {
    let disk = paths.disk_path(path);
    let bytes = match fs::read(&disk) {
        Ok(bytes) => bytes,
        Err(error) => return Err(SourceError::Read { path: disk, error }),
    };
    let found = Sha256::digest(&bytes);
    if found != file.sha256 {
        return Err(SourceError::Stale {
            path: disk,
            recorded: file.sha256,
            found,
        });
    }
    return Ok(bytes);
}

pub(crate) fn key_file(path: &SourcePath) -> Result<FilePath, SourceError> {
    return path.as_str().parse().map_err(|error| {
        return SourceError::Key {
            path: path.clone(),
            error,
        };
    });
}

pub(crate) fn parse(path: &SourcePath, source: &[u8]) -> Result<SourceFile, SourceError> {
    return SourceFile::parse(key_file(path)?, source).map_err(SourceError::Syntax);
}

pub(crate) fn load(
    paths: &PathBase,
    path: &SourcePath,
    file: &FileCoverage,
) -> Result<SourceFile, Failure> {
    return read_verified(paths, path, file)
        .and_then(|source| return parse(path, &source))
        .map_err(|error| return Failure::failed(error.to_string()));
}
