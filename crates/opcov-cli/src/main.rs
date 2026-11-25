#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "rustc's unreachable_pub, which the workspace denies, requires pub(crate) in private modules"
)]

use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use opcov_core::report::merge_streams;

mod changes;
mod checkup;
mod consumers;
mod convert;
mod doctor;
mod failure;
mod gate;
mod impact;
mod output;
mod source;
mod spool;
mod stitch;
mod stream;
mod view;
mod xml;

use changes::{ChangeSet, Patch, parse_diff, parse_list};
use convert::{Format, convert};
use failure::{Failure, Severity, Verdict};
use output::{write_atomically, write_stderr, write_stdout};

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
    #[command(about = "Convert an opcov report to Clover, Cobertura, lcov or HTML")]
    Convert(ConvertArgs),
    #[command(about = "Check the changed functions of a change set against a report")]
    Gate(GateArgs),
    #[command(
        about = "Choose the tests a change can affect from a baseline report, and show the baseline coverage of the changed functions"
    )]
    Impact(ImpactArgs),
    #[command(
        about = "Lay a run of the tests a change reaches over a baseline report, giving the report of a run of every test"
    )]
    Stitch(StitchArgs),
    #[command(
        about = "Find the code that uses changed PHP members: methods, functions, constants, properties and the code the framework calls"
    )]
    Consumers(ConsumersArgs),
    #[command(
        about = "Check that PHP can collect coverage with opcov, or with --report diagnose a report"
    )]
    Doctor(DoctorArgs),
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

#[derive(Debug, Args)]
struct ConvertArgs {
    #[arg(value_name = "REPORT", help = "opcov report to convert")]
    report: PathBuf,
    #[arg(long, value_enum, help = "Format to write")]
    format: Format,
    #[arg(
        short = 'o',
        long = "output",
        value_name = "OUT",
        help = "File to write"
    )]
    output: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
struct ChangesArgs {
    #[arg(
        long,
        value_name = "DIFF",
        help = "Unified diff of the change (`git diff -U0`), or `-` for standard input"
    )]
    diff: Option<PathBuf>,
    #[arg(
        long,
        value_name = "ENTRY",
        num_args = 1..,
        help = "Changed files as `path[:start-end,...]`, or `@listfile` with one entry per line (`@-` reads standard input)"
    )]
    files: Vec<String>,
}

#[derive(Debug, Args)]
struct Location {
    #[arg(
        long,
        value_name = "N",
        default_value_t = 1,
        help = "Leading path components to strip from diff paths (1 removes git's `a/` and `b/`)"
    )]
    strip: usize,
    #[arg(
        long,
        value_name = "DIR",
        help = "Directory that changed paths are relative to [default: the current directory]"
    )]
    base: Option<PathBuf>,
}

impl Location {
    fn base(&self) -> Result<PathBuf, Failure> {
        return self.base.as_ref().map_or_else(
            || {
                return env::current_dir().map_err(|error| {
                    return Failure::io("resolve", Path::new("the current directory"), &error);
                });
            },
            |base| return Ok(base.clone()),
        );
    }
}

#[derive(Debug, Args)]
struct GateArgs {
    #[arg(long, value_name = "REPORT", help = "opcov report to check")]
    report: PathBuf,
    #[command(flatten)]
    changes: ChangesArgs,
    #[command(flatten)]
    location: Location,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help = "Output format")]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImpactFormat {
    Text,
    Json,
    Filter,
}

#[derive(Debug, Args)]
struct ImpactArgs {
    #[arg(
        long,
        value_name = "REPORT",
        help = "Baseline report: a run of the commit the diff starts from"
    )]
    report: PathBuf,
    #[arg(
        long,
        value_name = "DIFF",
        help = "Unified diff from the baseline's commit to the working tree (`git diff -U0 <commit>`), or `-` for standard input"
    )]
    diff: PathBuf,
    #[command(flatten)]
    location: Location,
    #[arg(
        long,
        value_name = "PATH",
        help = "File or directory, relative to the base, whose changes cannot affect a test (repeatable)"
    )]
    ignore: Vec<PathBuf>,
    #[arg(
        long,
        help = "Also select every test that ran a changed file, as `opcov stitch` needs"
    )]
    whole_files: bool,
    #[arg(long, value_enum, default_value_t = ImpactFormat::Text, help = "Output format: text, json, or a PHPUnit/Pest --filter expression")]
    format: ImpactFormat,
}

#[derive(Debug, Args)]
struct StitchArgs {
    #[arg(
        long,
        value_name = "REPORT",
        help = "Baseline report: a run of every test on the commit the diff starts from"
    )]
    baseline: PathBuf,
    #[arg(
        long,
        value_name = "REPORT",
        help = "Report of the working tree's run of the tests `opcov impact --whole-files` selects"
    )]
    partial: PathBuf,
    #[arg(
        long,
        value_name = "DIFF",
        help = "Unified diff from the baseline's commit to the working tree (`git diff -U0 <commit>`), or `-` for standard input"
    )]
    diff: PathBuf,
    #[command(flatten)]
    location: Location,
    #[arg(
        long,
        value_name = "PATH",
        help = "File or directory, relative to the base, whose changes cannot affect a test (repeatable)"
    )]
    ignore: Vec<PathBuf>,
    #[arg(
        short = 'o',
        long = "output",
        value_name = "OUT",
        help = "Stitched report to write"
    )]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct ConsumersArgs {
    #[arg(
        long,
        value_name = "DIR",
        help = "Project root, the directory holding composer.json and vendor/ [default: the current directory]"
    )]
    root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "TARGET",
        help = "A member to find the uses of: `method Class::name`, `constant Class::NAME`, `property Class::name`, `function name` or `global-constant NAME` (repeatable)"
    )]
    target: Vec<String>,
    #[arg(
        long,
        value_name = "FILE",
        help = "JSON request with targets, queueEntryMethods, modelEventTargets, commandTargets and within, or `-` for standard input"
    )]
    request: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Search only this directory or file, relative to the root (repeatable) [default: every autoloaded project source]"
    )]
    path: Vec<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help = "Output format")]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(
        long,
        value_name = "REPORT",
        help = "opcov report to diagnose: list outcomes that match no source construct"
    )]
    report: Option<PathBuf>,
    #[arg(
        long,
        value_name = "BINARY",
        conflicts_with = "report",
        help = "PHP binary whose INI files and extensions to check [default: php]"
    )]
    php: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, help = "Output format")]
    format: OutputFormat,
}

fn merge(args: &MergeArgs) -> Result<Verdict, Failure> {
    let names: Vec<_> = args
        .inputs
        .iter()
        .map(|path| return path.display())
        .collect();
    write_atomically(&args.output, |out| {
        return merge_streams(
            &args.inputs,
            |path| return File::open(path).map(BufReader::new),
            out,
        )
        .map_err(|error| return Failure::merge(&error, &names));
    })?;
    return Ok(Verdict::Clean);
}

fn convert_report(args: &ConvertArgs) -> Result<Verdict, Failure> {
    let header = stream::header(&args.report)?;
    write_atomically(&args.output, |out| {
        return convert(&args.report, &header, args.format, out);
    })?;
    return Ok(Verdict::Clean);
}

fn read_input(path: &Path) -> Result<Vec<u8>, Failure> {
    if path == Path::new("-") {
        let mut buffer = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut buffer)
            .map_err(|error| return Failure::io("read", Path::new("standard input"), &error))?;
        return Ok(buffer);
    }
    return fs::read(path).map_err(|error| return Failure::io("read", path, &error));
}

fn describe(path: &Path) -> String {
    if path == Path::new("-") {
        return "standard input".to_owned();
    }
    return path.display().to_string();
}

fn invalid(error: &changes::ChangeError) -> Failure {
    return Failure::failed(error.to_string());
}

fn read_patch(diff: &Path, strip: usize) -> Result<Patch, Failure> {
    let input = read_input(diff)?;
    return parse_diff(&describe(diff), &input, strip).map_err(|error| return invalid(&error));
}

fn read_changes(args: &GateArgs) -> Result<ChangeSet, Failure> {
    if let Some(diff) = &args.changes.diff {
        return read_patch(diff, args.location.strip)
            .map(|patch| return ChangeSet::of_patch(&patch));
    }
    let mut set = ChangeSet::default();
    for entry in &args.changes.files {
        let parsed = match entry.strip_prefix('@') {
            Some(list) => {
                let path = Path::new(list);
                let bytes = read_input(path)?;
                let text = String::from_utf8(bytes).map_err(|error| {
                    return Failure::failed(format!(
                        "file list {} is not UTF-8: {error}",
                        describe(path)
                    ));
                })?;
                parse_list(&describe(path), &text)
            }
            None => parse_list("argument", entry),
        };
        set.merge(parsed.map_err(|error| return invalid(&error))?);
    }
    return Ok(set);
}

fn emit_result<T>(
    format: OutputFormat,
    result: &T,
    text: fn(&mut dyn Write, &T) -> io::Result<()>,
    json: fn(&mut dyn Write, &T) -> io::Result<()>,
) -> Result<(), Failure> {
    return write_stdout(|out| {
        return match format {
            OutputFormat::Text => text(out, result),
            OutputFormat::Json => json(out, result),
        };
    });
}

fn gate_report(args: &GateArgs) -> Result<Verdict, Failure> {
    let header = stream::header(&args.report)?;
    let changes = read_changes(args)?;
    let base = args.location.base()?;
    let result = gate::evaluate(&args.report, &header, &changes, &base)?;
    emit_result(args.format, &result, gate::render_text, gate::render_json)?;
    return Ok(result.verdict());
}

fn impact_report(args: &ImpactArgs) -> Result<Verdict, Failure> {
    let header = stream::header(&args.report)?;
    let patch = read_patch(&args.diff, args.location.strip)?;
    let base = args.location.base()?;
    let scope = if args.whole_files {
        impact::Scope::Files
    } else {
        impact::Scope::Functions
    };
    let result = impact::evaluate(&args.report, &header, &patch, &base, &args.ignore, scope)?;
    match args.format {
        ImpactFormat::Text => emit_result(
            OutputFormat::Text,
            &result,
            impact::render_text,
            impact::render_json,
        )?,
        ImpactFormat::Json => emit_result(
            OutputFormat::Json,
            &result,
            impact::render_text,
            impact::render_json,
        )?,
        ImpactFormat::Filter => return emit_filter(&result),
    }
    return Ok(result.verdict());
}

fn stitch_report(args: &StitchArgs) -> Result<Verdict, Failure> {
    let patch = read_patch(&args.diff, args.location.strip)?;
    let base = args.location.base()?;
    return stitch::run(
        &stitch::Inputs {
            baseline: &args.baseline,
            partial: &args.partial,
            output: &args.output,
            base: &base,
            ignore: &args.ignore,
        },
        &patch,
    );
}

fn consumers_report(args: &ConsumersArgs) -> Result<Verdict, Failure> {
    let root = match &args.root {
        Some(root) => root.clone(),
        None => env::current_dir().map_err(|error| {
            return Failure::io("resolve", Path::new("the current directory"), &error);
        })?,
    };
    let json = args.request.as_deref().map(read_input).transpose()?;
    let source = args.request.as_deref().map_or_else(String::new, describe);
    let request = consumers::request(json.as_deref(), &args.target, &args.path, &source)?;
    let found = consumers::find(&root, &request)?;
    emit_result(
        args.format,
        &found,
        consumers::render_text,
        consumers::render_json,
    )?;
    return Ok(Verdict::Clean);
}

fn emit_filter(result: &impact::ImpactResult<'_>) -> Result<Verdict, Failure> {
    if result.verdict() == Verdict::Findings {
        write_stderr(|out| return impact::render_text(out, result))?;
        return Ok(Verdict::Findings);
    }
    return match impact::filter(result) {
        impact::Filter::Expression(expression) => {
            write_stdout(|out| return writeln!(out, "{expression}"))?;
            Ok(Verdict::Clean)
        }
        impact::Filter::Empty => Ok(Verdict::Clean),
        impact::Filter::Unnamed(names) => {
            write_stderr(|out| {
                return writeln!(
                    out,
                    "impact: run every test, a --filter cannot select {} selected test(s) that are not Class::method, such as {}",
                    names.len(),
                    names.first().map_or("", String::as_str)
                );
            })?;
            Ok(Verdict::Findings)
        }
    };
}

fn doctor(args: &DoctorArgs) -> Result<Verdict, Failure> {
    let Some(report) = &args.report else {
        let php = args
            .php
            .clone()
            .unwrap_or_else(|| return PathBuf::from("php"));
        let found = checkup::run(&php)?;
        emit_result(
            args.format,
            &found,
            checkup::render_text,
            checkup::render_json,
        )?;
        return Ok(found.verdict());
    };
    return doctor_report(report, args.format);
}

fn doctor_report(path: &Path, format: OutputFormat) -> Result<Verdict, Failure> {
    let header = stream::header(path)?;
    let diagnosis = doctor::diagnose(path, &header)?;
    emit_result(format, &diagnosis, doctor::render_text, doctor::render_json)?;
    return Ok(diagnosis.verdict());
}

fn emit(handle: &mut impl Write, message: &dyn fmt::Display) -> io::Result<()> {
    return writeln!(handle, "{message}");
}

fn conclude(code: ExitCode, written: io::Result<()>) -> ExitCode {
    return written.map_or_else(
        |_unwritable| return Severity::Failed.exit_code(),
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
                return conclude(Severity::Failed.exit_code(), written);
            }
            return conclude(ExitCode::SUCCESS, emit(&mut io::stdout().lock(), &rendered));
        }
    };
    let result = match &cli.command {
        Command::Merge(args) => merge(args),
        Command::Convert(args) => convert_report(args),
        Command::Gate(args) => gate_report(args),
        Command::Impact(args) => impact_report(args),
        Command::Stitch(args) => stitch_report(args),
        Command::Consumers(args) => consumers_report(args),
        Command::Doctor(args) => doctor(args),
    };
    return match result {
        Ok(verdict) => verdict.exit_code(),
        Err(failure) => {
            let written = emit(&mut io::stderr().lock(), &failure);
            conclude(failure.severity.exit_code(), written)
        }
    };
}
