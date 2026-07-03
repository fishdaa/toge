//! Index persistence: save/load binary format.

use crate::index::Index;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStats {
    pub entry_count: u32,
    pub bytes_written: u64,
}

impl Index {
    pub fn save(&self, path: &Path) -> io::Result<SaveStats> {
        let _ = path;
        todo!()
    }

    pub fn load(path: &Path) -> io::Result<Index> {
        let _ = path;
        todo!()
    }
}

#[cfg(test)]
mod tests;
