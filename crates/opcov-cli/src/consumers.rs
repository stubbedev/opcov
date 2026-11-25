use std::io::{self, Write};
use std::path::Path;

use opcov_php::{Changed, FileChange, Index, IndexStats, Project, Request, Target, Tree};
use serde::Serialize;

use crate::changes::{Change, ChangeSet, FileChange as Lines};
use crate::failure::Failure;

#[derive(Debug, Serialize)]
pub(crate) struct Found {
    #[serde(flatten)]
    tree: Tree,
    index: IndexStats,
}

pub(crate) fn changes(set: &ChangeSet) -> Vec<FileChange> {
    return set
        .files()
        .map(|(path, lines)| {
            let changed = match lines {
                Lines::Whole => Changed::Whole,
                Lines::Parts(parts) => {
                    let mut ranges = Vec::new();
                    let mut deletions = Vec::new();
                    for part in parts {
                        match part {
                            Change::Lines(range) => {
                                ranges.push((range.first().get(), range.last().get()));
                            }
                            Change::Deletion { before } => deletions.push(before.get()),
                        }
                    }
                    Changed::Parts { ranges, deletions }
                }
            };
            return FileChange {
                path: path.to_string_lossy().into_owned(),
                changed,
            };
        })
        .collect();
}

pub(crate) fn request(
    json: Option<&[u8]>,
    targets: &[String],
    within: &[String],
    has_diff: bool,
    source: &str,
) -> Result<Request, Failure> {
    let mut request = match json {
        Some(bytes) => serde_json::from_slice::<Request>(bytes)
            .map_err(|error| return Failure::failed(format!("{source}: {error}")))?,
        None => Request::default(),
    };
    for target in targets {
        request.targets.push(
            target
                .parse::<Target>()
                .map_err(|error| return Failure::failed(error.to_string()))?,
        );
    }
    request.within.extend(within.iter().cloned());
    if request.targets.is_empty() && !has_diff {
        return Err(Failure::failed(
            "no targets: pass --diff, --target or a --request with targets".to_owned(),
        ));
    }
    return Ok(request);
}

pub(crate) fn find(
    root: &Path,
    request: &Request,
    changes: &[FileChange],
    depth: u32,
) -> Result<Found, Failure> {
    let project = Project::open(root).map_err(|error| return Failure::failed(error.to_string()))?;
    let index =
        Index::build(&project).map_err(|error| return Failure::failed(error.to_string()))?;
    let members = index.changed_members(changes);
    return Ok(Found {
        tree: index.consumers(members, &request.targets, &request.within, depth),
        index: index.stats(),
    });
}

pub(crate) fn render_text(out: &mut dyn Write, found: &Found) -> io::Result<()> {
    if !found.tree.changed.is_empty() {
        writeln!(out, "changed members:")?;
        for member in &found.tree.changed {
            writeln!(
                out,
                "  {}:{}-{}\t{}",
                member.path, member.first, member.last, member.target
            )?;
        }
    }
    if !found.tree.consumers.is_empty() {
        writeln!(out, "consumers:")?;
        for consumer in &found.tree.consumers {
            let member = consumer
                .member
                .as_deref()
                .unwrap_or("(outside any function)");
            let uses: Vec<&str> = consumer.uses.iter().map(String::as_str).collect();
            writeln!(
                out,
                "  {}:{}-{}\t{member}\t[depth {}] uses {}",
                consumer.path,
                consumer.first,
                consumer.last,
                consumer.depth,
                uses.join(", ")
            )?;
        }
    }
    return writeln!(
        out,
        "consumers: {} changed member(s), {} consumer(s); analysed {} of {} project file(s); {} file(s) indexed, {} model(s), {} facade(s)",
        found.tree.changed.len(),
        found.tree.consumers.len(),
        found.tree.analysed,
        found.index.sources,
        found.index.scanned,
        found.index.models,
        found.index.facades
    );
}

pub(crate) fn render_json(out: &mut dyn Write, found: &Found) -> io::Result<()> {
    serde_json::to_writer(&mut *out, found)?;
    return writeln!(out);
}

pub(crate) fn render_files(out: &mut dyn Write, found: &Found) -> io::Result<()> {
    for consumer in &found.tree.consumers {
        writeln!(
            out,
            "{}:{}-{}",
            consumer.path, consumer.first, consumer.last
        )?;
    }
    return Ok(());
}
