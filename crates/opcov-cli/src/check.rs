use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use opcov_core::cfg::Line;
use opcov_php::{Answer, Project, Query, Request};

use crate::changes::{Change, ChangeSet, FileChange, LineRange, Patch};
use crate::consumers::{self, Via};
use crate::failure::{Failure, Verdict};
use crate::impact::{self, Filter, Scope};
use crate::output::write_stdout;
use crate::serve;
use crate::snapshot::{self, report_digest};
use crate::stitch;
use crate::{gate, stream};

const FILTER: &str = "--filter";
const BASE_SUFFIX: &str = ".base";

pub(crate) struct Inputs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) report: &'a Path,
    pub(crate) depth: u32,
    pub(crate) ignore: &'a [PathBuf],
    pub(crate) command: &'a [String],
    pub(crate) via: Via,
}

enum Run {
    Everything(String),
    Selected(String),
    Nothing,
}

struct Ran {
    status: ExitStatus,
    tests: String,
}

fn base_path(report: &Path) -> PathBuf {
    let mut name = report.file_name().unwrap_or_default().to_os_string();
    name.push(BASE_SUFFIX);
    return report.with_file_name(name);
}

fn rename(from: &Path, to: &Path) -> Result<(), Failure> {
    return fs::rename(from, to).map_err(|error| {
        return Failure::failed(format!(
            "cannot move {} to {}: {error}",
            from.display(),
            to.display()
        ));
    });
}

fn recover(report: &Path) -> Result<bool, Failure> {
    let base = base_path(report);
    if !base.is_file() {
        return Ok(false);
    }
    rename(&base, report)?;
    return Ok(true);
}

fn execute(inputs: &Inputs<'_>, filter: Option<&str>) -> Result<ExitStatus, Failure> {
    let (program, arguments) = inputs
        .command
        .split_first()
        .ok_or_else(|| return Failure::failed("no test command after --".to_owned()))?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(inputs.root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(io::stderr()));
    if let Some(filter) = filter {
        command.args([FILTER, filter]);
    }
    return command
        .status()
        .map_err(|error| return Failure::failed(format!("cannot run {program}: {error}")));
}

fn wrote_report(report: &Path) -> Result<(), Failure> {
    if report.is_file() {
        return Ok(());
    }
    return Err(Failure::failed(format!(
        "the test command wrote no report at {}; point the opcov PHPUnit extension's output at it",
        report.display()
    )));
}

fn run_everything(inputs: &Inputs<'_>, reason: &str) -> Result<Ran, Failure> {
    let base = base_path(inputs.report);
    let had_baseline = inputs.report.is_file();
    if had_baseline {
        rename(inputs.report, &base)?;
    }
    let outcome = execute(inputs, None).and_then(|status| {
        wrote_report(inputs.report)?;
        return Ok(status);
    });
    match outcome {
        Ok(status) => {
            if had_baseline {
                fs::remove_file(&base)
                    .map_err(|error| return Failure::io("remove", &base, &error))?;
            }
            return Ok(Ran {
                status,
                tests: format!("every test ({reason})"),
            });
        }
        Err(failure) => {
            if had_baseline {
                rename(&base, inputs.report)?;
            }
            return Err(failure);
        }
    }
}

fn run_selected(inputs: &Inputs<'_>, filter: &str, patch: &Patch) -> Result<Ran, Failure> {
    let base = base_path(inputs.report);
    rename(inputs.report, &base)?;
    let outcome = execute(inputs, Some(filter)).and_then(|status| {
        wrote_report(inputs.report)?;
        stitch::run(
            &stitch::Inputs {
                baseline: &base,
                partial: inputs.report,
                output: inputs.report,
                base: inputs.root,
                ignore: inputs.ignore,
            },
            patch,
        )?;
        return Ok(status);
    });
    match outcome {
        Ok(status) => {
            fs::remove_file(&base).map_err(|error| return Failure::io("remove", &base, &error))?;
            return Ok(Ran {
                status,
                tests: format!("the tests matching {filter}"),
            });
        }
        Err(failure) => {
            rename(&base, inputs.report)?;
            return Err(failure);
        }
    }
}

fn plan(inputs: &Inputs<'_>, patch: &Patch) -> Result<Run, Failure> {
    let header = stream::header(inputs.report)?;
    let result = impact::evaluate(
        inputs.report,
        &header,
        patch,
        inputs.root,
        inputs.ignore,
        Scope::Files,
    )?;
    let unbounded: Vec<String> = result.unbounded().collect();
    if let Some(first) = unbounded.first() {
        return Ok(Run::Everything(format!(
            "the baseline cannot bound {first}"
        )));
    }
    return Ok(match impact::filter(&result) {
        Filter::Expression(expression) => Run::Selected(expression),
        Filter::Empty => Run::Nothing,
        Filter::Unnamed(names) => Run::Everything(format!(
            "{} selected test(s) have no class to filter by",
            names.len()
        )),
    });
}

fn consumer_changes(answer: &Answer) -> ChangeSet {
    let mut set = ChangeSet::default();
    for consumer in &answer.tree.consumers {
        let range = Line::new(consumer.first)
            .zip(Line::new(consumer.last))
            .and_then(|(first, last)| return LineRange::new(first, last));
        if let Some(range) = range {
            set.add(
                PathBuf::from(&consumer.path),
                FileChange::Parts(BTreeSet::from([Change::Lines(range)])),
            );
        }
    }
    return set;
}

fn summary(out: &mut dyn Write, answer: &Answer, ran: Option<&Ran>) -> io::Result<()> {
    let tests = ran.map_or_else(
        || return "no test reaches the change".to_owned(),
        |ran| return format!("ran {}", ran.tests),
    );
    return writeln!(
        out,
        "check: {} changed member(s), {} consumer(s) to depth; {tests}",
        answer.tree.changed.len(),
        answer.tree.consumers.len()
    );
}

pub(crate) fn run(inputs: &Inputs<'_>) -> Result<Verdict, Failure> {
    if recover(inputs.report)? {
        write_stdout(|out| {
            return writeln!(
                out,
                "check: restored the baseline an interrupted check left aside"
            );
        })?;
    }
    let project =
        Project::open(inputs.root).map_err(|error| return Failure::failed(error.to_string()))?;
    let files = project.all_files();
    let Some(previous) = snapshot::load(inputs.report)?.filter(|_| return inputs.report.is_file())
    else {
        let ran = run_everything(inputs, "no baseline yet")?;
        snapshot::take(inputs.report, inputs.root, &files)?;
        write_stdout(|out| {
            return writeln!(
                out,
                "check: wrote the baseline {} by running {}",
                inputs.report.display(),
                ran.tests
            );
        })?;
        return Ok(if ran.status.success() {
            Verdict::Clean
        } else {
            Verdict::Findings
        });
    };
    let patch = previous.patch(inputs.root, &files)?;
    if patch.files().is_empty() {
        write_stdout(|out| return writeln!(out, "check: nothing changed since the last check"))?;
        return Ok(Verdict::Clean);
    }
    let query = Query {
        changes: consumers::changes(&ChangeSet::of_patch(&patch)),
        request: Request::default(),
        depth: inputs.depth,
    };
    if previous.report() != report_digest(inputs.report)? {
        let ran = run_everything(inputs, "the report changed since the last check")?;
        snapshot::take(inputs.report, inputs.root, &files)?;
        write_stdout(|out| {
            return writeln!(out, "check: rebuilt the baseline by running {}", ran.tests);
        })?;
        return Ok(if ran.status.success() {
            Verdict::Clean
        } else {
            Verdict::Findings
        });
    }
    if inputs.via == Via::Serve {
        serve::ensure(inputs.root)?;
    }
    let answer = consumers::find(inputs.root, &query, inputs.via)?.answer;
    let ran = match plan(inputs, &patch)? {
        Run::Everything(reason) => Some(run_everything(inputs, &reason)?),
        Run::Selected(filter) => Some(run_selected(inputs, &filter, &patch)?),
        Run::Nothing => None,
    };
    let Some(ran) = ran else {
        let callable = answer
            .tree
            .changed
            .iter()
            .any(|member| return member.target.kind.is_callable());
        write_stdout(|out| {
            for member in &answer.tree.changed {
                writeln!(
                    out,
                    "{}:{}-{}\t{}\treached by no test",
                    member.path, member.first, member.last, member.target
                )?;
            }
            return summary(out, &answer, None);
        })?;
        return Ok(if callable {
            Verdict::Findings
        } else {
            Verdict::Clean
        });
    };
    snapshot::take(inputs.report, inputs.root, &files)?;
    let header = stream::header(inputs.report)?;
    let mut changes = ChangeSet::of_patch(&patch);
    changes.merge(consumer_changes(&answer));
    let result = gate::evaluate(inputs.report, &header, &changes, inputs.root)?;
    write_stdout(|out| {
        gate::render_text(out, &result)?;
        if !ran.status.success() {
            writeln!(out, "check: the test command failed ({})", ran.status)?;
        }
        return summary(out, &answer, Some(&ran));
    })?;
    if !ran.status.success() {
        return Ok(Verdict::Findings);
    }
    return Ok(result.verdict());
}
