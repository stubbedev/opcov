use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use opcov_core::output::{self, AtomicWriteError, WriteFailure};

use crate::failure::Failure;

pub(crate) fn write_atomically(
    target: &Path,
    write: impl FnOnce(BufWriter<File>) -> Result<BufWriter<File>, Failure>,
) -> Result<(), Failure> {
    return output::write_atomically(target, write).map_err(|error| {
        let AtomicWriteError { failure, cleanup } = *error;
        let failure = match failure {
            WriteFailure::Content(failure) => failure,
            other @ (WriteFailure::NoFileName { .. } | WriteFailure::Io(_)) => {
                Failure::failed(other.to_string())
            }
        };
        return match cleanup {
            None => failure,
            Some(cleanup) => Failure {
                message: format!("{failure}; also {cleanup}"),
                ..failure
            },
        };
    });
}

fn write_to(
    name: &str,
    handle: impl Write,
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<(), Failure> {
    let mut out = BufWriter::new(handle);
    return write(&mut out)
        .and_then(|()| return out.flush())
        .map_err(|error| return Failure::failed(format!("cannot write to {name}: {error}")));
}

pub(crate) fn write_stdout(
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<(), Failure> {
    return write_to("standard output", io::stdout().lock(), write);
}

pub(crate) fn write_stderr(
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<(), Failure> {
    return write_to("standard error", io::stderr().lock(), write);
}
