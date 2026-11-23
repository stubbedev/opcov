use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use opcov_core::cfg::{ClassRef, FunctionKey, Line, LineSpan};
use opcov_core::report::{FileCoverage, Function, HitSet, Identified, PathBase, SourcePath};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Taken {
    Unreached,
    Tests(usize),
}

impl Taken {
    pub(crate) const fn covered(self) -> bool {
        return matches!(self, Self::Tests(count) if count > 0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Branch {
    pub(crate) line: Line,
    pub(crate) block: u64,
    pub(crate) number: u64,
    pub(crate) taken: Taken,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub(crate) total: u64,
    pub(crate) covered: u64,
}

impl Tally {
    pub(crate) const fn add(&mut self, covered: bool) {
        self.total = self.total.saturating_add(1);
        if covered {
            self.covered = self.covered.saturating_add(1);
        }
    }

    pub(crate) const fn absorb(&mut self, other: Self) {
        self.total = self.total.saturating_add(other.total);
        self.covered = self.covered.saturating_add(other.covered);
    }

    pub(crate) const fn uncovered(self) -> u64 {
        return self.total.saturating_sub(self.covered);
    }

    pub(crate) const fn ratio(self) -> Ratio {
        return Ratio(self);
    }

    pub(crate) const fn percent(self) -> Percent {
        return Percent(self);
    }
}

fn scaled(tally: Tally, scale: u128) -> Option<u128> {
    return u128::from(tally.covered)
        .saturating_mul(scale)
        .checked_div(u128::from(tally.total));
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Ratio(Tally);

impl fmt::Display for Ratio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const SCALE: u128 = 10_000;
        let Some(permyriad) = scaled(self.0, SCALE) else {
            return formatter.write_str("1");
        };
        let whole = permyriad.checked_div(SCALE).unwrap_or_default();
        let fraction = permyriad.checked_rem(SCALE).unwrap_or_default();
        if fraction == 0 {
            return write!(formatter, "{whole}");
        }
        let digits = format!("{fraction:04}");
        return write!(formatter, "{whole}.{}", digits.trim_end_matches('0'));
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Percent(Tally);

impl fmt::Display for Percent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let percent = scaled(self.0, 100).unwrap_or(100);
        return write!(formatter, "{percent}%");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unit<'r> {
    File,
    Class(&'r ClassRef),
}

#[derive(Debug)]
pub(crate) struct FunctionView<'r> {
    pub(crate) key: &'r FunctionKey,
    pub(crate) range: Option<LineSpan>,
    pub(crate) unit: Unit<'r>,
    pub(crate) branches: Vec<Branch>,
    pub(crate) tests: usize,
    pub(crate) first_line: Option<Line>,
}

impl FunctionView<'_> {
    pub(crate) fn start(&self) -> Option<Line> {
        return self.range.map(LineSpan::start);
    }

    pub(crate) fn entry(&self) -> Option<Line> {
        return self.first_line.or_else(|| return self.start());
    }

    pub(crate) fn member_name(&self) -> String {
        return match self.key {
            FunctionKey::Method { method, .. } => method.as_str().to_owned(),
            FunctionKey::PropertyHook { property, hook, .. } => {
                format!("${}::{hook}", property.as_str())
            }
            other @ (FunctionKey::Main { .. }
            | FunctionKey::Function { .. }
            | FunctionKey::Closure { .. }) => other.to_string(),
        };
    }
}

#[derive(Debug)]
pub(crate) struct LineView<'r> {
    pub(crate) line: Line,
    pub(crate) hits: Option<&'r HitSet>,
    pub(crate) owner: Option<usize>,
    pub(crate) branches: Tally,
}

impl LineView<'_> {
    pub(crate) fn tests(&self) -> usize {
        return self.hits.map_or(0, |hits| return hits.as_slice().len());
    }

    pub(crate) const fn executable(&self) -> bool {
        return self.hits.is_some();
    }

    pub(crate) fn covered(&self) -> bool {
        return self.tests() > 0;
    }
}

#[derive(Debug)]
pub(crate) struct FileView<'r> {
    pub(crate) path: &'r SourcePath,
    pub(crate) location: String,
    pub(crate) lines: Vec<LineView<'r>>,
    pub(crate) functions: Vec<FunctionView<'r>>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Totals {
    pub(crate) lines: Tally,
    pub(crate) branches: Tally,
    pub(crate) functions: Tally,
}

impl Totals {
    pub(crate) const fn absorb(&mut self, other: Self) {
        self.lines.absorb(other.lines);
        self.branches.absorb(other.branches);
        self.functions.absorb(other.functions);
    }
}

impl FileView<'_> {
    pub(crate) fn unit_of_line(&self, line: &LineView<'_>) -> Unit<'_> {
        return line
            .owner
            .and_then(|owner| return self.functions.get(owner))
            .map_or(Unit::File, |function| return function.unit);
    }

    pub(crate) fn totals(&self, unit: Option<Unit<'_>>) -> Totals {
        let mut totals = line_totals(self.lines.iter().filter(|line| {
            return unit.is_none_or(|wanted| return self.unit_of_line(line) == wanted);
        }));
        for function in &self.functions {
            if function.range.is_none() || unit.is_some_and(|wanted| return function.unit != wanted)
            {
                continue;
            }
            totals.functions.add(function.tests > 0);
        }
        return totals;
    }

    pub(crate) fn units(&self) -> BTreeSet<Unit<'_>> {
        let mut units: BTreeSet<Unit<'_>> = self
            .functions
            .iter()
            .map(|function| return function.unit)
            .collect();
        units.extend(self.lines.iter().map(|line| return self.unit_of_line(line)));
        return units;
    }
}

pub(crate) fn line_totals<'l>(lines: impl Iterator<Item = &'l LineView<'l>>) -> Totals {
    let mut totals = Totals::default();
    for line in lines {
        if line.executable() {
            totals.lines.add(line.covered());
        }
        totals.branches.absorb(line.branches);
    }
    return totals;
}

pub(crate) fn location(paths: &PathBase, path: &SourcePath) -> String {
    return match paths {
        PathBase::Absolute => path.as_str().to_owned(),
        PathBase::Relative(root) if root.as_str() == "/" => format!("/{path}"),
        PathBase::Relative(root) => format!("{}/{path}", root.as_str()),
    };
}

const fn key_lines(key: &FunctionKey) -> Option<LineSpan> {
    return match key {
        FunctionKey::Closure { lines, .. } => Some(*lines),
        FunctionKey::Main { .. }
        | FunctionKey::Function { .. }
        | FunctionKey::Method { .. }
        | FunctionKey::PropertyHook { .. } => None,
    };
}

fn range_of(key: &FunctionKey, function: &Function) -> Option<LineSpan> {
    return function
        .span
        .and_then(|span| return LineSpan::new(span.start().0, span.end().0))
        .or_else(|| return key_lines(key));
}

fn contains(outer: LineSpan, inner: LineSpan) -> bool {
    return outer.start() <= inner.start() && inner.end() <= outer.end();
}

fn innermost(candidates: impl Iterator<Item = (usize, LineSpan)>) -> Option<usize> {
    return candidates
        .max_by_key(|&(index, span)| return (span.start(), Reverse(span.end()), Reverse(index)))
        .map(|(index, _)| return index);
}

pub(crate) const fn is_nested(key: &FunctionKey) -> bool {
    return matches!(key, FunctionKey::Closure { .. });
}

fn units<'r>(keys: &[(&'r FunctionKey, Option<LineSpan>)]) -> Vec<Unit<'r>> {
    let direct = |key: &'r FunctionKey| -> Option<Unit<'r>> {
        return match key {
            FunctionKey::Method { class, .. } | FunctionKey::PropertyHook { class, .. } => {
                Some(Unit::Class(class))
            }
            FunctionKey::Main { .. } | FunctionKey::Function { .. } => Some(Unit::File),
            FunctionKey::Closure { .. } => None,
        };
    };
    return keys
        .iter()
        .map(|&(key, range)| {
            if let Some(unit) = direct(key) {
                return unit;
            }
            let Some(range) = range else {
                return Unit::File;
            };
            let containers = keys
                .iter()
                .enumerate()
                .filter_map(|(index, (other, span))| {
                    let span = (*span)?;
                    return (!is_nested(other) && contains(span, range)).then_some((index, span));
                });
            return innermost(containers)
                .and_then(|index| return keys.get(index))
                .and_then(|(container, _)| return direct(container))
                .unwrap_or(Unit::File);
        })
        .collect();
}

fn taken(lines: &BTreeMap<Line, HitSet>, line: Line, hits: &HitSet) -> Taken {
    let reached = lines
        .get(&line)
        .is_some_and(|tests| return !tests.is_empty());
    if !reached && hits.is_empty() {
        return Taken::Unreached;
    }
    return Taken::Tests(hits.as_slice().len());
}

fn branches(lines: &BTreeMap<Line, HitSet>, ordinal: u64, function: &Function) -> Vec<Branch> {
    let outcome_block = ordinal.saturating_mul(2);
    let loop_block = outcome_block.saturating_add(1);
    let outcomes = function
        .outcomes
        .as_slice()
        .iter()
        .filter(|outcome| return outcome.kind().get().counts_toward_verdict())
        .map(|outcome| {
            return Branch {
                line: outcome.line(),
                block: outcome_block,
                number: u64::from(outcome.id().get()),
                taken: taken(lines, outcome.line(), outcome.hits()),
            };
        });
    let loops = function.loops.as_slice().iter().flat_map(|looped| {
        let zero = u64::from(looped.id.get()).saturating_mul(2);
        return [
            (zero, &looped.zero),
            (zero.saturating_add(1), &looped.second),
        ]
        .map(|(number, hits)| {
            return Branch {
                line: looped.line,
                block: loop_block,
                number,
                taken: taken(lines, looped.line, hits),
            };
        });
    });
    return outcomes.chain(loops).collect();
}

fn owner(ranges: &[(&FunctionKey, Option<LineSpan>)], line: Line) -> Option<usize> {
    let containing = ranges.iter().enumerate().filter_map(|(index, (_, range))| {
        let range = (*range)?;
        return range_has(range, line).then_some((index, range));
    });
    return innermost(containing);
}

fn range_has(range: LineSpan, line: Line) -> bool {
    return range.start() <= line && line <= range.end();
}

pub(crate) fn file_view<'r>(
    paths: &PathBase,
    path: &'r SourcePath,
    file: &'r FileCoverage,
) -> FileView<'r> {
    let ranges: Vec<(&FunctionKey, Option<LineSpan>)> = file
        .functions
        .iter()
        .map(|(key, function)| return (key, range_of(key, function)))
        .collect();
    let units = units(&ranges);
    let mut branch_lines: BTreeMap<Line, Tally> = BTreeMap::new();
    let mut functions: Vec<FunctionView<'r>> = Vec::with_capacity(ranges.len());
    for (((key, function), (_, range)), (unit, ordinal)) in file
        .functions
        .iter()
        .zip(&ranges)
        .zip(units.into_iter().zip(0_u64..))
    {
        let branches = branches(&file.lines, ordinal, function);
        for branch in &branches {
            branch_lines
                .entry(branch.line)
                .or_default()
                .add(branch.taken.covered());
        }
        functions.push(FunctionView {
            key,
            range: *range,
            unit,
            branches,
            tests: 0,
            first_line: None,
        });
    }
    let mut line_numbers: BTreeSet<Line> = file.lines.keys().copied().collect();
    line_numbers.extend(branch_lines.keys().copied());
    let mut function_tests: Vec<BTreeSet<_>> = vec![BTreeSet::new(); functions.len()];
    let lines = line_numbers
        .into_iter()
        .map(|line| {
            let hits = file.lines.get(&line);
            let owner = owner(&ranges, line);
            if let (Some(owner), Some(hits)) = (owner, hits) {
                if let Some(tests) = function_tests.get_mut(owner) {
                    tests.extend(hits.as_slice().iter().copied());
                }
                if let Some(function) = functions.get_mut(owner) {
                    function.first_line.get_or_insert(line);
                }
            }
            return LineView {
                line,
                hits,
                owner,
                branches: branch_lines.get(&line).copied().unwrap_or_default(),
            };
        })
        .collect();
    for (function, tests) in functions.iter_mut().zip(function_tests) {
        function.tests = tests.len();
    }
    return FileView {
        path,
        location: location(paths, path),
        lines,
        functions,
    };
}
