#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is the intended visibility; the pub this lint suggests is denied by rustc unreachable_pub"
)]

use std::fs;
use std::path::{Path, PathBuf};

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(crate) fn project_copy(name: &str) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    copy(&fixture, &root);
    return root;
}
