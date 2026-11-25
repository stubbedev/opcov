use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use opcov_core::filter::eval_host;
use opcov_core::report::{Header, Sha256, SourcePath, stitch_streams};

use crate::changes::Patch;
use crate::failure::{Failure, Verdict};
use crate::impact::{self, ImpactResult, Scope};
use crate::output::write_atomically;
use crate::stream;

const SHOWN: usize = 10;

pub(crate) struct Inputs<'a> {
    pub(crate) baseline: &'a Path,
    pub(crate) partial: &'a Path,
    pub(crate) output: &'a Path,
    pub(crate) base: &'a Path,
    pub(crate) ignore: &'a [PathBuf],
}

fn listed(lines: &[String]) -> String {
    let shown: Vec<&str> = lines.iter().take(SHOWN).map(String::as_str).collect();
    let more = lines.len().saturating_sub(shown.len());
    let mut listing = format!("\n  {}", shown.join("\n  "));
    if more > 0 {
        listing = format!("{listing}\n  and {more} more");
    }
    return listing;
}

fn bounded(result: &ImpactResult<'_>) -> Result<(), Failure> {
    let unbounded: Vec<String> = result.unbounded().collect();
    if unbounded.is_empty() {
        return Ok(());
    }
    return Err(Failure::refused(format!(
        "cannot stitch: {} change(s) reach tests the baseline cannot name, so only a run of every test measures the branch:{}",
        unbounded.len(),
        listed(&unbounded)
    )));
}

fn rerun_all(result: &ImpactResult<'_>, partial: &Path, header: &Header) -> Result<(), Failure> {
    let ran = header.tests.names();
    let missing: Vec<String> = result
        .tests()
        .iter()
        .filter(|test| {
            return ran
                .binary_search_by(|name| return name.as_str().cmp(test))
                .is_err();
        })
        .map(|test| return (*test).to_owned())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    return Err(Failure::refused(format!(
        "cannot stitch: {} test(s) that reach the change did not run in {}; run `opcov impact --whole-files --format filter` for the tests to run:{}",
        missing.len(),
        partial.display(),
        listed(&missing)
    )));
}

fn built_from_disk(partial: &Path, header: &Header) -> Result<(), Failure> {
    let mut stale = Vec::new();
    stream::entries(
        partial,
        header,
        &stream::source_files,
        &|_, path, file| {
            let disk = header.paths.disk_path(&path);
            return match fs::read(&disk) {
                Ok(bytes) if Sha256::digest(&bytes) == file.sha256 => Ok(None),
                Ok(bytes) => Ok(Some(format!(
                    "{path}: the file has sha256 {}, the partial report records {}",
                    Sha256::digest(&bytes),
                    file.sha256
                ))),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Ok(Some(format!("{path}: the file does not exist")))
                }
                Err(error) => Err(Failure::io("read", &disk, &error)),
            };
        },
        &mut |found| {
            stale.extend(found);
            return Ok(());
        },
    )?;
    if stale.is_empty() {
        return Ok(());
    }
    return Err(Failure::failed(format!(
        "the partial report {} was not built from the working tree:{}",
        partial.display(),
        listed(&stale)
    )));
}

fn changed(touched: &BTreeSet<&str>, path: &SourcePath) -> bool {
    return touched.contains(eval_host(path.as_str()).unwrap_or(path.as_str()));
}

fn open(path: &Path) -> Result<BufReader<File>, Failure> {
    return File::open(path)
        .map(BufReader::new)
        .map_err(|error| return Failure::io("open", path, &error));
}

pub(crate) fn run(inputs: &Inputs<'_>, patch: &Patch) -> Result<Verdict, Failure> {
    let header = stream::header(inputs.baseline)?;
    let result = impact::evaluate(
        inputs.baseline,
        &header,
        patch,
        inputs.base,
        inputs.ignore,
        Scope::Files,
    )?;
    bounded(&result)?;
    let partial = stream::header(inputs.partial)?;
    rerun_all(&result, inputs.partial, &partial)?;
    built_from_disk(inputs.partial, &partial)?;
    let names = [inputs.baseline.display(), inputs.partial.display()];
    let touched: BTreeSet<&str> = result.touched().iter().map(SourcePath::as_str).collect();
    write_atomically(inputs.output, |out| {
        return stitch_streams(
            open(inputs.baseline)?,
            open(inputs.partial)?,
            |test| return result.selects(test),
            |path| return changed(&touched, path),
            out,
        )
        .map_err(|error| return Failure::merge(&error, &names));
    })?;
    return Ok(Verdict::Clean);
}
