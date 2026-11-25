use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use opcov_core::cfg::{ClassRef, FunctionKey, Line};
use opcov_core::filter::Selection;
use opcov_core::report::{FileCoverage, Header, HitSet, Sha256, SourcePath, TestIndex};
use opcov_syntax::{Outline, SourceFile};
use serde::{Serialize, Serializer};

use crate::changes::{Content, FileChange, FilePatch, base_change, normalize, original, resolve};
use crate::failure::{Failure, Verdict};
use crate::gate::{
    PHP_EXTENSION, Unmeasurable, changed_functions, owner_of_line, selection, unmeasurable,
};
use crate::source;
use crate::stream;

const STALE_SHOWN: usize = 10;

#[derive(Debug, Clone, Copy, Serialize)]
struct Coverage {
    covered: usize,
    total: usize,
}

impl Coverage {
    fn of<'h>(hits: impl Iterator<Item = &'h HitSet>) -> Self {
        let mut coverage = Self {
            covered: 0,
            total: 0,
        };
        for hit in hits {
            coverage.total = coverage.total.saturating_add(1);
            if !hit.is_empty() {
                coverage.covered = coverage.covered.saturating_add(1);
            }
        }
        return coverage;
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FunctionImpact<'r> {
    key: String,
    first: Line,
    last: Line,
    lines: Coverage,
    outcomes: Coverage,
    tests: Vec<&'r str>,
}

#[derive(Debug)]
enum Unbounded {
    Declarations { line: Option<Line> },
    Binary,
    NotPhp,
    OutsideRoot,
    Unmeasured(Unmeasurable),
}

impl fmt::Display for Unbounded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Declarations { line: Some(line) } => write!(
                formatter,
                "line {line} changes declarations outside function bodies, whose readers coverage cannot tell"
            ),
            Self::Declarations { line: None } => formatter.write_str(
                "declarations outside function bodies changed, whose readers coverage cannot tell",
            ),
            Self::Binary => formatter.write_str("a binary file"),
            Self::NotPhp => write!(formatter, "not a .{PHP_EXTENSION} file"),
            Self::OutsideRoot => formatter.write_str("outside the report's root"),
            Self::Unmeasured(reason) => write!(
                formatter,
                "neither a test class nor measured code ({reason}), so the tests that reach it are unknown"
            ),
        };
    }
}

impl Serialize for Unbounded {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.collect_str(self);
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
enum Effect<'r> {
    Functions { functions: Vec<FunctionImpact<'r>> },
    WholeFile { tests: Vec<&'r str> },
    TestClasses { classes: Vec<String> },
    NewFile { classes: Vec<String> },
    NeverLoaded,
    Ignored,
    Unbounded { reason: Unbounded },
}

#[derive(Debug, Serialize)]
pub(crate) struct ChangeImpact<'r> {
    old: Option<String>,
    new: Option<String>,
    #[serde(flatten)]
    effect: Effect<'r>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum VerdictName {
    Bounded,
    Unbounded,
}

#[derive(Debug, Serialize)]
pub(crate) struct ImpactResult<'r> {
    verdict: VerdictName,
    tests: Vec<&'r str>,
    classes: Vec<String>,
    changes: Vec<ChangeImpact<'r>>,
}

impl ImpactResult<'_> {
    pub(crate) const fn verdict(&self) -> Verdict {
        return match self.verdict {
            VerdictName::Bounded => Verdict::Clean,
            VerdictName::Unbounded => Verdict::Findings,
        };
    }
}

struct Version {
    file: SourceFile,
    outline: Outline,
}

impl Version {
    fn parse(path: &SourcePath, bytes: &[u8]) -> Result<Self, Failure> {
        let key_file =
            source::key_file(path).map_err(|error| return Failure::failed(error.to_string()))?;
        let (file, outline) = SourceFile::parse_outlined(key_file, bytes)
            .map_err(|error| return Failure::failed(error.to_string()))?;
        return Ok(Self { file, outline });
    }

    fn classes(&self) -> BTreeSet<String> {
        return self
            .file
            .functions()
            .iter()
            .filter_map(|function| return class_name(function.key()))
            .collect();
    }

    fn only_methods(&self) -> bool {
        return self.file.functions().iter().all(|function| {
            return matches!(
                function.key(),
                FunctionKey::Main { .. }
                    | FunctionKey::Closure { .. }
                    | FunctionKey::Method { .. }
                    | FunctionKey::PropertyHook { .. }
            );
        });
    }
}

fn class_name(key: &FunctionKey) -> Option<String> {
    return match key {
        FunctionKey::Method {
            class: ClassRef::Named(name),
            ..
        }
        | FunctionKey::PropertyHook {
            class: ClassRef::Named(name),
            ..
        } => Some(name.to_string()),
        FunctionKey::Main { .. }
        | FunctionKey::Function { .. }
        | FunctionKey::Method { .. }
        | FunctionKey::PropertyHook { .. }
        | FunctionKey::Closure { .. } => None,
    };
}

fn test_class(name: &str) -> Option<String> {
    return name
        .split_once("::")
        .map(|(class, _)| return class.to_ascii_lowercase());
}

enum Decided {
    TestClasses(Vec<String>),
    NewFile(Vec<String>),
    Unbounded(Unbounded),
}

impl From<Decided> for Effect<'_> {
    fn from(decided: Decided) -> Self {
        return match decided {
            Decided::TestClasses(classes) => Self::TestClasses { classes },
            Decided::NewFile(classes) => Self::NewFile { classes },
            Decided::Unbounded(reason) => Self::Unbounded { reason },
        };
    }
}

struct Measured<'p> {
    index: usize,
    old: SourcePath,
    patch: &'p FilePatch,
    base_sha: Sha256,
    base: Version,
    current: Option<Version>,
}

enum Planned<'p> {
    Decided(Decided),
    Measured(Measured<'p>),
}

struct Plan<'p> {
    measured: BTreeMap<SourcePath, Measured<'p>>,
    touched: BTreeSet<SourcePath>,
}

fn is_php(path: &Path) -> bool {
    return path
        .extension()
        .is_some_and(|extension| return extension == PHP_EXTENSION);
}

fn read(disk: &Path) -> Result<Vec<u8>, Failure> {
    return fs::read(disk).map_err(|error| return Failure::io("read", disk, &error));
}

fn ignored(ignore: &[PathBuf], base: &Path, path: &Path) -> bool {
    let path = normalize(&base.join(path));
    return ignore.iter().any(|prefix| return path.starts_with(prefix));
}

struct Context<'h> {
    header: &'h Header,
    base: &'h Path,
    test_classes: BTreeSet<String>,
    selection: Option<Selection>,
}

impl Context<'_> {
    fn is_test_file(&self, versions: &[&Version]) -> bool {
        let classes: BTreeSet<String> = versions
            .iter()
            .flat_map(|version| return version.classes())
            .collect();
        return !classes.is_empty()
            && versions.iter().all(|version| return version.only_methods())
            && classes
                .iter()
                .all(|class| return self.test_classes.contains(&class.to_ascii_lowercase()));
    }

    fn plan<'p>(&self, change: &'p FilePatch, index: usize) -> Result<Planned<'p>, Failure> {
        let Content::Text(hunks) = &change.content else {
            return Ok(Planned::Decided(Decided::Unbounded(Unbounded::Binary)));
        };
        if !change
            .old
            .iter()
            .chain(change.new.iter())
            .all(|path| return is_php(path))
        {
            return Ok(Planned::Decided(Decided::Unbounded(Unbounded::NotPhp)));
        }
        let layout = &self.header.paths;
        let resolved = |name: Option<&PathBuf>| {
            return name
                .map(|name| return resolve(layout, self.base, name).ok_or(()))
                .transpose();
        };
        let (Ok(old), Ok(new)) = (resolved(change.old.as_ref()), resolved(change.new.as_ref()))
        else {
            return Ok(Planned::Decided(Decided::Unbounded(Unbounded::OutsideRoot)));
        };
        let current_bytes = match &new {
            Some(source) => Some(read(&layout.disk_path(source))?),
            None => None,
        };
        let current = match (&new, &current_bytes) {
            (Some(source), Some(bytes)) => Some(Version::parse(source, bytes)?),
            _ => None,
        };
        let classes = || {
            return current
                .as_ref()
                .map_or_else(BTreeSet::new, Version::classes);
        };
        let Some(old) = old else {
            let classes = classes();
            return Ok(Planned::Decided(Decided::NewFile(
                classes.into_iter().collect(),
            )));
        };
        let base_bytes =
            original(current_bytes.as_deref().unwrap_or_default(), hunks).map_err(|error| {
                return Failure::failed(format!(
                    "cannot rebuild the base version of {old} from the diff: {error}"
                ));
            })?;
        let base = Version::parse(&old, &base_bytes)?;
        let versions: Vec<&Version> = std::iter::once(&base).chain(current.as_ref()).collect();
        if self.is_test_file(&versions) {
            let classes = classes();
            return Ok(Planned::Decided(Decided::TestClasses(
                classes.into_iter().collect(),
            )));
        }
        return Ok(Planned::Measured(Measured {
            index,
            old,
            patch: change,
            base_sha: Sha256::digest(&base_bytes),
            base,
            current,
        }));
    }

    fn unloaded<'r>(&mut self, path: &SourcePath) -> Result<Effect<'r>, Failure> {
        let disk = self.header.paths.disk_path(path);
        let chosen = match self.selection.take() {
            Some(chosen) => chosen,
            None => selection(&self.header.settings, self.base)?,
        };
        let decision = unmeasurable(&self.header.settings, &chosen, &disk);
        self.selection = Some(chosen);
        return Ok(decision.map_or(Effect::NeverLoaded, |reason| {
            return Effect::Unbounded {
                reason: Unbounded::Unmeasured(reason),
            };
        }));
    }
}

fn file_tests(file: &FileCoverage) -> BTreeSet<TestIndex> {
    let mut tests = BTreeSet::new();
    let mut add = |hits: &HitSet| tests.extend(hits.as_slice().iter().copied());
    file.lines.values().for_each(&mut add);
    for function in file.functions.values() {
        for outcome in function.outcomes.as_slice() {
            add(outcome.hits());
        }
        for looped in function.loops.as_slice() {
            if let Some(zero) = looped.zero() {
                add(zero);
            }
            add(looped.second());
        }
    }
    return tests;
}

fn names<'r>(header: &'r Header, tests: &BTreeSet<TestIndex>) -> Vec<&'r str> {
    return tests
        .iter()
        .filter_map(|index| return header.tests.name(*index))
        .collect();
}

fn function_impacts<'r>(
    header: &'r Header,
    file: &FileCoverage,
    base: &SourceFile,
    change: &FileChange,
) -> Vec<FunctionImpact<'r>> {
    let functions = base.functions();
    return changed_functions(functions, change)
        .into_iter()
        .filter_map(|index| {
            let source = functions.get(index)?;
            if matches!(source.key(), FunctionKey::Main { .. }) {
                return None;
            }
            let span = source.span();
            let (first, last) = (span.start().line(), span.end().line());
            let owned: Vec<&HitSet> = file
                .lines
                .range(first..=last)
                .filter(|(line, _)| return owner_of_line(functions, **line) == Some(index))
                .map(|(_, hits)| return hits)
                .collect();
            let measured = file.functions.get(source.key());
            let outcomes: Vec<&HitSet> = measured.map_or_else(Vec::new, |function| {
                return function
                    .outcomes
                    .as_slice()
                    .iter()
                    .filter(|outcome| return outcome.kind().get().counts_toward_verdict())
                    .map(|outcome| return outcome.hits())
                    .collect();
            });
            let mut tests: BTreeSet<TestIndex> = BTreeSet::new();
            for hits in owned.iter().chain(outcomes.iter()) {
                tests.extend(hits.as_slice().iter().copied());
            }
            if let Some(function) = measured {
                for looped in function.loops.as_slice() {
                    tests.extend(looped.second().as_slice().iter().copied());
                    if let Some(zero) = looped.zero() {
                        tests.extend(zero.as_slice().iter().copied());
                    }
                }
            }
            return Some(FunctionImpact {
                key: source.key().to_string(),
                first,
                last,
                lines: Coverage::of(owned.into_iter()),
                outcomes: Coverage::of(outcomes.into_iter()),
                tests: names(header, &tests),
            });
        })
        .collect();
}

fn measured_effect<'r>(
    header: &'r Header,
    measured: &Measured<'_>,
    file: &FileCoverage,
) -> Effect<'r> {
    let renamed = measured.patch.old != measured.patch.new;
    let Some(current) = measured.current.as_ref().filter(|_| return !renamed) else {
        return Effect::WholeFile {
            tests: names(header, &file_tests(file)),
        };
    };
    if let Some(divergence) = measured.base.outline.diverges_from(&current.outline) {
        return Effect::Unbounded {
            reason: Unbounded::Declarations {
                line: divergence.there().or_else(|| return divergence.here()),
            },
        };
    }
    let Content::Text(hunks) = &measured.patch.content else {
        return Effect::Unbounded {
            reason: Unbounded::Binary,
        };
    };
    return Effect::Functions {
        functions: function_impacts(header, file, &measured.base.file, &base_change(hunks)),
    };
}

enum Checked<'r> {
    Measured { index: usize, effect: Effect<'r> },
    Fresh,
    Stale(String),
}

fn check_entry<'r>(
    header: &'r Header,
    plan: &Plan<'_>,
    path: &SourcePath,
    file: &FileCoverage,
) -> Result<Checked<'r>, Failure> {
    if let Some(measured) = plan.measured.get(path) {
        if measured.base_sha != file.sha256 {
            return Ok(Checked::Stale(format!(
                "{path}: the diff's base has sha256 {}, the baseline records {}",
                measured.base_sha, file.sha256
            )));
        }
        return Ok(Checked::Measured {
            index: measured.index,
            effect: measured_effect(header, measured, file),
        });
    }
    if plan.touched.contains(path) {
        return Ok(Checked::Fresh);
    }
    let disk = header.paths.disk_path(path);
    return match fs::read(&disk) {
        Ok(bytes) if Sha256::digest(&bytes) == file.sha256 => Ok(Checked::Fresh),
        Ok(bytes) => Ok(Checked::Stale(format!(
            "{path}: the file has sha256 {}, the baseline records {}, and the diff does not change it",
            Sha256::digest(&bytes),
            file.sha256
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Checked::Stale(format!(
            "{path}: the file does not exist, and the diff does not delete it"
        ))),
        Err(error) => Err(Failure::io("read", &disk, &error)),
    };
}

fn stale_failure(report: &Path, stale: &[String]) -> Failure {
    let shown: Vec<&str> = stale.iter().take(STALE_SHOWN).map(String::as_str).collect();
    let more = stale.len().saturating_sub(shown.len());
    let mut message = format!(
        "the baseline {} was not built from the diff's base; diff against the commit the baseline was built from:\n  {}",
        report.display(),
        shown.join("\n  ")
    );
    if more > 0 {
        message = format!("{message}\n  and {more} more");
    }
    return Failure::failed(message);
}

fn display(path: Option<&PathBuf>) -> Option<String> {
    return path.map(|path| return path.display().to_string());
}

pub(crate) fn evaluate<'r>(
    report: &Path,
    header: &'r Header,
    patch: &crate::changes::Patch,
    base: &Path,
    ignore: &[PathBuf],
) -> Result<ImpactResult<'r>, Failure> {
    let ignore: Vec<PathBuf> = ignore
        .iter()
        .map(|path| return normalize(&base.join(path)))
        .collect();
    let mut context = Context {
        header,
        base,
        test_classes: header
            .tests
            .names()
            .iter()
            .filter_map(|name| return test_class(name))
            .collect(),
        selection: None,
    };
    let files = patch.files();
    let mut plan = Plan {
        measured: BTreeMap::new(),
        touched: BTreeSet::new(),
    };
    let mut effects: Vec<Option<Effect<'r>>> = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        let names = || return file.old.iter().chain(file.new.iter());
        plan.touched
            .extend(names().filter_map(|path| return resolve(&header.paths, base, path)));
        if names().any(|path| return ignored(&ignore, base, path)) {
            effects.push(Some(Effect::Ignored));
            continue;
        }
        match context.plan(file, index)? {
            Planned::Decided(decided) => effects.push(Some(decided.into())),
            Planned::Measured(measured) => {
                effects.push(None);
                plan.measured.insert(measured.old.clone(), measured);
            }
        }
    }
    let mut stale = Vec::new();
    let mut found = BTreeSet::new();
    stream::entries(
        report,
        header,
        &stream::source_files,
        &|_, path, file| {
            let checked = check_entry(header, &plan, &path, &file)?;
            return Ok((path, checked));
        },
        &mut |(path, checked)| {
            match checked {
                Checked::Measured { index, effect } => {
                    found.insert(path);
                    if let Some(slot) = effects.get_mut(index) {
                        *slot = Some(effect);
                    }
                }
                Checked::Fresh => {}
                Checked::Stale(message) => stale.push(message),
            }
            return Ok(());
        },
    )?;
    if !stale.is_empty() {
        return Err(stale_failure(report, &stale));
    }
    for (path, measured) in &plan.measured {
        if found.contains(path) {
            continue;
        }
        let effect = context.unloaded(path)?;
        if let Some(slot) = effects.get_mut(measured.index) {
            *slot = Some(effect);
        }
    }
    return aggregate(files, effects);
}

fn aggregate<'r>(
    files: &[FilePatch],
    effects: Vec<Option<Effect<'r>>>,
) -> Result<ImpactResult<'r>, Failure> {
    let mut changes = Vec::with_capacity(effects.len());
    let mut tests = BTreeSet::new();
    let mut classes = BTreeSet::new();
    let mut verdict = VerdictName::Bounded;
    for (file, effect) in files.iter().zip(effects) {
        let (old, new) = (display(file.old.as_ref()), display(file.new.as_ref()));
        let effect = effect.ok_or_else(|| {
            return Failure::failed(format!(
                "no effect was decided for the change of {}",
                new.as_deref().or(old.as_deref()).unwrap_or_default()
            ));
        })?;
        match &effect {
            Effect::Functions { functions } => {
                for function in functions {
                    tests.extend(function.tests.iter().copied());
                }
            }
            Effect::WholeFile { tests: whole } => tests.extend(whole.iter().copied()),
            Effect::TestClasses { classes: named } | Effect::NewFile { classes: named } => {
                classes.extend(named.iter().cloned());
            }
            Effect::NeverLoaded | Effect::Ignored => {}
            Effect::Unbounded { .. } => verdict = VerdictName::Unbounded,
        }
        changes.push(ChangeImpact { old, new, effect });
    }
    let lowered: BTreeSet<String> = classes
        .iter()
        .map(|class| return class.to_ascii_lowercase())
        .collect();
    tests.retain(|name: &&str| {
        return test_class(name).is_none_or(|class| return !lowered.contains(&class));
    });
    return Ok(ImpactResult {
        verdict,
        tests: tests.into_iter().collect(),
        classes: classes.into_iter().collect(),
        changes,
    });
}

fn render_change(out: &mut dyn Write, change: &ChangeImpact<'_>) -> io::Result<()> {
    let path = match (&change.old, &change.new) {
        (Some(old), Some(new)) if old != new => format!("{old} -> {new}"),
        (_, Some(path)) | (Some(path), None) => path.clone(),
        (None, None) => String::new(),
    };
    writeln!(out, "{path}")?;
    return match &change.effect {
        Effect::Functions { functions } if functions.is_empty() => {
            writeln!(out, "  no function changed: only comments and whitespace")
        }
        Effect::Functions { functions } => {
            for function in functions {
                writeln!(
                    out,
                    "  {} (base lines {}-{}): {}/{} lines and {}/{} outcomes covered, reached by {} test(s)",
                    function.key,
                    function.first,
                    function.last,
                    function.lines.covered,
                    function.lines.total,
                    function.outcomes.covered,
                    function.outcomes.total,
                    function.tests.len()
                )?;
                for test in &function.tests {
                    writeln!(out, "    {test}")?;
                }
            }
            Ok(())
        }
        Effect::WholeFile { tests } => writeln!(
            out,
            "  deleted or renamed: every test that ran it, {} test(s)",
            tests.len()
        ),
        Effect::TestClasses { classes } => {
            writeln!(out, "  test classes: {}", classes.join(", "))
        }
        Effect::NewFile { classes } if classes.is_empty() => {
            writeln!(
                out,
                "  new file without classes: reachable only from changed code"
            )
        }
        Effect::NewFile { classes } => writeln!(out, "  new file: {}", classes.join(", ")),
        Effect::NeverLoaded => writeln!(out, "  no baseline test loaded it"),
        Effect::Ignored => writeln!(out, "  ignored"),
        Effect::Unbounded { reason } => writeln!(out, "  run every test: {reason}"),
    };
}

pub(crate) fn render_text(out: &mut dyn Write, result: &ImpactResult<'_>) -> io::Result<()> {
    for change in &result.changes {
        render_change(out, change)?;
    }
    if result.verdict == VerdictName::Unbounded {
        let unbounded = result
            .changes
            .iter()
            .filter(|change| return matches!(change.effect, Effect::Unbounded { .. }))
            .count();
        return writeln!(
            out,
            "impact: run every test, {unbounded} change(s) cannot be bounded by the baseline"
        );
    }
    return writeln!(
        out,
        "impact: run {} test(s) and {} test class(es)",
        result.tests.len(),
        result.classes.len()
    );
}

pub(crate) fn render_json(out: &mut dyn Write, result: &ImpactResult<'_>) -> io::Result<()> {
    serde_json::to_writer(&mut *out, result)?;
    return writeln!(out);
}

fn quote(name: &str, out: &mut String) {
    for character in name.chars() {
        if "\\^$.|?*+()[]{}/#-".contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
}

pub(crate) enum Filter {
    Expression(String),
    Empty,
    Unnamed(Vec<String>),
}

pub(crate) fn filter(result: &ImpactResult<'_>) -> Filter {
    let unnamed: Vec<String> = result
        .tests
        .iter()
        .filter(|name| return !name.contains("::"))
        .map(|name| return (*name).to_owned())
        .collect();
    if !unnamed.is_empty() {
        return Filter::Unnamed(unnamed);
    }
    let mut methods: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for name in &result.tests {
        let Some((class, rest)) = name.split_once("::") else {
            continue;
        };
        let method = rest.split(['#', ' ']).next().unwrap_or(rest);
        methods.entry(class).or_default().insert(method);
    }
    let mut alternatives = Vec::with_capacity(methods.len().saturating_add(result.classes.len()));
    for class in &result.classes {
        let mut alternative = String::new();
        quote(class, &mut alternative);
        alternative.push_str("::");
        alternatives.push(alternative);
    }
    for (class, names) in methods {
        let mut alternative = String::new();
        quote(class, &mut alternative);
        alternative.push_str("::(?:");
        let mut first = true;
        for method in names {
            if !first {
                alternative.push('|');
            }
            first = false;
            quote(method, &mut alternative);
        }
        alternative.push_str(")(?: |$)");
        alternatives.push(alternative);
    }
    if alternatives.is_empty() {
        return Filter::Empty;
    }
    return Filter::Expression(format!("/^(?:{})/i", alternatives.join("|")));
}
