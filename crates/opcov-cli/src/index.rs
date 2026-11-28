use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use opcov_php::{Build, CacheError, Index, Project, ScanCache};

use crate::failure::Failure;
use crate::local;
use crate::output::{write_atomically, write_stderr};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const CACHE_NAME: &str = "index.cache";

fn build_identity() -> Result<Vec<u8>, Failure> {
    let executable = std::env::current_exe().map_err(|error| {
        return Failure::io("locate", Path::new("the opcov executable"), &error);
    })?;
    let metadata = fs::metadata(&executable)
        .map_err(|error| return Failure::io("inspect", &executable, &error))?;
    let modified = metadata
        .modified()
        .map_err(|error| return Failure::io("inspect", &executable, &error))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            return Failure::failed(format!(
                "cannot inspect {}: its modification time {error}",
                executable.display()
            ));
        })?
        .as_nanos();
    return Ok(format!("{VERSION}\n{}\n{modified}", metadata.len()).into_bytes());
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, Failure> {
    return match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Failure::io("read", path, &error)),
    };
}

fn previous(path: &Path, build: &[u8], bytes: Option<&[u8]>) -> Result<ScanCache, Failure> {
    let Some(bytes) = bytes else {
        return Ok(ScanCache::new(build));
    };
    return match ScanCache::decode(build, bytes) {
        Ok(cache) => Ok(cache),
        Err(CacheError::Stale) => Ok(ScanCache::new(build)),
        Err(error) => {
            write_stderr(|out| {
                return writeln!(
                    out,
                    "opcov: rebuilding the index cache {}, because {error}",
                    path.display()
                );
            })?;
            Ok(ScanCache::new(build))
        }
    };
}

fn build_cached(project: &Project) -> Result<(PathBuf, Build), Failure> {
    let path = local::path(project.root(), CACHE_NAME);
    let build = build_identity()?;
    let read = read(&path)?;
    let scanned = Index::build_cached(project, previous(&path, &build, read.as_deref())?)
        .map_err(|error| return Failure::failed(error.to_string()))?;
    return Ok((path, scanned));
}

pub(crate) fn build(project: &Project) -> Result<Index, Failure> {
    let (
        path,
        Build {
            index,
            cache,
            changed,
            ..
        },
    ) = build_cached(project)?;
    if !changed {
        return Ok(index);
    }
    let bytes = cache
        .encode()
        .map_err(|error| return Failure::failed(error.to_string()))?;
    local::create_parent(&path)?;
    write_atomically(&path, |mut out| {
        out.write_all(&bytes)
            .map_err(|error| return Failure::io("write", &path, &error))?;
        return Ok(out);
    })?;
    return Ok(index);
}

pub(crate) fn build_unpersisted(project: &Project) -> Result<Index, Failure> {
    return Ok(build_cached(project)?.1.index);
}
