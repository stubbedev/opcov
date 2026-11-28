use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::{panic, thread};

use opcov_php::{FileChange, Graph, Index, Links, Overlaid, Project, Revision, Routes};
use serde::{Deserialize, Serialize};

mod coverage;
mod revisions;

pub(crate) use revisions::read as revisions;

use crate::changes::{
    ChangeSet, Content, FileChange as Lines, FilePatch, Patch, parse_diff, rebuild_base,
};
use crate::consumers;
use crate::failure::Failure;
use crate::gate::PHP_EXTENSION;
use crate::index;

fn is_php(path: &Path) -> bool {
    return path
        .extension()
        .is_some_and(|extension| return extension == PHP_EXTENSION);
}

fn current(
    root: &Path,
    head: &BTreeMap<PathBuf, Overlaid>,
    path: &Path,
) -> Result<Vec<u8>, Failure> {
    return match head.get(path) {
        Some(Overlaid::Contents(contents)) => Ok(contents.clone()),
        Some(Overlaid::Absent) => Err(Failure::failed(format!(
            "cannot rebuild the base version of {}: the diff changes it, but the head has no such file",
            path.display()
        ))),
        None => {
            let disk = root.join(path);
            fs::read(&disk).map_err(|error| return Failure::io("read", &disk, &error))
        }
    };
}

fn overlay(
    root: &Path,
    patch: &Patch,
    head: &BTreeMap<PathBuf, Overlaid>,
) -> Result<BTreeMap<PathBuf, Overlaid>, Failure> {
    let mut overlay = head.clone();
    for file in patch.files() {
        let (old, new) = (file.old.as_deref(), file.new.as_deref());
        if !old.into_iter().chain(new).any(is_php) {
            continue;
        }
        let Content::Text(hunks) = &file.content else {
            let shown = new.or(old).unwrap_or_else(|| return Path::new(""));
            return Err(Failure::failed(format!(
                "cannot rebuild the base version of {}: the diff shows a binary change",
                shown.display()
            )));
        };
        if let Some(new) = new.filter(|new| return is_php(new) && Some(*new) != old) {
            overlay.insert(new.to_path_buf(), Overlaid::Absent);
        }
        let Some(old) = old.filter(|old| return is_php(old)) else {
            continue;
        };
        let current = match new {
            Some(new) => current(root, head, new)?,
            None => Vec::new(),
        };
        let base = rebuild_base(&old.display(), &current, hunks)?;
        overlay.insert(old.to_path_buf(), Overlaid::Contents(base));
    }
    return Ok(overlay);
}

fn failed(error: &impl ToString) -> Failure {
    return Failure::failed(error.to_string());
}

fn side_changes(
    mut set: ChangeSet,
    patch: &Patch,
    side: fn(&FilePatch) -> Option<&PathBuf>,
) -> Vec<FileChange> {
    for file in patch.files() {
        if let (Content::Text(_), Some(path)) = (&file.content, side(file)) {
            set.add(path.clone(), Lines::Parts(BTreeSet::new()));
        }
    }
    return consumers::changes(&set);
}

fn routes(path: Option<&Path>, project: &Project) -> Result<Routes, Failure> {
    let Some(path) = path else {
        return Ok(Routes::default());
    };
    let bytes = fs::read(path).map_err(|error| return Failure::io("read", path, &error))?;
    return Routes::parse(&bytes)
        .map(|routes| return routes.located(project))
        .map_err(|error| {
            return Failure::failed(format!(
                "cannot read the routes {}: {error}",
                path.display()
            ));
        });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Format {
    Json,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Request {
    pub(crate) source: String,
    pub(crate) diff: Vec<u8>,
    pub(crate) strip: usize,
    pub(crate) within: Vec<String>,
    pub(crate) depth: u32,
    pub(crate) tests: bool,
    pub(crate) routes: Option<PathBuf>,
    pub(crate) report: Option<PathBuf>,
    pub(crate) format: Format,
    pub(crate) links: Option<Links>,
}

impl Request {
    pub(crate) fn changed_paths(&self) -> Vec<PathBuf> {
        return self.patch().map_or_else(
            |_| return Vec::new(),
            |patch| {
                return patch
                    .files()
                    .iter()
                    .filter_map(|file| return file.new.clone())
                    .collect();
            },
        );
    }

    fn patch(&self) -> Result<Patch, Failure> {
        return parse_diff(&self.source, &self.diff, self.strip)
            .map_err(|error| return Failure::failed(error.to_string()));
    }
}

fn open(root: &Path, request: &Request) -> Result<Project, Failure> {
    let opened = Project::open(root).map_err(|error| return failed(&error))?;
    if request.tests {
        return Ok(opened.with_development());
    }
    return Ok(opened);
}

fn describe(
    project: &Project,
    head_index: &Index,
    mut base_index: Index,
    patch: &Patch,
    overlay: &BTreeMap<PathBuf, Overlaid>,
    request: &Request,
) -> Result<String, Failure> {
    let root = project.root();
    let routes = routes(request.routes.as_deref(), project)?;
    let head_changes = side_changes(ChangeSet::of_patch(patch), patch, |file| {
        return file.new.as_ref();
    });
    let base_changes = side_changes(ChangeSet::of_base_patch(patch), patch, |file| {
        return file.old.as_ref();
    });
    let diff: Vec<&str> = base_changes
        .iter()
        .chain(&head_changes)
        .map(|change| return change.path.as_str())
        .collect();
    base_index.share_unaffected(head_index, &diff);
    let walked = Graph::walk(
        Revision {
            index: &base_index,
            changes: &base_changes,
        },
        Revision {
            index: head_index,
            changes: &head_changes,
        },
        &routes,
        &request.within,
        request.depth,
    )
    .map_err(|error| return failed(&error));
    thread::spawn(move || return drop(base_index));
    let mut graph = walked?;
    if let Some(report) = &request.report {
        let covered = coverage::measure(report, root, overlay, &graph)?;
        graph.cover(covered);
    }
    let mut rendered = Vec::new();
    match request.format {
        Format::Json => render_json(&mut rendered, &graph),
        Format::Markdown => graph.render_markdown(&mut rendered, request.links.as_ref()),
    }
    .map_err(|error| return Failure::failed(format!("cannot render the graph: {error}")))?;
    return String::from_utf8(rendered)
        .map_err(|error| return Failure::failed(format!("cannot render the graph: {error}")));
}

pub(crate) fn build(
    root: &Path,
    request: &Request,
    head: &BTreeMap<PathBuf, Overlaid>,
) -> Result<String, Failure> {
    let patch = request.patch()?;
    let project = open(root, request)?.overlaid(head.clone());
    let overlay = overlay(root, &patch, head)?;
    let base_project = project.clone().overlaid(overlay.clone());
    let persisted = !request.tests && head.is_empty();
    let (head_index, base_index) = thread::scope(|scope| {
        let base = scope.spawn(|| return index::build_unpersisted(&base_project));
        let head = if persisted {
            index::build(&project)
        } else {
            index::build_unpersisted(&project)
        };
        let base = match base.join() {
            Ok(built) => built,
            Err(payload) => panic::resume_unwind(payload),
        };
        return (head, base);
    });
    let (head_index, base_index) = (head_index?, base_index?);
    let described = describe(&project, &head_index, base_index, &patch, &overlay, request);
    thread::spawn(move || return drop(head_index));
    return described;
}

pub(crate) fn build_on(
    project: &Project,
    head_index: &Index,
    request: &Request,
) -> Result<String, Failure> {
    let patch = request.patch()?;
    let root = project.root();
    let overlay = overlay(root, &patch, &BTreeMap::new())?;
    let base_index = index::build_unpersisted(&project.clone().overlaid(overlay.clone()))?;
    return describe(project, head_index, base_index, &patch, &overlay, request);
}

fn render_json(out: &mut dyn Write, graph: &Graph) -> io::Result<()> {
    serde_json::to_writer(&mut *out, graph)?;
    return writeln!(out);
}
