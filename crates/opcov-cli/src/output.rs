use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process;

use opcov_core::report::Report;

use crate::failure::Failure;

fn temporary_path(target: &Path) -> Result<PathBuf, Failure> {
    let Some(name) = target.file_name() else {
        return Err(Failure::failed(format!(
            "output path {} has no file name",
            target.display()
        )));
    };
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{}.tmp", process::id()));
    return Ok(target.with_file_name(temporary));
}

pub(crate) fn write_atomically(
    target: &Path,
    write: impl FnOnce(BufWriter<File>) -> Result<BufWriter<File>, Failure>,
) -> Result<(), Failure> {
    let temporary = temporary_path(target)?;
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| return Failure::io("create", &temporary, &error))?;
    let written = write(BufWriter::new(file))
        .and_then(|buffered| {
            let file = buffered
                .into_inner()
                .map_err(|error| return Failure::io("write", &temporary, error.error()))?;
            return file
                .sync_all()
                .map_err(|error| return Failure::io("sync", &temporary, &error));
        })
        .and_then(|()| {
            return fs::rename(&temporary, target).map_err(|error| {
                return Failure::io(
                    &format!("rename {} to", temporary.display()),
                    target,
                    &error,
                );
            });
        });
    let Err(failure) = written else {
        return Ok(());
    };
    return Err(match fs::remove_file(&temporary) {
        Ok(()) => failure,
        Err(cleanup) => Failure {
            message: format!(
                "{failure}; also cannot remove {}: {cleanup}",
                temporary.display()
            ),
            ..failure
        },
    });
}

pub(crate) fn write_stdout(
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<(), Failure> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    return write(&mut out)
        .and_then(|()| return out.flush())
        .map_err(|error| {
            return Failure::failed(format!("cannot write to standard output: {error}"));
        });
}

pub(crate) fn read_report(path: &Path) -> Result<Report, Failure> {
    let file = File::open(path).map_err(|error| return Failure::io("open", path, &error))?;
    return Report::read(BufReader::new(file)).map_err(|error| {
        return Failure::failed(format!("cannot read report {}: {error}", path.display()));
    });
}
