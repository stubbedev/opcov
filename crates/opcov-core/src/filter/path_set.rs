use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::CaseSensitivity;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Node {
    entry: Option<PathBuf>,
    children: BTreeMap<Box<[u8]>, Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathSet {
    case: CaseSensitivity,
    root: Node,
}

impl PathSet {
    pub(super) fn new(case: CaseSensitivity) -> Self {
        return Self {
            case,
            root: Node::default(),
        };
    }

    pub(super) const fn is_empty(&self) -> bool {
        return self.root.children.is_empty();
    }

    pub(super) fn insert(&mut self, path: PathBuf) {
        let mut scratch = Vec::new();
        let mut node = &mut self.root;
        for component in path.components() {
            let key = fold(self.case, component.as_os_str().as_encoded_bytes(), &mut scratch);
            node = node.children.entry(key.into()).or_default();
        }
        if node.entry.is_none() {
            node.entry = Some(path);
        }
    }

    pub(super) fn first_containing(&self, file: &Path) -> Option<&Path> {
        let mut scratch = Vec::new();
        let mut node = &self.root;
        for component in file.components() {
            let key = fold(self.case, component.as_os_str().as_encoded_bytes(), &mut scratch);
            node = node.children.get(key)?;
            if let Some(entry) = &node.entry {
                return Some(entry);
            }
        }
        return None;
    }
}

fn fold<'a>(case: CaseSensitivity, bytes: &'a [u8], scratch: &'a mut Vec<u8>) -> &'a [u8] {
    return match case {
        CaseSensitivity::Sensitive => bytes,
        CaseSensitivity::Insensitive => {
            scratch.clear();
            scratch.extend(bytes.iter().map(u8::to_ascii_lowercase));
            scratch.as_slice()
        }
    };
}
