use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use opcov_core::cfg::{Line, LoopShape};
use opcov_core::filter::{
    CaseSensitivity, CompiledFile, Decision, Selection, SkipReason, UnresolvedSelection,
};
use opcov_core::kind::EdgeKind;
use opcov_core::report::{
    FileCoverage, Function, Header, HitSet, Identified, LoopId, OutcomeId, Settings, SourcePath,
    Span as ReportSpan, TestTable,
};
use opcov_syntax::{FunctionSource, Span};
use serde::{Serialize, Serializer};

use crate::changes::{Change, ChangeSet, FileChange, resolve};
use crate::failure::{Failure, Verdict};
use crate::source;
use crate::stream;
use crate::view::{is_nested, location};

const PHP_EXTENSION: &str = "php";

#[derive(Debug, Serialize)]
pub(crate) struct OutcomeFinding {
    id: OutcomeId,
    kind: EdgeKind,
    line: Line,
    span: Option<ReportSpan>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LoopFinding {
    id: LoopId,
    kind: LoopShape,
    line: Line,
}

#[derive(Debug, Serialize)]
pub(crate) struct LineTests<'r> {
    line: Line,
    tests: Vec<&'r str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FunctionResult<'r> {
    key: String,
    span: Span,
    in_report: bool,
    uncovered_lines: Vec<Line>,
    unseen_outcomes: Vec<OutcomeFinding>,
    loops_never_zero: Vec<LoopFinding>,
    loops_never_twice: Vec<LoopFinding>,
    lines: Vec<LineTests<'r>>,
}

impl FunctionResult<'_> {
    const fn findings(&self) -> usize {
        return self
            .uncovered_lines
            .len()
            .saturating_add(self.unseen_outcomes.len())
            .saturating_add(self.loops_never_zero.len())
            .saturating_add(self.loops_never_twice.len());
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FileResult<'r> {
    path: SourcePath,
    functions: Vec<FunctionResult<'r>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ChangedLines {
    WholeFile,
    Parts {
        lines: Vec<Line>,
        removed_before: Vec<Line>,
    },
}

impl ChangedLines {
    fn of(changes: &[&FileChange]) -> Self {
        let mut lines = BTreeSet::new();
        let mut removed_before = BTreeSet::new();
        for change in changes {
            let FileChange::Parts(parts) = change else {
                return Self::WholeFile;
            };
            for part in parts {
                match *part {
                    Change::Lines(range) => lines.extend(range.lines()),
                    Change::Deletion { before } => {
                        removed_before.insert(before);
                    }
                }
            }
        }
        return Self::Parts {
            lines: lines.into_iter().collect(),
            removed_before: removed_before.into_iter().collect(),
        };
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct UnloadedFile {
    path: SourcePath,
    changed: ChangedLines,
}

#[derive(Debug)]
enum Unmeasurable {
    OutsideRoot,
    NotPhp,
    Missing,
    NotSelected(SkipReason),
    PatternNotEvaluated,
}

impl fmt::Display for Unmeasurable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::OutsideRoot => formatter.write_str("outside the report's root"),
            Self::NotPhp => write!(formatter, "not a .{PHP_EXTENSION} file"),
            Self::Missing => formatter.write_str("does not exist"),
            Self::NotSelected(reason) => write!(formatter, "{reason}"),
            Self::PatternNotEvaluated => formatter.write_str(
                "the report's opcov.exclude_pattern may exclude it, and the gate cannot evaluate PCRE patterns",
            ),
        };
    }
}

impl Serialize for Unmeasurable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.collect_str(self);
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Unmeasured {
    path: String,
    reason: Unmeasurable,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum VerdictName {
    Pass,
    Findings,
}

#[derive(Debug, Serialize)]
pub(crate) struct GateResult<'r> {
    verdict: VerdictName,
    findings: usize,
    files: Vec<FileResult<'r>>,
    unloaded: Vec<UnloadedFile>,
    unmeasured: Vec<Unmeasured>,
}

impl GateResult<'_> {
    pub(crate) const fn verdict(&self) -> Verdict {
        return match self.verdict {
            VerdictName::Pass => Verdict::Clean,
            VerdictName::Findings => Verdict::Findings,
        };
    }
}

const fn line_range(span: Span) -> (Line, Line) {
    return (span.start().line(), span.end().line());
}

fn innermost<'s>(functions: impl Iterator<Item = (usize, &'s FunctionSource)>) -> Option<usize> {
    return functions
        .max_by_key(|&(index, function)| {
            let span = function.span();
            return (span.start(), Reverse(span.end()), Reverse(index));
        })
        .map(|(index, _)| return index);
}

fn owner_of_line(functions: &[FunctionSource], line: Line) -> Option<usize> {
    return innermost(functions.iter().enumerate().filter(|(_, function)| {
        let (first, last) = line_range(function.span());
        return first <= line && line <= last;
    }));
}

fn owner_of_gap(functions: &[FunctionSource], before: Line) -> Option<usize> {
    return innermost(functions.iter().enumerate().filter(|(_, function)| {
        let (first, last) = line_range(function.span());
        return first < before && before <= last;
    }));
}

fn encloses(outer: Span, inner: Span) -> bool {
    return outer.start() <= inner.start() && inner.end() <= outer.end();
}

fn changed_functions(functions: &[FunctionSource], change: &FileChange) -> BTreeSet<usize> {
    let FileChange::Parts(parts) = change else {
        return (0..functions.len()).collect();
    };
    let mut direct = BTreeSet::new();
    for part in parts {
        match *part {
            Change::Lines(range) => {
                direct.extend(
                    range
                        .lines()
                        .filter_map(|line| return owner_of_line(functions, line)),
                );
            }
            Change::Deletion { before } => direct.extend(owner_of_gap(functions, before)),
        }
    }
    let mut selected = direct;
    for (index, function) in functions.iter().enumerate() {
        let mut current = (index, function);
        while is_nested(current.1.key()) {
            let Some(parent) = parent_of(functions, current.0) else {
                break;
            };
            if selected.contains(&parent) {
                selected.insert(index);
                break;
            }
            let Some(next) = functions.get(parent) else {
                break;
            };
            current = (parent, next);
        }
    }
    return selected;
}

fn parent_of(functions: &[FunctionSource], child: usize) -> Option<usize> {
    let span = functions.get(child)?.span();
    return innermost(functions.iter().enumerate().filter(|&(index, outer)| {
        return index != child && encloses(outer.span(), span);
    }));
}

fn test_names<'r>(tests: &'r TestTable, hits: &HitSet) -> Vec<&'r str> {
    return hits
        .as_slice()
        .iter()
        .filter_map(|index| return tests.name(*index))
        .collect();
}

fn evaluate_function<'r>(
    tests: &'r TestTable,
    file: &FileCoverage,
    functions: &[FunctionSource],
    index: usize,
    measured: Option<&Function>,
) -> Option<FunctionResult<'r>> {
    let source = functions.get(index)?;
    let (first, last) = line_range(source.span());
    let within = |line: Line| return first <= line && line <= last;
    let mut uncovered_lines = Vec::new();
    let mut lines = Vec::new();
    for (line, hits) in file.lines.range(first..=last) {
        if owner_of_line(functions, *line) != Some(index) {
            continue;
        }
        if hits.is_empty() {
            uncovered_lines.push(*line);
        }
        lines.push(LineTests {
            line: *line,
            tests: test_names(tests, hits),
        });
    }
    let mut unseen_outcomes = Vec::new();
    let mut loops_never_zero = Vec::new();
    let mut loops_never_twice = Vec::new();
    if let Some(function) = measured {
        unseen_outcomes.extend(
            function
                .outcomes
                .as_slice()
                .iter()
                .filter(|outcome| {
                    return outcome.hits().is_empty()
                        && outcome.kind().get().counts_toward_verdict()
                        && within(outcome.line());
                })
                .map(|outcome| {
                    return OutcomeFinding {
                        id: outcome.id(),
                        kind: outcome.kind().get(),
                        line: outcome.line(),
                        span: outcome.span(),
                    };
                }),
        );
        for looped in function
            .loops
            .as_slice()
            .iter()
            .filter(|looped| return within(looped.line()))
        {
            let finding = || {
                return LoopFinding {
                    id: looped.id(),
                    kind: looped.kind().get(),
                    line: looped.line(),
                };
            };
            if looped.zero().is_some_and(HitSet::is_empty) {
                loops_never_zero.push(finding());
            }
            if looped.second().is_empty() {
                loops_never_twice.push(finding());
            }
        }
    }
    return Some(FunctionResult {
        key: source.key().to_string(),
        span: source.span(),
        in_report: measured.is_some(),
        uncovered_lines,
        unseen_outcomes,
        loops_never_zero,
        loops_never_twice,
        lines,
    });
}

fn evaluate_file<'r>(
    header: &'r Header,
    path: SourcePath,
    file: &FileCoverage,
    changes: &[&FileChange],
) -> Result<FileResult<'r>, Failure> {
    let source = source::load(&header.paths, &path, file)?;
    let functions = source.functions();
    let mut selected = BTreeSet::new();
    for change in changes {
        selected.extend(changed_functions(functions, change));
    }
    let results = selected
        .into_iter()
        .filter_map(|index| {
            let found = functions
                .get(index)
                .and_then(|function| return file.functions.get(function.key()));
            return evaluate_function(&header.tests, file, functions, index, found);
        })
        .collect();
    return Ok(FileResult {
        path,
        functions: results,
    });
}

fn selection(settings: &Settings, base: &Path) -> Result<Selection, Failure> {
    return UnresolvedSelection::from_entries(
        settings.collect_paths.iter().cloned(),
        settings.exclude_paths.iter().cloned(),
    )
    .resolve(
        |entry| return fs::canonicalize(base.join(entry)),
        CaseSensitivity::HOST,
    )
    .map_err(|error| {
        return Failure::failed(format!(
            "cannot rebuild the report's file selection relative to {}: {error}",
            base.display()
        ));
    });
}

fn canonical(disk: &Path) -> Result<Option<PathBuf>, Failure> {
    return match fs::canonicalize(disk) {
        Ok(resolved) => Ok(Some(resolved)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Failure::io("resolve", disk, &error)),
    };
}

fn split_unloaded<'c>(
    header: &Header,
    base: &Path,
    missing: impl Iterator<Item = (SourcePath, Vec<&'c FileChange>)>,
) -> Result<(Vec<UnloadedFile>, Vec<Unmeasured>), Failure> {
    let mut unmeasured = Vec::new();
    let mut candidates = Vec::new();
    for (path, changes) in missing {
        let is_php = Path::new(path.as_str())
            .extension()
            .is_some_and(|extension| return extension == PHP_EXTENSION);
        if !is_php {
            unmeasured.push(Unmeasured {
                path: path.to_string(),
                reason: Unmeasurable::NotPhp,
            });
            continue;
        }
        match canonical(Path::new(&location(&header.paths, &path)))? {
            Some(disk) => candidates.push((path, changes, disk)),
            None => unmeasured.push(Unmeasured {
                path: path.to_string(),
                reason: Unmeasurable::Missing,
            }),
        }
    }
    if candidates.is_empty() {
        return Ok((Vec::new(), unmeasured));
    }
    let selection = selection(&header.settings, base)?;
    let mut unloaded = Vec::new();
    for (path, changes, disk) in candidates {
        let reason = match selection.decide(CompiledFile::Resolved(&disk), false) {
            Decision::Skipped(reason) => Unmeasurable::NotSelected(reason),
            Decision::Selected if header.settings.exclude_pattern.is_some() => {
                Unmeasurable::PatternNotEvaluated
            }
            Decision::Selected => {
                unloaded.push(UnloadedFile {
                    path,
                    changed: ChangedLines::of(&changes),
                });
                continue;
            }
        };
        unmeasured.push(Unmeasured {
            path: path.to_string(),
            reason,
        });
    }
    return Ok((unloaded, unmeasured));
}

pub(crate) fn evaluate<'r>(
    report: &Path,
    header: &'r Header,
    changes: &ChangeSet,
    base: &Path,
) -> Result<GateResult<'r>, Failure> {
    let mut targets: BTreeMap<SourcePath, Vec<&FileChange>> = BTreeMap::new();
    let mut outside = Vec::new();
    for (changed, change) in changes.files() {
        match resolve(&header.paths, base, changed) {
            Some(path) => targets.entry(path).or_default().push(change),
            None => outside.push(Unmeasured {
                path: changed.display().to_string(),
                reason: Unmeasurable::OutsideRoot,
            }),
        }
    }
    let mut files = Vec::with_capacity(targets.len());
    stream::entries(
        report,
        header,
        &|path| return targets.contains_key(path),
        &|_, path, file| {
            let parts = targets.get(&path).map_or(&[][..], Vec::as_slice);
            return evaluate_file(header, path, &file, parts);
        },
        &mut |result| {
            files.push(result);
            return Ok(());
        },
    )?;
    let measured: BTreeSet<&SourcePath> = files.iter().map(|file| return &file.path).collect();
    let missing = targets
        .into_iter()
        .filter(|(path, _)| return !measured.contains(path));
    let (unloaded, mut unmeasured) = split_unloaded(header, base, missing)?;
    unmeasured.extend(outside);
    let findings = files
        .iter()
        .flat_map(|file| return file.functions.iter())
        .fold(unloaded.len(), |total, function| {
            return total.saturating_add(function.findings());
        });
    let verdict = if findings == 0 {
        VerdictName::Pass
    } else {
        VerdictName::Findings
    };
    return Ok(GateResult {
        verdict,
        findings,
        files,
        unloaded,
        unmeasured,
    });
}

fn ranges(lines: &[Line]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<(Line, Line)> = None;
    for &line in lines {
        run = match run {
            Some((first, last)) if last.get().checked_add(1) == Some(line.get()) => {
                Some((first, line))
            }
            Some(previous) => {
                parts.push(render_run(previous));
                Some((line, line))
            }
            None => Some((line, line)),
        };
    }
    parts.extend(run.map(render_run));
    return parts.join(", ");
}

fn render_run((first, last): (Line, Line)) -> String {
    if first == last {
        return first.to_string();
    }
    return format!("{first}-{last}");
}

fn render_function(out: &mut dyn Write, function: &FunctionResult<'_>) -> io::Result<()> {
    let (first, last) = line_range(function.span);
    let lines = if first == last { "line" } else { "lines" };
    writeln!(
        out,
        "  {} ({lines} {})",
        function.key,
        render_run((first, last))
    )?;
    if !function.in_report {
        writeln!(out, "    not in the report: only its lines are checked")?;
    }
    if !function.uncovered_lines.is_empty() {
        writeln!(
            out,
            "    uncovered lines: {}",
            ranges(&function.uncovered_lines)
        )?;
    }
    for outcome in &function.unseen_outcomes {
        let at = outcome.span.map_or_else(
            || return format!("line {}", outcome.line),
            |span| return span.to_string(),
        );
        writeln!(
            out,
            "    unseen outcome {} {:?} at {at}",
            outcome.id, outcome.kind
        )?;
    }
    for looped in &function.loops_never_zero {
        writeln!(
            out,
            "    loop {} ({:?}, line {}) never ran zero times",
            looped.id, looped.kind, looped.line
        )?;
    }
    for looped in &function.loops_never_twice {
        writeln!(
            out,
            "    loop {} ({:?}, line {}) never ran two or more times",
            looped.id, looped.kind, looped.line
        )?;
    }
    for line in function
        .lines
        .iter()
        .filter(|line| return !line.tests.is_empty())
    {
        writeln!(out, "    line {}: {}", line.line, line.tests.join(", "))?;
    }
    return Ok(());
}

fn render_unloaded(out: &mut dyn Write, changed: &ChangedLines) -> io::Result<()> {
    let ChangedLines::Parts {
        lines,
        removed_before,
    } = changed
    else {
        return writeln!(out, "  never loaded by any test: the whole file");
    };
    let mut parts = Vec::with_capacity(2);
    if !lines.is_empty() {
        parts.push(format!("changed lines {}", ranges(lines)));
    }
    if !removed_before.is_empty() {
        parts.push(format!("lines removed before {}", ranges(removed_before)));
    }
    return writeln!(out, "  never loaded by any test: {}", parts.join("; "));
}

pub(crate) fn render_text(out: &mut dyn Write, result: &GateResult<'_>) -> io::Result<()> {
    let mut checked = 0_usize;
    for file in &result.files {
        writeln!(out, "{}", file.path)?;
        for function in &file.functions {
            render_function(out, function)?;
            checked = checked.saturating_add(1);
        }
    }
    for file in &result.unloaded {
        writeln!(out, "{}", file.path)?;
        render_unloaded(out, &file.changed)?;
    }
    for unmeasured in &result.unmeasured {
        writeln!(
            out,
            "not in the report: {} ({})",
            unmeasured.path, unmeasured.reason
        )?;
    }
    if result.verdict == VerdictName::Pass {
        return writeln!(out, "gate: pass, {checked} changed function(s) checked");
    }
    write!(
        out,
        "gate: {} finding(s) in {checked} changed function(s)",
        result.findings
    )?;
    if !result.unloaded.is_empty() {
        write!(
            out,
            " and {} changed file(s) no test loaded",
            result.unloaded.len()
        )?;
    }
    return writeln!(out);
}

pub(crate) fn render_json(out: &mut dyn Write, result: &GateResult<'_>) -> io::Result<()> {
    serde_json::to_writer(&mut *out, result)?;
    return writeln!(out);
}
