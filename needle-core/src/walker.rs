//! Filesystem walking and exclusion logic.

use std::path::Path;

use crate::index::Index;

/// Simple exclusion rules used while walking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excludes {
    pub skip_hidden: bool,
    pub skip_system_paths: bool,
    pub patterns: Vec<String>,
    pub folders: Vec<String>,
    pub include_only: Vec<String>,
}

impl Excludes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        let _ = path;
        todo!()
    }
}

/// Walk a directory tree and insert entries into the index.
pub fn walk(root: &Path, index: &mut Index, excludes: &Excludes) -> usize {
    let _ = root;
    let _ = index;
    let _ = excludes;
    todo!()
}

#[cfg(test)]
mod tests;
