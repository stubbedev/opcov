use std::cmp::Ordering;
use std::io::{Read, Write};
use std::thread;

use super::combine::{
    EntrySource, HeaderMismatch, InputId, MergeError, Remap, StaleEntry, first_difference,
    union_files,
};
use super::decode::ReportReader;
use super::encode::{ReportWriter, WriteError};
use super::error::ModelError;
use super::model::{FileCoverage, Header, SourcePath, TestIndex, TestTable};
use super::streaming::{StreamMergeError, StreamSource};

const BASELINE: InputId = InputId::new(0);
const PARTIAL: InputId = InputId::new(1);

struct StitchPlan {
    header: Header,
    baseline: Remap,
    partial: Remap,
}

fn next_index(names: &[String]) -> Result<TestIndex, MergeError> {
    return u32::try_from(names.len())
        .map(TestIndex::new)
        .map_err(|_overflow| {
            return MergeError::MergedTests(Box::new(ModelError::TooManyTests {
                count: names.len(),
            }));
        });
}

fn plan(
    baseline: &Header,
    partial: &Header,
    replaced: &impl Fn(&str) -> bool,
) -> Result<StitchPlan, MergeError> {
    if let Some((field, left, right)) = first_difference(baseline, partial) {
        return Err(MergeError::Header(Box::new(HeaderMismatch {
            field,
            first: BASELINE,
            second: PARTIAL,
            left,
            right,
        })));
    }
    let (old, new) = (baseline.tests.names(), partial.tests.names());
    let mut names: Vec<String> = Vec::with_capacity(old.len().saturating_add(new.len()));
    let mut kept = Vec::with_capacity(old.len());
    let mut rerun = Vec::with_capacity(new.len());
    let mut old = old.iter().peekable();
    let mut new = new.iter().peekable();
    loop {
        let order = match (old.peek(), new.peek()) {
            (None, None) => break,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(left), Some(right)) => left.cmp(right),
        };
        if order != Ordering::Greater
            && let Some(name) = old.next()
        {
            if order == Ordering::Equal || replaced(name) {
                kept.push(None);
            } else {
                kept.push(Some(next_index(&names)?));
                names.push(name.clone());
            }
        }
        if order != Ordering::Less
            && let Some(name) = new.next()
        {
            rerun.push(Some(next_index(&names)?));
            names.push(name.clone());
        }
    }
    return Ok(StitchPlan {
        header: Header {
            generator: baseline.generator.clone(),
            settings: baseline.settings.clone(),
            paths: baseline.paths.clone(),
            tests: TestTable(names),
        },
        baseline: Remap(kept),
        partial: Remap(rerun),
    });
}

fn stitch_entry(
    tests: &TestTable,
    path: &SourcePath,
    old: Option<FileCoverage>,
    new: Option<FileCoverage>,
    changed: bool,
) -> Result<Option<FileCoverage>, MergeError> {
    if !changed {
        return match (old, new) {
            (Some(old), Some(new)) => union_files(old, new)
                .map(Some)
                .map_err(|clash| return clash.conflict(path.clone(), BASELINE, PARTIAL)),
            (old, new) => Ok(old.or(new)),
        };
    }
    let stale = old.as_ref().and_then(|file| {
        return file
            .hit_sets()
            .find_map(|hits| return hits.as_slice().first().copied());
    });
    if let Some(index) = stale {
        return Err(MergeError::Stale(Box::new(StaleEntry {
            path: path.clone(),
            test: tests.name(index).unwrap_or_default().to_owned(),
        })));
    }
    return Ok(new);
}

fn take_at(
    source: &mut StreamSource,
    path: &SourcePath,
) -> Result<Option<FileCoverage>, StreamMergeError> {
    if source.head() != Some(path) {
        return Ok(None);
    }
    return source.take();
}

pub fn stitch_streams<R, W>(
    baseline: R,
    partial: R,
    replaced: impl Fn(&str) -> bool,
    changed: impl Fn(&SourcePath) -> bool,
    out: W,
) -> Result<W, StreamMergeError>
where
    R: Read + Send,
    W: Write,
{
    let open = |input: InputId, reader: R| {
        return ReportReader::open(reader)
            .map_err(|error| return StreamMergeError::Read { input, error });
    };
    let (baseline, partial) = (open(BASELINE, baseline)?, open(PARTIAL, partial)?);
    let plan = plan(baseline.header(), partial.header(), &replaced)?;
    let mut writer = ReportWriter::begin(out, &plan.header)
        .map_err(|error| return StreamMergeError::Write(WriteError::Io(error)))?;
    let paths = &plan.header.paths;
    thread::scope(|scope| {
        let mut old = StreamSource::start(scope, BASELINE, baseline, paths, &plan.baseline)?;
        let mut new = StreamSource::start(scope, PARTIAL, partial, paths, &plan.partial)?;
        loop {
            let Some(path) = [old.head(), new.head()]
                .into_iter()
                .flatten()
                .min()
                .cloned()
            else {
                return Ok::<(), StreamMergeError>(());
            };
            let (before, after) = (take_at(&mut old, &path)?, take_at(&mut new, &path)?);
            let stitched = stitch_entry(&plan.header.tests, &path, before, after, changed(&path))?;
            if let Some(file) = stitched {
                writer
                    .entry(&path, &file)
                    .map_err(StreamMergeError::Write)?;
            }
        }
    })?;
    return writer
        .finish()
        .map_err(|error| return StreamMergeError::Write(WriteError::Io(error)));
}
