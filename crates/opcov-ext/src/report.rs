use core::{
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};
use std::{
    collections::BTreeMap,
    ffi::CStr,
    fmt, fs,
    io::{self, Write},
    panic,
    path::Path,
    thread,
};

use opcov_core::{
    cfg::{FilePath, FunctionKey, KeyError, Line, LoopSite, LoopTest, OpIndex},
    output::{AtomicWriteError, WriteFailure},
    report::{
        FileCoverage, Function, Generator, Header, HitSet, IdSorted, Loop, LoopId, MeasuredShape,
        ModelError, Outcome, OutcomeKind, PathBase, ReportWriter, Sha256, SourcePath, TestIndex,
        TestTable, VmKind, WriteError, line_hits,
    },
};
use opcov_sys::{ZTS, opcov_php_version, zend_vm_kind};

use crate::{
    VERSION,
    engine::{
        AttributedTest, Content, Covered, Engine, FileId, FileRecord, Files, Fingerprint,
        FunctionId, FunctionRecord, Source,
    },
};

#[derive(Debug)]
pub(crate) enum ReportError {
    Model(ModelError),
    Key { path: SourcePath, error: KeyError },
    SourceChanged { path: String },
    SourceUnreadable { path: String, reason: String },
    SourceModified { path: String },
    SourceUnknown { path: String },
    VmKind(i32),
    PhpVersion,
    NotConfigured,
    LoopWithoutTest { function: FunctionKey, id: LoopId },
    Io(io::Error),
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Model(error) => write!(f, "{error}"),
            Self::Key { path, error } => {
                write!(f, "{path} cannot name the functions of its entry: {error}")
            }
            Self::SourceChanged { path } => write!(
                f,
                "{path} was compiled from different sources in this process, so no single sha256 describes its coverage"
            ),
            Self::SourceUnreadable { path, reason } => {
                write!(f, "the source of {path} could not be hashed: {reason}")
            }
            Self::SourceModified { path } => write!(
                f,
                "{path} changed on disk after PHP compiled it, so the sha256 of the compiled source cannot be computed; keep measured files unchanged until the report is written"
            ),
            Self::SourceUnknown { path } => {
                write!(f, "opcov recorded no source for the measured file {path}")
            }
            Self::VmKind(raw) => write!(f, "the Zend VM kind {raw} is unknown to opcov"),
            Self::PhpVersion => f.write_str("the PHP version is not valid UTF-8"),
            Self::NotConfigured => f.write_str("no file selection is configured for this request"),
            Self::LoopWithoutTest { function, id } => write!(
                f,
                "loop {id} of `{function}` has no planned test outcome to anchor its site"
            ),
            Self::Io(error) => write!(f, "{error}"),
        };
    }
}

impl From<ModelError> for ReportError {
    fn from(error: ModelError) -> Self {
        return Self::Model(error);
    }
}

pub(crate) struct Entry {
    pub(crate) lines: BTreeMap<Line, HitSet>,
    pub(crate) functions: BTreeMap<FunctionKey, Function>,
}

fn hit_set(mut tests: Vec<TestIndex>) -> Result<HitSet, ModelError> {
    tests.sort_unstable();
    tests.dedup();
    return HitSet::new(tests);
}

#[derive(Default)]
struct FunctionTests {
    outcomes: BTreeMap<u32, Vec<TestIndex>>,
    zero: BTreeMap<u32, Vec<TestIndex>>,
    second: BTreeMap<u32, Vec<TestIndex>>,
}

fn loop_site(record: &FunctionRecord, planned: &LoopSite) -> Result<OpIndex, ReportError> {
    let (LoopTest::Foreach { back, .. }
    | LoopTest::PreTest { back, .. }
    | LoopTest::PostTest { back, .. }) = &planned.test;
    return record
        .plan
        .outcome(*back)
        .map(|outcome| return outcome.site)
        .ok_or_else(|| {
            return ReportError::LoopWithoutTest {
                function: record.key.clone(),
                id: planned.id,
            };
        });
}

fn function_model(
    record: &FunctionRecord,
    mut tests: FunctionTests,
) -> Result<Function, ReportError> {
    let outcomes = record
        .plan
        .outcomes()
        .iter()
        .map(|outcome| {
            return Outcome::new(
                outcome.id,
                outcome.site,
                OutcomeKind::new(outcome.kind)?,
                outcome.line,
                None,
                hit_set(tests.outcomes.remove(&outcome.id.get()).unwrap_or_default())?,
            );
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    let loops = record
        .plan
        .loops()
        .iter()
        .zip(
            record
                .zero_possible
                .iter()
                .copied()
                .chain(core::iter::repeat(true)),
        )
        .map(|(planned, zero_possible)| {
            let number = planned.id.get();
            let zero = hit_set(tests.zero.remove(&number).unwrap_or_default())?;
            return Ok(Loop::new(
                planned.id,
                loop_site(record, planned)?,
                MeasuredShape::new(planned.test.shape())?,
                planned.line,
                zero_possible.then_some(zero),
                hit_set(tests.second.remove(&number).unwrap_or_default())?,
            ));
        })
        .collect::<Result<Vec<_>, ReportError>>()?;
    return Ok(Function {
        span: None,
        outcomes: IdSorted::new(outcomes)?,
        loops: IdSorted::new(loops)?,
    });
}

pub(crate) fn entry(
    files: &Files,
    record: &FileRecord,
    key_file: &FilePath,
    hits: &[(Covered, TestIndex)],
) -> Result<Entry, ReportError> {
    let mut by_function: BTreeMap<FunctionId, FunctionTests> = BTreeMap::new();
    let mut lines = Vec::new();
    for &(covered, test) in hits {
        match covered {
            Covered::Line(_, line) => lines.push((line, test)),
            Covered::Outcome(function, number) => by_function
                .entry(function)
                .or_default()
                .outcomes
                .entry(number)
                .or_default()
                .push(test),
            Covered::Zero(function, number) => by_function
                .entry(function)
                .or_default()
                .zero
                .entry(number)
                .or_default()
                .push(test),
            Covered::Second(function, number) => by_function
                .entry(function)
                .or_default()
                .second
                .entry(number)
                .or_default()
                .push(test),
        }
    }
    let mut functions = BTreeMap::new();
    for (key, &id) in &record.functions {
        let Some(function) = files.function(id) else {
            continue;
        };
        let tests = by_function.remove(&id).unwrap_or_default();
        functions.insert(key.in_file(key_file), function_model(function, tests)?);
    }
    return Ok(Entry {
        lines: line_hits(record.lines.iter().copied(), lines)?,
        functions,
    });
}

fn vm_kind() -> Result<VmKind, ReportError> {
    // SAFETY: zend_vm_kind only returns a compile-time constant of the running engine.
    let raw = unsafe { zend_vm_kind() };
    let kind = opcov_sys::VmKind::try_from(raw)
        .map_err(|unknown| return ReportError::VmKind(unknown.0))?;
    return match kind.name().to_bytes() {
        b"CALL" => Ok(VmKind::Call),
        b"SWITCH" => Ok(VmKind::Switch),
        b"GOTO" => Ok(VmKind::Goto),
        b"HYBRID" => Ok(VmKind::Hybrid),
        b"TAILCALL" => Ok(VmKind::Tailcall),
        _ => Err(ReportError::VmKind(raw)),
    };
}

fn generator() -> Result<Generator, ReportError> {
    // SAFETY: php_version returns a static NUL-terminated string.
    let php = unsafe { CStr::from_ptr(opcov_php_version()) }
        .to_str()
        .map_err(|_utf8| return ReportError::PhpVersion)?;
    return Ok(Generator {
        opcov: VERSION.to_string_lossy().into_owned(),
        php: php.to_owned(),
        zts: ZTS,
        vm: vm_kind()?,
    });
}

fn compiled(record: &FileRecord) -> Result<Option<Content>, ReportError> {
    let path = || return record.path.clone();
    return match &record.source {
        Some(Source::Compiled(content)) => Ok(Some(*content)),
        Some(Source::Changed) => Err(ReportError::SourceChanged { path: path() }),
        Some(Source::Unreadable(reason)) => Err(ReportError::SourceUnreadable {
            path: path(),
            reason: reason.clone(),
        }),
        None => Ok(None),
    };
}

fn digest_file(path: &str, compiled: Fingerprint) -> Result<Sha256, ReportError> {
    let content = fs::read(path).map_err(|error| {
        return ReportError::SourceUnreadable {
            path: path.to_owned(),
            reason: format!("cannot read it to write the report: {error}"),
        };
    })?;
    if Fingerprint::of(&content) != compiled {
        return Err(ReportError::SourceModified {
            path: path.to_owned(),
        });
    }
    return Ok(Sha256::digest(&content));
}

fn digest(planned: &Planned<'_>) -> Result<Sha256, ReportError> {
    return match planned.content {
        Content::Text(sha256) => Ok(sha256),
        Content::File(fingerprint) => digest_file(&planned.record.path, fingerprint),
    };
}

fn digests(planned: &[Planned<'_>]) -> Result<Vec<Sha256>, ReportError> {
    let workers = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(planned.len());
    let next = AtomicUsize::new(0);
    let mut done = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                return scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = planned.get(index) else {
                            return done;
                        };
                        done.push((index, digest(file)));
                    }
                });
            })
            .collect();
        return handles
            .into_iter()
            .flat_map(|handle| {
                return handle
                    .join()
                    .unwrap_or_else(|payload| panic::resume_unwind(payload));
            })
            .collect::<Vec<_>>();
    });
    done.sort_unstable_by_key(|&(index, _)| return index);
    return done.into_iter().map(|(_, digest)| return digest).collect();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    File(FileId),
    Function(FunctionId),
}

const fn owner(covered: Covered) -> Owner {
    return match covered {
        Covered::Line(file, _) => Owner::File(file),
        Covered::Outcome(function, _)
        | Covered::Zero(function, _)
        | Covered::Second(function, _) => Owner::Function(function),
    };
}

const BATCH_HITS: usize = 1 << 24;

struct Attributed<'e> {
    files: &'e Files,
    ids: Vec<FileId>,
    tests: &'e [AttributedTest],
    indexes: Vec<TestIndex>,
}

impl Attributed<'_> {
    fn slot(&self, owner: Owner) -> Option<usize> {
        let file = match owner {
            Owner::File(file) => file,
            Owner::Function(function) => self.files.function(function)?.file,
        };
        return self.ids.binary_search(&file).ok();
    }

    fn runs(&self, mut visit: impl FnMut(usize, &[Covered], TestIndex)) {
        for (test, &index) in self.tests.iter().zip(&self.indexes) {
            for run in test
                .covered
                .chunk_by(|left, right| return owner(*left) == owner(*right))
            {
                let Some(&first) = run.first() else {
                    continue;
                };
                if let Some(slot) = self.slot(owner(first)) {
                    visit(slot, run, index);
                }
            }
        }
    }

    fn counts(&self) -> Vec<usize> {
        let mut counts = vec![0_usize; self.ids.len()];
        self.runs(|slot, run, _| {
            if let Some(count) = counts.get_mut(slot) {
                *count = count.saturating_add(run.len());
            }
        });
        return counts;
    }

    fn gather(&self, batch: &[Planned<'_>], counts: &[usize]) -> Vec<Vec<(Covered, TestIndex)>> {
        let mut position = vec![None; self.ids.len()];
        let mut buckets = Vec::with_capacity(batch.len());
        for (at, planned) in batch.iter().enumerate() {
            if let Some(marker) = position.get_mut(planned.slot) {
                *marker = Some(at);
            }
            buckets.push(Vec::with_capacity(
                counts.get(planned.slot).copied().unwrap_or_default(),
            ));
        }
        self.runs(|slot, run, test| {
            let bucket = position
                .get(slot)
                .copied()
                .flatten()
                .and_then(|at| return buckets.get_mut(at));
            if let Some(bucket) = bucket {
                bucket.extend(run.iter().map(|&covered| return (covered, test)));
            }
        });
        return buckets;
    }
}

struct Planned<'e> {
    path: SourcePath,
    slot: usize,
    record: &'e FileRecord,
    key_file: FilePath,
    content: Content,
}

fn batch_lengths(sizes: impl IntoIterator<Item = usize>, budget: usize) -> Vec<usize> {
    let mut lengths = Vec::new();
    let mut length = 0_usize;
    let mut total = 0_usize;
    for size in sizes {
        if length > 0 && total.saturating_add(size) > budget {
            lengths.push(length);
            length = 0;
            total = 0;
        }
        length = length.saturating_add(1);
        total = total.saturating_add(size);
    }
    if length > 0 {
        lengths.push(length);
    }
    return lengths;
}

impl From<WriteError> for ReportError {
    fn from(error: WriteError) -> Self {
        return match error {
            WriteError::Model(error) => Self::Model(*error),
            WriteError::Io(error) => Self::Io(error),
        };
    }
}

pub(crate) fn write_failure(target: &Path, error: &AtomicWriteError<ReportError>) -> String {
    let action = match &error.failure {
        WriteFailure::Content(content) if !matches!(content, ReportError::Io(_)) => {
            "cannot build the report for"
        }
        WriteFailure::Content(_) | WriteFailure::NoFileName { .. } | WriteFailure::Io(_) => {
            "cannot write the report to"
        }
    };
    return format!("{action} {}: {error}", target.display());
}

impl Engine {
    fn plan<'e>(
        &'e self,
        paths: &PathBase,
        counts: &[usize],
    ) -> Result<Vec<Planned<'e>>, ReportError> {
        let mut planned = Vec::new();
        for (slot, (_, record)) in self.files.iter().enumerate() {
            let Some(content) = compiled(record)? else {
                let hit = counts.get(slot).is_some_and(|&count| return count > 0);
                if record.lines.is_empty() && !hit {
                    continue;
                }
                return Err(ReportError::SourceUnknown {
                    path: record.path.clone(),
                });
            };
            let path = paths.locate(&record.path)?;
            let key_file: FilePath = path.as_str().parse().map_err(|error| {
                return ReportError::Key {
                    path: path.clone(),
                    error,
                };
            })?;
            planned.push(Planned {
                path,
                slot,
                record,
                key_file,
                content,
            });
        }
        planned.sort_unstable_by(|left, right| return left.path.cmp(&right.path));
        return Ok(planned);
    }

    pub(crate) fn write_report<W: Write>(&self, paths: PathBase, out: W) -> Result<W, ReportError> {
        let settings = self
            .request
            .as_ref()
            .and_then(|request| return request.settings.clone())
            .ok_or(ReportError::NotConfigured)?;
        let tests = self.attribution.tests();
        let names: Vec<String> = tests.iter().map(|test| return test.name.clone()).collect();
        let (table, indexes) = TestTable::from_unordered(&names)?;
        drop(names);
        let attributed = Attributed {
            files: &self.files,
            ids: self.files.iter().map(|(id, _)| return id).collect(),
            tests,
            indexes,
        };
        let counts = attributed.counts();
        let planned = self.plan(&paths, &counts)?;
        let sha256s = digests(&planned)?;
        let header = Header {
            generator: generator()?,
            settings,
            paths,
            tests: table,
        };
        let mut writer = ReportWriter::begin(out, &header).map_err(ReportError::Io)?;
        let sizes = planned
            .iter()
            .map(|file| return counts.get(file.slot).copied().unwrap_or_default());
        let mut rest = planned.as_slice();
        let mut sha256s = sha256s.into_iter();
        for length in batch_lengths(sizes, BATCH_HITS) {
            let (batch, remaining) = rest.split_at(length.min(rest.len()));
            rest = remaining;
            let buckets = attributed.gather(batch, &counts);
            for ((file, hits), sha256) in batch.iter().zip(buckets).zip(sha256s.by_ref()) {
                let Entry { lines, functions } =
                    entry(&self.files, file.record, &file.key_file, &hits)?;
                drop(hits);
                writer.entry(
                    &file.path,
                    &FileCoverage {
                        sha256,
                        lines,
                        functions,
                    },
                )?;
            }
        }
        return writer.finish().map_err(ReportError::Io);
    }
}

#[cfg(test)]
mod tests {
    use super::batch_lengths;

    #[test]
    fn batches_stay_within_the_budget_and_keep_every_file() {
        assert_eq!(
            batch_lengths([3, 3, 3, 3], 6),
            [2, 2],
            "two files fit each batch"
        );
        assert_eq!(
            batch_lengths([1, 2, 3], 6),
            [3],
            "everything fits one batch"
        );
        assert_eq!(batch_lengths([], 6), [0_usize; 0], "no files, no batches");
    }

    #[test]
    fn a_file_larger_than_the_budget_gets_a_batch_of_its_own() {
        assert_eq!(
            batch_lengths([1, 10, 1], 4),
            [1, 1, 1],
            "the large file stands alone"
        );
        assert_eq!(
            batch_lengths([0, 0, 9], 4),
            [2, 1],
            "empty files join the batch before"
        );
    }
}
