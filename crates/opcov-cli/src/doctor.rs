use std::collections::BTreeMap;
use std::io::{self, Write};

use opcov_core::cfg::{BlockId, FunctionKey, Line};
use opcov_core::kind::EdgeKind;
use opcov_core::report::{FileCoverage, Function, Identified, OutcomeId, Report, SourcePath};
use opcov_syntax::{OutcomeRef, SourceFile};
use serde::Serialize;

use crate::failure::Verdict;
use crate::source::SourceError;

#[derive(Debug, Serialize)]
pub(crate) struct UnmatchedOutcome<'r> {
    function: &'r FunctionKey,
    id: OutcomeId,
    kind: EdgeKind,
    line: Line,
    counts_toward_verdict: bool,
    reason: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct MissingFunction<'r> {
    function: &'r FunctionKey,
    reason: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FileDiagnosis<'r> {
    path: &'r SourcePath,
    missing_functions: Vec<MissingFunction<'r>>,
    unmatched_outcomes: Vec<UnmatchedOutcome<'r>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Diagnosis<'r> {
    outcomes: usize,
    unmatched: usize,
    files: Vec<FileDiagnosis<'r>>,
}

impl Diagnosis<'_> {
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
    let mut ordinals: BTreeMap<(EdgeKind, Line), u32> = BTreeMap::new();
    return function
        .outcomes
        .as_slice()
        .iter()
        .map(|outcome| {
            let kind = outcome.kind().get();
            let ordinal = ordinals.entry((kind, outcome.line())).or_default();
            let source = BlockId::new(*ordinal);
            *ordinal = ordinal.saturating_add(1);
            return (
                outcome.id(),
                OutcomeRef {
                    source,
                    kind,
                    line: outcome.line(),
                },
            );
        })
        .collect();
}

fn diagnose_file<'r>(
    path: &'r SourcePath,
    file: &'r FileCoverage,
    source: &SourceFile,
) -> (FileDiagnosis<'r>, usize) {
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
                    function: key,
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
                function: key,
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

pub(crate) fn diagnose(
    report: &Report,
    mut load: impl FnMut(&SourcePath, &FileCoverage) -> Result<SourceFile, SourceError>,
) -> Result<Diagnosis<'_>, SourceError> {
    let mut diagnosis = Diagnosis {
        outcomes: 0,
        unmatched: 0,
        files: Vec::new(),
    };
    for (path, file) in report.files() {
        let source = load(path, file)?;
        let (found, outcomes) = diagnose_file(path, file, &source);
        diagnosis.outcomes = diagnosis.outcomes.saturating_add(outcomes);
        diagnosis.unmatched = diagnosis
            .unmatched
            .saturating_add(found.unmatched_outcomes.len());
        if !found.missing_functions.is_empty() || !found.unmatched_outcomes.is_empty() {
            diagnosis.files.push(found);
        }
    }
    return Ok(diagnosis);
}

pub(crate) fn render_text(out: &mut dyn Write, diagnosis: &Diagnosis<'_>) -> io::Result<()> {
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
    return writeln!(
        out,
        "doctor: {} of {} outcome(s) have no source construct",
        diagnosis.unmatched, diagnosis.outcomes
    );
}

pub(crate) fn render_json(out: &mut dyn Write, diagnosis: &Diagnosis<'_>) -> io::Result<()> {
    serde_json::to_writer(&mut *out, diagnosis)?;
    return writeln!(out);
}
