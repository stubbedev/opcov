use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use opcov_core::report::Sha256;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::changes::{Content, FilePatch, Patch, hunks};
use crate::failure::Failure;
use crate::output::write_atomically;

const MANIFEST: &str = "manifest.json";
const BLOBS: &str = "blobs";
const SUFFIX: &str = ".snapshot";
const CHUNK: usize = 1 << 16;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    report: Sha256,
    files: BTreeMap<String, Sha256>,
}

#[derive(Debug)]
pub(crate) struct Snapshot {
    directory: PathBuf,
    report: Sha256,
    files: BTreeMap<String, Sha256>,
}

pub(crate) fn report_digest(report: &Path) -> Result<Sha256, Failure> {
    let mut file =
        fs::File::open(report).map_err(|error| return Failure::io("open", report, &error))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| return Failure::io("read", report, &error))?;
        let Some(chunk) = buffer.get(..read).filter(|chunk| return !chunk.is_empty()) else {
            break;
        };
        hasher.update(chunk);
    }
    return Ok(Sha256::from_bytes(hasher.finalize().into()));
}

pub(crate) fn directory(report: &Path) -> PathBuf {
    let mut name = report.file_name().unwrap_or_default().to_os_string();
    name.push(SUFFIX);
    return report.with_file_name(name);
}

fn relative(root: &Path, path: &Path) -> Result<String, Failure> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    return relative
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| return Failure::failed(format!("{} is not a UTF-8 path", path.display())));
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, Failure> {
    return match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Failure::io("read", path, &error)),
    };
}

pub(crate) fn load(report: &Path) -> Result<Option<Snapshot>, Failure> {
    let directory = directory(report);
    let path = directory.join(MANIFEST);
    let Some(bytes) = read(&path)? else {
        return Ok(None);
    };
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| return Failure::failed(format!("{}: {error}", path.display())))?;
    return Ok(Some(Snapshot {
        directory,
        report: manifest.report,
        files: manifest.files,
    }));
}

pub(crate) fn take(report: &Path, root: &Path, files: &[PathBuf]) -> Result<(), Failure> {
    let directory = directory(report);
    let blobs = directory.join(BLOBS);
    fs::create_dir_all(&blobs).map_err(|error| return Failure::io("create", &blobs, &error))?;
    let mut manifest = Manifest {
        report: report_digest(report)?,
        files: BTreeMap::new(),
    };
    for path in files {
        let bytes = fs::read(path).map_err(|error| return Failure::io("read", path, &error))?;
        let sha = Sha256::digest(&bytes);
        let blob = blobs.join(sha.to_string());
        if !blob.is_file() {
            write_atomically(&blob, |mut out| {
                out.write_all(&bytes)
                    .map_err(|error| return Failure::io("write", &blob, &error))?;
                return Ok(out);
            })?;
        }
        manifest.files.insert(relative(root, path)?, sha);
    }
    let target = directory.join(MANIFEST);
    write_atomically(&target, |mut out| {
        serde_json::to_writer(&mut out, &manifest).map_err(|error| {
            return Failure::failed(format!("cannot write {}: {error}", target.display()));
        })?;
        return Ok(out);
    })?;
    let kept: BTreeSet<String> = manifest.files.values().map(ToString::to_string).collect();
    let entries =
        fs::read_dir(&blobs).map_err(|error| return Failure::io("list", &blobs, &error))?;
    for entry in entries {
        let entry = entry.map_err(|error| return Failure::io("list", &blobs, &error))?;
        if !kept.contains(entry.file_name().to_string_lossy().as_ref()) {
            fs::remove_file(entry.path())
                .map_err(|error| return Failure::io("remove", &entry.path(), &error))?;
        }
    }
    return Ok(());
}

impl Snapshot {
    pub(crate) const fn report(&self) -> Sha256 {
        return self.report;
    }

    fn blob(&self, sha: &Sha256) -> Result<Option<Vec<u8>>, Failure> {
        return read(&self.directory.join(BLOBS).join(sha.to_string()));
    }

    fn content(old: Option<&[u8]>, new: &[u8]) -> Content {
        return old
            .and_then(|old| return hunks(old, new))
            .map_or(Content::Binary, Content::Text);
    }

    pub(crate) fn patch(&self, root: &Path, files: &[PathBuf]) -> Result<Patch, Failure> {
        let mut current: BTreeMap<String, &Path> = BTreeMap::new();
        for path in files {
            current.insert(relative(root, path)?, path);
        }
        let paths: BTreeSet<&String> = current.keys().chain(self.files.keys()).collect();
        let mut changed = Vec::new();
        for path in paths {
            let recorded = self.files.get(path);
            let now = match current.get(path) {
                Some(disk) => read(disk)?,
                None => None,
            };
            let file = match (recorded, now) {
                (Some(sha), Some(bytes)) if Sha256::digest(&bytes) == *sha => continue,
                (Some(sha), Some(bytes)) => FilePatch {
                    old: Some(PathBuf::from(path)),
                    new: Some(PathBuf::from(path)),
                    content: Self::content(self.blob(sha)?.as_deref(), &bytes),
                },
                (Some(sha), None) => FilePatch {
                    old: Some(PathBuf::from(path)),
                    new: None,
                    content: Self::content(self.blob(sha)?.as_deref(), b""),
                },
                (None, Some(bytes)) => FilePatch {
                    old: None,
                    new: Some(PathBuf::from(path)),
                    content: Self::content(Some(b""), &bytes),
                },
                (None, None) => continue,
            };
            changed.push(file);
        }
        return Ok(Patch::new(changed));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{load, take};
    use crate::changes::{Content, original};

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn a_patch_against_the_snapshot_rebuilds_every_old_file() {
        let root = std::env::temp_dir().join(format!("opcov-snapshot-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let report = root.join("build/opcov.json");
        write(&report, "{}");
        let (kept, edited, deleted, added) = (
            root.join("src/Kept.php"),
            root.join("src/Edited.php"),
            root.join("src/Deleted.php"),
            root.join("src/Added.php"),
        );
        write(&kept, "<?php\nkept\n");
        write(&edited, "<?php\none\ntwo\n");
        write(&deleted, "<?php\ngone\n");
        take(
            &report,
            &root,
            &[kept.clone(), edited.clone(), deleted.clone()],
        )
        .unwrap();
        write(&edited, "<?php\none\n2\ntwo\n");
        fs::remove_file(&deleted).unwrap();
        write(&added, "<?php\nnew\n");
        let snapshot = load(&report).unwrap().unwrap();
        let patch = snapshot
            .patch(&root, &[kept, edited.clone(), added])
            .unwrap();
        let sides: Vec<(Option<PathBuf>, Option<PathBuf>)> = patch
            .files()
            .iter()
            .map(|file| return (file.old.clone(), file.new.clone()))
            .collect();
        assert_eq!(
            sides,
            [
                (None, Some(PathBuf::from("src/Added.php"))),
                (Some(PathBuf::from("src/Deleted.php")), None),
                (
                    Some(PathBuf::from("src/Edited.php")),
                    Some(PathBuf::from("src/Edited.php"))
                ),
            ]
        );
        let Content::Text(hunks) = &patch.files()[2].content else {
            panic!("an edited text file gives hunks");
        };
        assert_eq!(
            original(&fs::read(&edited).unwrap(), hunks).unwrap(),
            b"<?php\none\ntwo\n"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
