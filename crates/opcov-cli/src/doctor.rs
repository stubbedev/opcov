use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use opcov_core::cfg::{FunctionKey, Line};
use opcov_core::kind::EdgeKind;
use opcov_core::report::{FileCoverage, Function, Header, Identified, OutcomeId, SourcePath};
use opcov_syntax::{OutcomeRef, SourceFile};
use serde::Serialize;

use crate::failure::{Failure, Verdict};
use crate::source;
use crate::stream;

#[derive(Debug, Serialize)]
pub(crate) struct UnmatchedOutcome {
    function: FunctionKey,
    id: OutcomeId,
    kind: EdgeKind,
    line: Line,
    counts_toward_verdict: bool,
    reason: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct MissingFunction {
    function: FunctionKey,
    reason: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FileDiagnosis {
    path: SourcePath,
    missing_functions: Vec<MissingFunction>,
    unmatched_outcomes: Vec<UnmatchedOutcome>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Diagnosis {
    outcomes: usize,
    unmatched: usize,
    unchecked_eval_entries: usize,
    files: Vec<FileDiagnosis>,
}

impl Diagnosis {
    pub(crate) fn verdict(&self) -> Verdict {
        let blocking = self.files.iter().any(|file| {
            return !file.missing_functions.is_empty()
                || file
                    .unmatched_outcomes
                    .iter()
                    .any(|outcome| return outcome.counts_toward_verdict);
        });
        return if blocking {
            Verdict::Findings
        } else {
            Verdict::Clean
        };
    }
}

fn outcome_refs(function: &Function) -> Vec<(OutcomeId, OutcomeRef)> {
    return function
        .outcomes
        .as_slice()
        .iter()
        .map(|outcome| {
            return (
                outcome.id(),
                OutcomeRef {
                    site: outcome.site(),
                    kind: outcome.kind().get(),
                    line: outcome.line(),
                },
            );
        })
        .collect();
}

fn diagnose_file(
    path: SourcePath,
    file: &FileCoverage,
    source: &SourceFile,
) -> (FileDiagnosis, usize) {
    let mut diagnosis = FileDiagnosis {
        path,
        missing_functions: Vec::new(),
        unmatched_outcomes: Vec::new(),
    };
    let mut outcomes = 0_usize;
    for (key, function) in &file.functions {
        outcomes = outcomes.saturating_add(function.outcomes.as_slice().len());
        let located = match source.function(key) {
            Ok(located) => located,
            Err(error) => {
                diagnosis.missing_functions.push(MissingFunction {
                    function: key.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let refs = outcome_refs(function);
        let probes: Vec<OutcomeRef> = refs.iter().map(|(_, outcome)| return *outcome).collect();
        let matching = located.match_outcomes(&probes);
        for ((id, outcome), matched) in refs.iter().zip(matching.outcomes()) {
            let Err(reason) = matched.location else {
                continue;
            };
            diagnosis.unmatched_outcomes.push(UnmatchedOutcome {
                function: key.clone(),
                id: *id,
                kind: outcome.kind,
                line: outcome.line,
                counts_toward_verdict: outcome.kind.counts_toward_verdict(),
                reason: reason.to_string(),
            });
        }
    }
    return (diagnosis, outcomes);
}

pub(crate) fn diagnose(report: &Path, header: &Header) -> Result<Diagnosis, Failure> {
    let mut diagnosis = Diagnosis {
        outcomes: 0,
        unmatched: 0,
        unchecked_eval_entries: 0,
        files: Vec::new(),
    };
    let unchecked = AtomicUsize::new(0);
    let checked = |path: &SourcePath| {
        let source = stream::source_files(path);
        if !source {
            unchecked.fetch_add(1, Ordering::Relaxed);
        }
        return source;
    };
    stream::entries(
        report,
        header,
        &checked,
        &|_, path, file| {
            let source = source::load(&header.paths, &path, &file)?;
            return Ok(diagnose_file(path, &file, &source));
        },
        &mut |(found, outcomes)| {
            diagnosis.outcomes = diagnosis.outcomes.saturating_add(outcomes);
            diagnosis.unmatched = diagnosis
                .unmatched
                .saturating_add(found.unmatched_outcomes.len());
            if !found.missing_functions.is_empty() || !found.unmatched_outcomes.is_empty() {
                diagnosis.files.push(found);
            }
            return Ok(());
        },
    )?;
    diagnosis.unchecked_eval_entries = unchecked.into_inner();
    return Ok(diagnosis);
}

pub(crate) fn render_text(out: &mut dyn Write, diagnosis: &Diagnosis) -> io::Result<()> {
    for file in &diagnosis.files {
        writeln!(out, "{}", file.path)?;
        for missing in &file.missing_functions {
            writeln!(out, "  {}: {}", missing.function, missing.reason)?;
        }
        for outcome in &file.unmatched_outcomes {
            let note = if outcome.counts_toward_verdict {
                ""
            } else {
                " (excluded from verdicts)"
            };
            writeln!(
                out,
                "  {} {} {:?} on line {}{note}: {}",
                outcome.function, outcome.id, outcome.kind, outcome.line, outcome.reason
            )?;
        }
    }
    if diagnosis.unchecked_eval_entries > 0 {
        writeln!(
            out,
            "doctor: {} eval()'d code entr{} not checked: eval()'d code has no source file to match",
            diagnosis.unchecked_eval_entries,
            if diagnosis.unchecked_eval_entries == 1 {
                "y"
            } else {
                "ies"
            }
        )?;
    }
    return writeln!(
        out,
        "doctor: {} of {} outcome(s) have no source construct",
        diagnosis.unmatched, diagnosis.outcomes
    );
}

pub(crate) fn render_json(out: &mut dyn Write, diagnosis: &Diagnosis) -> io::Result<()> {
    serde_json::to_writer(&mut *out, diagnosis)?;
    return writeln!(out);
}
