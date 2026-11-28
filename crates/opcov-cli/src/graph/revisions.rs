use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{panic, thread};

use opcov_php::Overlaid;

use crate::failure::Failure;

const PHP_PATHS: &str = "*.php";
const SETTINGS: [&str; 6] = [
    "-c",
    "core.quotePath=false",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
];

pub(crate) struct Revisions {
    pub(crate) source: String,
    pub(crate) diff: Vec<u8>,
    pub(crate) head: BTreeMap<PathBuf, Overlaid>,
    pub(crate) base_commit: String,
    pub(crate) head_commit: String,
}

fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, Failure> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(SETTINGS)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            return Failure::failed(format!("cannot run git {}: {error}", arguments.join(" ")));
        })?;
    if !output.status.success() {
        return Err(Failure::failed(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    return Ok(output.stdout);
}

fn commit(root: &Path, revision: &str) -> Result<String, Failure> {
    let named = format!("{revision}^{{commit}}");
    let output = git(root, &["rev-parse", "--verify", "--quiet", &named])
        .map_err(|error| return Failure::failed(format!("{revision} names no commit: {error}")))?;
    return Ok(String::from_utf8_lossy(&output).trim().to_owned());
}

fn paths(output: &[u8]) -> impl Iterator<Item = PathBuf> + '_ {
    return output
        .split(|byte| return *byte == 0)
        .filter(|path| return !path.is_empty())
        .map(|path| return PathBuf::from(String::from_utf8_lossy(path).into_owned()));
}

fn contents(root: &Path, revision: &str, wanted: &[PathBuf]) -> Result<Vec<Vec<u8>>, Failure> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| return Failure::failed(format!("cannot run git cat-file: {error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| return Failure::failed("git cat-file has no input".to_owned()))?;
    let requests: Vec<String> = wanted
        .iter()
        .map(|path| return format!("{revision}:./{}\n", path.display()))
        .collect();
    let writer = thread::spawn(move || {
        for request in requests {
            stdin.write_all(request.as_bytes())?;
        }
        return Ok::<(), std::io::Error>(());
    });
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| return Failure::failed("git cat-file has no output".to_owned()))?;
    let mut reader = BufReader::new(stdout);
    let mut found = Vec::with_capacity(wanted.len());
    for path in wanted {
        let mut header = String::new();
        reader.read_line(&mut header).map_err(|error| {
            return Failure::failed(format!("cannot read git cat-file: {error}"));
        })?;
        let size = header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|size| return size.parse::<usize>().ok())
            .filter(|_| return !header.trim_end().ends_with("missing"))
            .ok_or_else(|| {
                return Failure::failed(format!(
                    "git has no {} at {revision}: {}",
                    path.display(),
                    header.trim_end()
                ));
            })?;
        let mut content = vec![0; size];
        reader
            .read_exact(&mut content)
            .and_then(|()| return reader.read_exact(&mut [0; 1]))
            .map_err(|error| {
                return Failure::failed(format!("cannot read git cat-file: {error}"));
            })?;
        found.push(content);
    }
    let written = match writer.join() {
        Ok(written) => written,
        Err(payload) => panic::resume_unwind(payload),
    };
    written.map_err(|error| {
        return Failure::failed(format!("cannot write to git cat-file: {error}"));
    })?;
    let status = child
        .wait()
        .map_err(|error| return Failure::failed(format!("git cat-file did not finish: {error}")))?;
    if !status.success() {
        return Err(Failure::failed(format!(
            "git cat-file failed with {status}"
        )));
    }
    return Ok(found);
}

fn head_overlay(root: &Path, head: &str) -> Result<BTreeMap<PathBuf, Overlaid>, Failure> {
    let differing = git(
        root,
        &[
            "diff",
            "--name-status",
            "--no-renames",
            "--relative",
            "-z",
            head,
            "--",
            PHP_PATHS,
        ],
    )?;
    let mut fields = differing
        .split(|byte| return *byte == 0)
        .filter(|field| return !field.is_empty());
    let mut from_head = Vec::new();
    let mut overlay = BTreeMap::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let path = PathBuf::from(String::from_utf8_lossy(path).into_owned());
        if status == b"A" {
            overlay.insert(path, Overlaid::Absent);
        } else {
            from_head.push(path);
        }
    }
    let untracked = git(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            PHP_PATHS,
        ],
    )?;
    for path in paths(&untracked) {
        overlay.insert(path, Overlaid::Absent);
    }
    for (path, content) in from_head.iter().zip(contents(root, head, &from_head)?) {
        overlay.insert(path.clone(), Overlaid::Contents(content));
    }
    return Ok(overlay);
}

pub(crate) fn read(root: &Path, base: &str, head: Option<&str>) -> Result<Revisions, Failure> {
    let head_commit = head.map(|head| return commit(root, head)).transpose()?;
    let tip = match &head_commit {
        Some(head) => head.clone(),
        None => commit(root, "HEAD")?,
    };
    let base_commit = commit(root, base)?;
    let merge_base = String::from_utf8_lossy(&git(root, &["merge-base", &base_commit, &tip])?)
        .trim()
        .to_owned();
    let mut arguments = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--relative",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        merge_base.as_str(),
    ];
    if let Some(head) = head_commit.as_deref() {
        arguments.push(head);
    }
    let diff = git(root, &arguments)?;
    let head_overlay = match head_commit.as_deref() {
        Some(head) => head_overlay(root, head)?,
        None => BTreeMap::new(),
    };
    let shown_head = head.unwrap_or("the working tree");
    return Ok(Revisions {
        source: format!("the diff from {base} to {shown_head}"),
        diff,
        head: head_overlay,
        base_commit: merge_base,
        head_commit: tip,
    });
}
