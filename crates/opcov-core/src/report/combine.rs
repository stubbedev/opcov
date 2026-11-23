use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::btree_map;
use std::convert::Infallible;
use std::error::Error;
use std::fmt;

use serde::Serialize;

use super::error::ModelError;
use super::model::{
    FileCoverage, Function, Generator, Header, HitSet, IdSorted, Identified, Loop, LoopId, Outcome,
    OutcomeId, Report, Settings, SourcePath, TestIndex, TestRefs, TestTable,
};
use crate::cfg::FunctionKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputId(usize);

impl InputId {
    #[must_use]
    pub const fn new(position: usize) -> Self {
        return Self(position);
    }

    #[must_use]
    pub const fn get(self) -> usize {
        return self.0;
    }
}

impl fmt::Display for InputId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(formatter, "input #{}", self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderField {
    GeneratorOpcov,
    GeneratorPhp,
    GeneratorZts,
    GeneratorVm,
    SettingsCollectPaths,
    SettingsExcludePaths,
    SettingsExcludePattern,
    Paths,
}

impl fmt::Display for HeaderField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(match self {
            Self::GeneratorOpcov => "generator.opcov",
            Self::GeneratorPhp => "generator.php",
            Self::GeneratorZts => "generator.zts",
            Self::GeneratorVm => "generator.vm",
            Self::SettingsCollectPaths => "settings.collect_paths",
            Self::SettingsExcludePaths => "settings.exclude_paths",
            Self::SettingsExcludePattern => "settings.exclude_pattern",
            Self::Paths => "paths",
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeField {
    Site,
    Kind,
    Line,
    Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopField {
    Kind,
    Line,
    Zero,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictSite {
    Sha256,
    FunctionSpan {
        function: FunctionKey,
    },
    Outcome {
        function: FunctionKey,
        id: OutcomeId,
        field: OutcomeField,
    },
    Loop {
        function: FunctionKey,
        id: LoopId,
        field: LoopField,
    },
}

impl fmt::Display for ConflictSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Sha256 => formatter.write_str("sha256"),
            Self::FunctionSpan { function } => write!(formatter, "span of function `{function}`"),
            Self::Outcome {
                function,
                id,
                field,
            } => {
                let field = match field {
                    OutcomeField::Site => "site",
                    OutcomeField::Kind => "kind",
                    OutcomeField::Line => "line",
                    OutcomeField::Span => "span",
                };
                write!(
                    formatter,
                    "{field} of outcome {id} in function `{function}`"
                )
            }
            Self::Loop {
                function,
                id,
                field,
            } => {
                let field = match field {
                    LoopField::Kind => "kind",
                    LoopField::Line => "line",
                    LoopField::Zero => "zero",
                };
                write!(formatter, "{field} of loop {id} in function `{function}`")
            }
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderMismatch {
    pub field: HeaderField,
    pub first: InputId,
    pub second: InputId,
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryConflict {
    pub path: SourcePath,
    pub site: ConflictSite,
    pub first: InputId,
    pub second: InputId,
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    NoInputs,
    Header(Box<HeaderMismatch>),
    Conflict(Box<EntryConflict>),
    MergedTests(Box<ModelError>),
    Invalid {
        input: InputId,
        error: Box<ModelError>,
    },
}

impl MergeError {
    #[must_use]
    pub const fn is_refusal(&self) -> bool {
        return matches!(self, Self::Header(_) | Self::Conflict(_));
    }

    #[must_use]
    pub fn describe<'a, N: fmt::Display>(&'a self, names: &'a [N]) -> impl fmt::Display + 'a {
        return Described { error: self, names };
    }

    pub(super) fn render(
        &self,
        formatter: &mut fmt::Formatter<'_>,
        name: &dyn Fn(InputId) -> String,
    ) -> fmt::Result {
        return match self {
            Self::NoInputs => formatter.write_str("no reports to merge"),
            Self::Header(mismatch) => write!(
                formatter,
                "refusing to merge: {} differs: {} in {} but {} in {}",
                mismatch.field,
                mismatch.left,
                name(mismatch.first),
                mismatch.right,
                name(mismatch.second)
            ),
            Self::Conflict(conflict) => write!(
                formatter,
                "refusing to merge {}: {} differs: {} in {} but {} in {}",
                conflict.path,
                conflict.site,
                conflict.left,
                name(conflict.first),
                conflict.right,
                name(conflict.second)
            ),
            Self::MergedTests(error) => write!(formatter, "merged test table: {error}"),
            Self::Invalid { input, error } => write!(formatter, "{}: {error}", name(*input)),
        };
    }
}

struct Described<'a, N> {
    error: &'a MergeError,
    names: &'a [N],
}

impl<N: fmt::Display> fmt::Display for Described<'_, N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return self.error.render(formatter, &|input| {
            return self
                .names
                .get(input.0)
                .map_or_else(|| return input.to_string(), ToString::to_string);
        });
    }
}

impl fmt::Display for MergeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return self.render(formatter, &|input| return input.to_string());
    }
}

impl Error for MergeError {}

fn render<T: Serialize + ?Sized>(value: &T) -> String {
    return serde_json::to_string(value)
        .unwrap_or_else(|error| return format!("<unrenderable: {error}>"));
}

fn differ<T: PartialEq + Serialize>(
    field: HeaderField,
    left: &T,
    right: &T,
) -> Option<(HeaderField, String, String)> {
    return (left != right).then(|| return (field, render(left), render(right)));
}

fn first_difference(left: &Header, right: &Header) -> Option<(HeaderField, String, String)> {
    let Header {
        generator:
            Generator {
                opcov,
                php,
                zts,
                vm,
            },
        settings:
            Settings {
                collect_paths,
                exclude_paths,
                exclude_pattern,
            },
        paths,
        tests: _,
    } = left;
    return differ(HeaderField::GeneratorOpcov, opcov, &right.generator.opcov)
        .or_else(|| return differ(HeaderField::GeneratorPhp, php, &right.generator.php))
        .or_else(|| return differ(HeaderField::GeneratorZts, zts, &right.generator.zts))
        .or_else(|| return differ(HeaderField::GeneratorVm, vm, &right.generator.vm))
        .or_else(|| {
            return differ(
                HeaderField::SettingsCollectPaths,
                collect_paths,
                &right.settings.collect_paths,
            );
        })
        .or_else(|| {
            return differ(
                HeaderField::SettingsExcludePaths,
                exclude_paths,
                &right.settings.exclude_paths,
            );
        })
        .or_else(|| {
            return differ(
                HeaderField::SettingsExcludePattern,
                exclude_pattern,
                &right.settings.exclude_pattern,
            );
        })
        .or_else(|| return differ(HeaderField::Paths, paths, &right.paths));
}

pub(super) struct Remap(Vec<TestIndex>);

impl TestRefs for Remap {
    fn resolve(&self, index: TestIndex) -> Option<TestIndex> {
        return self.0.get(index.position()?).copied();
    }

    fn count(&self) -> usize {
        return self.0.len();
    }
}

pub(super) struct MergePlan {
    pub(super) header: Header,
    pub(super) remaps: Vec<Remap>,
}

pub(super) fn plan(headers: &[&Header]) -> Result<MergePlan, MergeError> {
    let Some(first) = headers.first() else {
        return Err(MergeError::NoInputs);
    };
    for (position, other) in headers.iter().enumerate().skip(1) {
        if let Some((field, left, right)) = first_difference(first, other) {
            return Err(MergeError::Header(Box::new(HeaderMismatch {
                field,
                first: InputId(0),
                second: InputId(position),
                left,
                right,
            })));
        }
    }
    let mut cursors: Vec<_> = headers
        .iter()
        .map(|header| return header.tests.names().iter().peekable())
        .collect();
    let mut remaps: Vec<Vec<TestIndex>> = headers
        .iter()
        .map(|header| return Vec::with_capacity(header.tests.len()))
        .collect();
    let mut names: Vec<String> = Vec::new();
    while let Some(next) = cursors
        .iter_mut()
        .filter_map(|cursor| return cursor.peek().copied())
        .min()
    {
        let global = u32::try_from(names.len()).map_err(|_overflow| {
            return MergeError::MergedTests(Box::new(ModelError::TooManyTests {
                count: names.len(),
            }));
        })?;
        for (cursor, remap) in cursors.iter_mut().zip(remaps.iter_mut()) {
            if cursor.next_if_eq(&next).is_some() {
                remap.push(TestIndex::new(global));
            }
        }
        names.push(next.clone());
    }
    return Ok(MergePlan {
        header: Header {
            generator: first.generator.clone(),
            settings: first.settings.clone(),
            paths: first.paths.clone(),
            tests: TestTable(names),
        },
        remaps: remaps.into_iter().map(Remap).collect(),
    });
}

fn merge_sorted<T, K: Ord, E>(
    left: Vec<T>,
    right: Vec<T>,
    key: impl Fn(&T) -> K,
    mut both: impl FnMut(T, T) -> Result<T, E>,
) -> Result<Vec<T>, E> {
    let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    loop {
        let order = match (left.peek(), right.peek()) {
            (None, None) => return Ok(merged),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(next_left), Some(next_right)) => key(next_left).cmp(&key(next_right)),
        };
        match order {
            Ordering::Less => merged.extend(left.next()),
            Ordering::Greater => merged.extend(right.next()),
            Ordering::Equal => {
                if let (Some(next_left), Some(next_right)) = (left.next(), right.next()) {
                    merged.push(both(next_left, next_right)?);
                }
            }
        }
    }
}

fn union_hits(left: HitSet, right: HitSet) -> HitSet {
    let Ok(merged) = merge_sorted(
        left.0,
        right.0,
        |index| return *index,
        |kept, _duplicate| return Ok::<_, Infallible>(kept),
    );
    return HitSet(merged);
}

fn union_maps<K: Ord, V, E>(
    mut left: BTreeMap<K, V>,
    right: BTreeMap<K, V>,
    mut both: impl FnMut(&K, V, V) -> Result<V, E>,
) -> Result<BTreeMap<K, V>, E> {
    for (key, value) in right {
        let merged = match left.remove(&key) {
            Some(existing) => both(&key, existing, value)?,
            None => value,
        };
        left.insert(key, merged);
    }
    return Ok(left);
}

struct Clash {
    site: ConflictSite,
    left: String,
    right: String,
}

fn clash_if<T: PartialEq + Serialize>(
    left: &T,
    right: &T,
    site: impl FnOnce() -> ConflictSite,
) -> Result<(), Box<Clash>> {
    if left == right {
        return Ok(());
    }
    return Err(Box::new(Clash {
        site: site(),
        left: render(left),
        right: render(right),
    }));
}

fn union_outcomes(
    function: &FunctionKey,
    left: Outcome,
    right: Outcome,
) -> Result<Outcome, Box<Clash>> {
    let site = |field| {
        return move || {
            return ConflictSite::Outcome {
                function: function.clone(),
                id: left.id,
                field,
            };
        };
    };
    clash_if(&left.site, &right.site, site(OutcomeField::Site))?;
    clash_if(&left.kind, &right.kind, site(OutcomeField::Kind))?;
    clash_if(&left.line, &right.line, site(OutcomeField::Line))?;
    clash_if(&left.span, &right.span, site(OutcomeField::Span))?;
    return Ok(Outcome {
        hits: union_hits(left.hits, right.hits),
        ..left
    });
}

fn union_loops(function: &FunctionKey, left: Loop, right: Loop) -> Result<Loop, Box<Clash>> {
    let site = |field| {
        return move || {
            return ConflictSite::Loop {
                function: function.clone(),
                id: left.id,
                field,
            };
        };
    };
    clash_if(&left.kind, &right.kind, site(LoopField::Kind))?;
    clash_if(&left.line, &right.line, site(LoopField::Line))?;
    clash_if(
        &left.zero.is_some(),
        &right.zero.is_some(),
        site(LoopField::Zero),
    )?;
    return Ok(Loop {
        zero: left
            .zero
            .zip(right.zero)
            .map(|(left, right)| return union_hits(left, right)),
        second: union_hits(left.second, right.second),
        ..left
    });
}

fn union_functions(
    key: &FunctionKey,
    left: Function,
    right: Function,
) -> Result<Function, Box<Clash>> {
    clash_if(&left.span, &right.span, || {
        return ConflictSite::FunctionSpan {
            function: key.clone(),
        };
    })?;
    let outcomes = merge_sorted(
        left.outcomes.0,
        right.outcomes.0,
        Identified::id,
        |kept, other| return union_outcomes(key, kept, other),
    )?;
    let loops = merge_sorted(
        left.loops.0,
        right.loops.0,
        Identified::id,
        |kept, other| return union_loops(key, kept, other),
    )?;
    return Ok(Function {
        span: left.span,
        outcomes: IdSorted(outcomes),
        loops: IdSorted(loops),
    });
}

fn union_files(left: FileCoverage, right: FileCoverage) -> Result<FileCoverage, Box<Clash>> {
    clash_if(&left.sha256, &right.sha256, || return ConflictSite::Sha256)?;
    let lines = union_maps(left.lines, right.lines, |_line, kept, other| {
        return Ok::<_, Box<Clash>>(union_hits(kept, other));
    })?;
    let functions = union_maps(left.functions, right.functions, union_functions)?;
    return Ok(FileCoverage {
        sha256: left.sha256,
        lines,
        functions,
    });
}

pub(super) trait EntrySource {
    type Error: From<MergeError>;
    fn input(&self) -> InputId;
    fn head(&self) -> Option<&SourcePath>;
    fn take(&mut self) -> Result<Option<FileCoverage>, Self::Error>;
}

pub(super) fn merge_entries<S: EntrySource>(
    sources: &mut [S],
    mut emit: impl FnMut(SourcePath, FileCoverage) -> Result<(), S::Error>,
) -> Result<(), S::Error> {
    loop {
        let Some(path) = sources.iter().filter_map(EntrySource::head).min().cloned() else {
            return Ok(());
        };
        let mut merged: Option<(InputId, FileCoverage)> = None;
        for source in &mut *sources {
            if source.head() != Some(&path) {
                continue;
            }
            let input = source.input();
            let Some(file) = source.take()? else {
                continue;
            };
            merged = Some(match merged {
                None => (input, file),
                Some((first, kept)) => {
                    let union = union_files(kept, file).map_err(|clash| {
                        return MergeError::Conflict(Box::new(EntryConflict {
                            path: path.clone(),
                            site: clash.site,
                            first,
                            second: input,
                            left: clash.left,
                            right: clash.right,
                        }));
                    })?;
                    (first, union)
                }
            });
        }
        if let Some((_, file)) = merged {
            emit(path, file)?;
        }
    }
}

struct MemorySource<'a> {
    input: InputId,
    remap: &'a Remap,
    rest: btree_map::Iter<'a, SourcePath, FileCoverage>,
    head: Option<(&'a SourcePath, &'a FileCoverage)>,
}

impl EntrySource for MemorySource<'_> {
    type Error = MergeError;

    fn input(&self) -> InputId {
        return self.input;
    }

    fn head(&self) -> Option<&SourcePath> {
        return self.head.map(|(path, _)| return path);
    }

    fn take(&mut self) -> Result<Option<FileCoverage>, MergeError> {
        let Some((path, file)) = self.head else {
            return Ok(None);
        };
        self.head = self.rest.next();
        let resolved = file.clone().resolve_tests(self.remap).map_err(|bad| {
            return MergeError::Invalid {
                input: self.input,
                error: Box::new(bad.into_error(path, self.remap.count())),
            };
        })?;
        return Ok(Some(resolved));
    }
}

pub fn merge(reports: &[Report]) -> Result<Report, MergeError> {
    let headers: Vec<&Header> = reports.iter().map(Report::header).collect();
    let MergePlan { header, remaps } = plan(&headers)?;
    let mut sources: Vec<MemorySource<'_>> = reports
        .iter()
        .zip(&remaps)
        .enumerate()
        .map(|(position, (report, remap))| {
            let mut rest = report.files().iter();
            let head = rest.next();
            return MemorySource {
                input: InputId(position),
                remap,
                rest,
                head,
            };
        })
        .collect();
    let mut files = BTreeMap::new();
    merge_entries(&mut sources, |path, file| {
        files.insert(path, file);
        return Ok(());
    })?;
    return Ok(Report::from_validated(header, files));
}
