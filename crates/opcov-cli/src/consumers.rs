use std::io::{self, Write};
use std::path::Path;

use opcov_php::{Index, Project, Request, Target, Use};
use serde::Serialize;

use crate::failure::Failure;

#[derive(Debug, Serialize)]
pub(crate) struct Found {
    uses: Vec<Use>,
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
    let index = Index::build(&project);
    return Ok(Found {
        uses: index.uses(request),
    });
}

pub(crate) fn render_text(out: &mut dyn Write, found: &Found) -> io::Result<()> {
    for found in &found.uses {
        writeln!(out, "{}:{}\t{}", found.path, found.line, found.target)?;
    }
    return writeln!(out, "consumers: {} use(s)", found.uses.len());
}

pub(crate) fn render_json(out: &mut dyn Write, found: &Found) -> io::Result<()> {
    serde_json::to_writer(&mut *out, found)?;
    return writeln!(out);
}
