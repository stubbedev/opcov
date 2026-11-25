use std::io::{self, Write};
use std::path::Path;

use opcov_php::{Index, IndexStats, Project, Request, Search, Target};
use serde::Serialize;

use crate::failure::Failure;

#[derive(Debug, Serialize)]
pub(crate) struct Found {
    #[serde(flatten)]
    search: Search,
    index: IndexStats,
}

pub(crate) fn request(
    json: Option<&[u8]>,
    targets: &[String],
    within: &[String],
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
    if request.targets.is_empty() {
        return Err(Failure::failed(
            "no targets: pass --target or a --request with targets".to_owned(),
        ));
    }
    return Ok(request);
}

pub(crate) fn find(root: &Path, request: &Request) -> Result<Found, Failure> {
    let project = Project::open(root).map_err(|error| return Failure::failed(error.to_string()))?;
    let index =
        Index::build(&project).map_err(|error| return Failure::failed(error.to_string()))?;
    return Ok(Found {
        search: index.uses(request),
        index: index.stats(),
    });
}

pub(crate) fn render_text(out: &mut dyn Write, found: &Found) -> io::Result<()> {
    for found in &found.search.uses {
        writeln!(out, "{}:{}\t{}", found.path, found.line, found.target)?;
    }
    return writeln!(
        out,
        "consumers: {} use(s) in {} of {} project file(s) that mention a target; {} file(s) indexed, {} model(s), {} facade(s)",
        found.search.uses.len(),
        found.search.analysed,
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
