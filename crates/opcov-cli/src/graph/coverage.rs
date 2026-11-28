use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use opcov_core::report::{FileCoverage, Header, Sha256, SourcePath};
use opcov_php::{CoverageTarget, Graph, NodeCoverage, Overlaid, Ratio};
use opcov_syntax::SourceFile;

use crate::changes::resolve;
use crate::failure::Failure;
use crate::gate::{Unmeasurable, selection, unmeasurable};
use crate::impact::{Coverage, function_coverage, stale_failure};
use crate::{source, stream};

const NO_BODY: &str = "it has no body to execute";

struct Base {
    targets: Vec<CoverageTarget>,
    sha: Sha256,
    file: SourceFile,
}

enum Read {
    Measured(SourcePath, Vec<(usize, NodeCoverage)>),
    Stale(String),
}

const fn ratio(coverage: Coverage) -> Ratio {
    return Ratio {
        covered: coverage.covered,
        total: coverage.total,
    };
}

fn base_bytes(
    root: &Path,
    overlay: &BTreeMap<PathBuf, Overlaid>,
    path: &str,
) -> Result<Vec<u8>, Failure> {
    return match overlay.get(Path::new(path)) {
        Some(Overlaid::Contents(contents)) => Ok(contents.clone()),
        Some(Overlaid::Absent) => Err(Failure::failed(format!(
            "cannot measure {path}: the graph places a base member in it, but the diff creates it"
        ))),
        None => {
            let disk = root.join(path);
            fs::read(&disk).map_err(|error| return Failure::io("read", &disk, &error))
        }
    };
}

fn measured(
    header: &Header,
    file: &FileCoverage,
    base: &SourceFile,
    function: &str,
) -> NodeCoverage {
    let functions = base.functions();
    let found = functions
        .iter()
        .position(|source| return source.key().to_string().eq_ignore_ascii_case(function))
        .and_then(|index| return function_coverage(header, file, functions, index));
    return found.map_or_else(
        || {
            return NodeCoverage::Unmeasured {
                reason: NO_BODY.to_owned(),
            };
        },
        |impact| {
            return NodeCoverage::Measured {
                lines: ratio(impact.lines),
                outcomes: ratio(impact.outcomes),
                tests: impact
                    .tests
                    .iter()
                    .map(|test| return (*test).to_owned())
                    .collect(),
            };
        },
    );
}

struct Planned {
    covered: Vec<(usize, NodeCoverage)>,
    bases: BTreeMap<SourcePath, Base>,
}

fn plan(
    header: &Header,
    root: &Path,
    overlay: &BTreeMap<PathBuf, Overlaid>,
    graph: &Graph,
) -> Result<Planned, Failure> {
    let mut grouped: BTreeMap<String, Vec<CoverageTarget>> = BTreeMap::new();
    for target in graph.coverage_targets() {
        grouped.entry(target.path.clone()).or_default().push(target);
    }
    let mut planned = Planned {
        covered: Vec::new(),
        bases: BTreeMap::new(),
    };
    for (path, targets) in grouped {
        let Some(source) = resolve(&header.paths, root, Path::new(&path)) else {
            let reason = Unmeasurable::OutsideRoot.to_string();
            planned.covered.extend(targets.iter().map(|target| {
                return (
                    target.node,
                    NodeCoverage::Unmeasured {
                        reason: reason.clone(),
                    },
                );
            }));
            continue;
        };
        let bytes = base_bytes(root, overlay, &path)?;
        let key_file =
            source::key_file(&source).map_err(|error| return Failure::failed(error.to_string()))?;
        let file = SourceFile::parse(key_file, &bytes)
            .map_err(|error| return Failure::failed(error.to_string()))?;
        planned.bases.insert(
            source,
            Base {
                targets,
                sha: Sha256::digest(&bytes),
                file,
            },
        );
    }
    return Ok(planned);
}

fn read(
    report: &Path,
    header: &Header,
    bases: &BTreeMap<SourcePath, Base>,
    covered: &mut Vec<(usize, NodeCoverage)>,
) -> Result<BTreeSet<SourcePath>, Failure> {
    let mut stale = Vec::new();
    let mut found = BTreeSet::new();
    stream::entries(
        report,
        header,
        &|path| return bases.contains_key(path),
        &|_, path, file| {
            let Some(base) = bases.get(&path) else {
                return Ok(None);
            };
            if base.sha != file.sha256 {
                return Ok(Some(Read::Stale(format!(
                    "{path}: the diff's base has sha256 {}, the baseline records {}",
                    base.sha, file.sha256
                ))));
            }
            let nodes = base
                .targets
                .iter()
                .map(|target| {
                    return (
                        target.node,
                        measured(header, &file, &base.file, &target.function),
                    );
                })
                .collect();
            return Ok(Some(Read::Measured(path, nodes)));
        },
        &mut |read| {
            match read {
                Some(Read::Measured(path, nodes)) => {
                    found.insert(path);
                    covered.extend(nodes);
                }
                Some(Read::Stale(message)) => stale.push(message),
                None => {}
            }
            return Ok(());
        },
    )?;
    if !stale.is_empty() {
        return Err(stale_failure(report, &stale));
    }
    return Ok(found);
}

pub(crate) fn measure(
    report: &Path,
    root: &Path,
    overlay: &BTreeMap<PathBuf, Overlaid>,
    graph: &Graph,
) -> Result<Vec<(usize, NodeCoverage)>, Failure> {
    let header = stream::header(report)?;
    let Planned { mut covered, bases } = plan(&header, root, overlay, graph)?;
    let found = read(report, &header, &bases, &mut covered)?;
    let unloaded: Vec<(&SourcePath, &Base)> = bases
        .iter()
        .filter(|(path, _)| return !found.contains(*path))
        .collect();
    if unloaded.is_empty() {
        return Ok(covered);
    }
    let chosen = selection(&header.settings, root)?;
    for (path, base) in unloaded {
        let disk = header.paths.disk_path(path);
        let unread = unmeasurable(&header.settings, &chosen, &disk).map_or(
            NodeCoverage::NeverLoaded,
            |reason| {
                return NodeCoverage::Unmeasured {
                    reason: reason.to_string(),
                };
            },
        );
        covered.extend(
            base.targets
                .iter()
                .map(|target| return (target.node, unread.clone())),
        );
    }
    return Ok(covered);
}
