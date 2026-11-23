#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Create,
    Write,
    Sync,
    Rename,
    Remove,
}

#[derive(Debug)]
pub struct IoFailure {
    step: Step,
    path: PathBuf,
    target: PathBuf,
    source: io::Error,
}

impl IoFailure {
    #[must_use]
    pub const fn step(&self) -> Step {
        return self.step;
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        return &self.path;
    }
}

impl fmt::Display for IoFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        let source = &self.source;
        return match self.step {
            Step::Create => write!(formatter, "cannot create {path}: {source}"),
            Step::Write => write!(formatter, "cannot write {path}: {source}"),
            Step::Sync => write!(formatter, "cannot sync {path}: {source}"),
            Step::Rename => write!(
                formatter,
                "cannot rename {path} to {}: {source}",
                self.target.display()
            ),
            Step::Remove => write!(formatter, "cannot remove {path}: {source}"),
        };
    }
}

impl Error for IoFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return Some(&self.source);
    }
}

#[derive(Debug)]
pub enum WriteFailure<E> {
    NoFileName { target: PathBuf },
    Content(E),
    Io(IoFailure),
}

impl<E: fmt::Display> fmt::Display for WriteFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::NoFileName { target } => {
                write!(
                    formatter,
                    "output path {} has no file name",
                    target.display()
                )
            }
            Self::Content(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
        };
    }
}

#[derive(Debug)]
pub struct AtomicWriteError<E> {
    pub failure: WriteFailure<E>,
    pub cleanup: Option<IoFailure>,
}

impl<E: fmt::Display> fmt::Display for AtomicWriteError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.failure)?;
        return self.cleanup.as_ref().map_or(Ok(()), |cleanup| {
            return write!(formatter, "; also {cleanup}");
        });
    }
}

impl<E: fmt::Debug + fmt::Display> Error for AtomicWriteError<E> {}

fn temporary_path(target: &Path) -> Option<PathBuf> {
    let name = target.file_name()?;
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{}.tmp", process::id()));
    return Some(target.with_file_name(temporary));
}

fn io_failure(step: Step, path: &Path, target: &Path, source: io::Error) -> IoFailure {
    return IoFailure {
        step,
        path: path.to_path_buf(),
        target: target.to_path_buf(),
        source,
    };
}

pub fn write_atomically<E>(
    target: &Path,
    write: impl FnOnce(BufWriter<File>) -> Result<BufWriter<File>, E>,
) -> Result<(), Box<AtomicWriteError<E>>> {
    let Some(temporary) = temporary_path(target) else {
        return Err(Box::new(AtomicWriteError {
            failure: WriteFailure::NoFileName {
                target: target.to_path_buf(),
            },
            cleanup: None,
        }));
    };
    let failed = |step: Step, source: io::Error| {
        return WriteFailure::Io(io_failure(step, &temporary, target, source));
    };
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| {
            return Box::new(AtomicWriteError {
                failure: failed(Step::Create, source),
                cleanup: None,
            });
        })?;
    let written = write(BufWriter::new(file))
        .map_err(WriteFailure::Content)
        .and_then(|buffered| {
            let file = buffered
                .into_inner()
                .map_err(|error| return failed(Step::Write, error.into_error()))?;
            return file
                .sync_all()
                .map_err(|source| return failed(Step::Sync, source));
        })
        .and_then(|()| {
            return fs::rename(&temporary, target)
                .map_err(|source| return failed(Step::Rename, source));
        });
    let Err(failure) = written else {
        return Ok(());
    };
    let cleanup = fs::remove_file(&temporary)
        .err()
        .map(|source| return io_failure(Step::Remove, &temporary, target, source));
    return Err(Box::new(AtomicWriteError { failure, cleanup }));
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opcov-output-{}-{name}", process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).unwrap();
        return dir;
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| return entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        return names;
    }

    #[test]
    fn a_successful_write_replaces_the_target_and_leaves_no_temporary_file() {
        let dir = scratch("ok");
        let target = dir.join("out.json");
        fs::write(&target, b"old").unwrap();
        write_atomically::<io::Error>(&target, |mut out| {
            out.write_all(b"new")?;
            return Ok(out);
        })
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new", "target replaced");
        assert_eq!(entries(&dir), ["out.json"], "no temporary file left");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failing_write_keeps_the_target_and_removes_the_temporary_file() {
        let dir = scratch("fail");
        let target = dir.join("out.json");
        fs::write(&target, b"old").unwrap();
        let error = write_atomically(&target, |mut out| {
            out.write_all(b"partial").unwrap();
            return Err::<BufWriter<File>, _>("content failed");
        })
        .unwrap_err();
        assert!(
            matches!(error.failure, WriteFailure::Content("content failed")),
            "content error is kept: {error}"
        );
        assert!(error.cleanup.is_none(), "cleanup succeeded");
        assert_eq!(fs::read(&target).unwrap(), b"old", "target untouched");
        assert_eq!(entries(&dir), ["out.json"], "temporary file removed");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_directory_is_reported_with_the_temporary_path() {
        let dir = scratch("missing");
        let target = dir.join("absent").join("out.json");
        let error = write_atomically::<io::Error>(&target, Ok).unwrap_err();
        let WriteFailure::Io(failure) = &error.failure else {
            panic!("expected an I/O failure, got {error}");
        };
        assert_eq!(failure.step(), Step::Create, "fails at create");
        assert!(
            error.to_string().starts_with(&format!(
                "cannot create {}",
                dir.join("absent").join(".out.json.").display()
            )),
            "message names the temporary file: {error}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
