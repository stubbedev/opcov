use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use crate::failure::Failure;

const DIRECTORY: &str = ".opcov";
const PRIVATE: u32 = 0o700;

pub(crate) fn path(root: &Path, name: &str) -> PathBuf {
    return root.join(DIRECTORY).join(name);
}

pub(crate) fn create_parent(path: &Path) -> Result<(), Failure> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    return fs::DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE)
        .create(parent)
        .map_err(|error| return Failure::io("create", parent, &error));
}
