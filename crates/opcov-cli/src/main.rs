#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

use clap::{Args, Parser, Subcommand};
use opcov_core::report::merge_streams;

#[derive(Debug, Parser)]
#[command(name = "opcov", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Merge opcov reports of parallel runs into one report")]
    Merge(MergeArgs),
}

#[derive(Debug, Args)]
struct MergeArgs {
    #[arg(required = true, value_name = "INPUT", help = "Reports to merge")]
    inputs: Vec<PathBuf>,
    #[arg(
        short = 'o',
        long = "output",
        value_name = "OUT",
        help = "Merged report to write"
    )]
    output: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum Outcome {
    Refused,
    Failed,
}

impl Outcome {
    fn exit_code(self) -> ExitCode {
        return ExitCode::from(match self {
            Self::Refused => 1,
            Self::Failed => 2,
        });
    }
}

#[derive(Debug)]
struct Failure {
    outcome: Outcome,
    message: String,
}

impl Failure {
    fn io(action: &str, path: &Path, error: &io::Error) -> Self {
        return Self {
            outcome: Outcome::Failed,
            message: format!("cannot {action} {}: {error}", path.display()),
        };
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(&self.message);
    }
}

fn temporary_path(target: &Path) -> Result<PathBuf, Failure> {
    let Some(name) = target.file_name() else {
        return Err(Failure {
            outcome: Outcome::Failed,
            message: format!("output path {} has no file name", target.display()),
        });
    };
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{}.tmp", process::id()));
    return Ok(target.with_file_name(temporary));
}

fn write_atomically(
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

fn merge(args: &MergeArgs) -> Result<(), Failure> {
    let names: Vec<_> = args
        .inputs
        .iter()
        .map(|path| return path.display())
        .collect();
    return write_atomically(&args.output, |out| {
        return merge_streams(
            &args.inputs,
            |path| return File::open(path).map(BufReader::new),
            out,
        )
        .map_err(|error| {
            return Failure {
                outcome: if error.is_refusal() {
                    Outcome::Refused
                } else {
                    Outcome::Failed
                },
                message: error.describe(&names).to_string(),
            };
        });
    });
}

fn emit(handle: &mut impl Write, message: &dyn fmt::Display) -> io::Result<()> {
    return writeln!(handle, "{message}");
}

fn conclude(code: ExitCode, written: io::Result<()>) -> ExitCode {
    return written.map_or_else(
        |_unwritable| return Outcome::Failed.exit_code(),
        |()| return code,
    );
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let rendered = error.render();
            if error.use_stderr() {
                let written = emit(&mut io::stderr().lock(), &rendered);
                return conclude(Outcome::Failed.exit_code(), written);
            }
            return conclude(ExitCode::SUCCESS, emit(&mut io::stdout().lock(), &rendered));
        }
    };
    let result = match &cli.command {
        Command::Merge(args) => merge(args),
    };
    return match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let written = emit(&mut io::stderr().lock(), &failure);
            conclude(failure.outcome.exit_code(), written)
        }
    };
}
