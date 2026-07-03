//! Configuration loading and representation.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub roots: Vec<std::path::PathBuf>,
    pub exclude_fstypes: Vec<String>,
    pub exclude_hidden: bool,
    pub exclude_patterns: Vec<String>,
    pub exclude_folders: Vec<String>,
    pub include_only: Vec<String>,
    pub index_size: bool,
    pub index_date_modified: bool,
    pub index_date_created: bool,
    pub index_date_accessed: bool,
    pub index_permissions: bool,
    pub fast_sort_extension: bool,
    pub fast_sort_path: bool,
    pub whole_filename_wildcards: bool,
    pub operator_precedence: OperatorOrder,
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorOrder {
    OrAnd,
    AndOr,
}

impl Config {
    pub fn default_config() -> Self {
        todo!()
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let _ = path;
        todo!()
    }
}

#[cfg(test)]
mod tests;
