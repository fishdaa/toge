//! Filesystem walking and exclusion logic.

use std::fs;
use std::path::{Path, PathBuf};

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
        if self.skip_system_paths {
            let s = path.as_os_str().as_encoded_bytes();
            if s == b"/proc" || s == b"/sys" || s == b"/dev" {
                return true;
            }
        }

        if self.skip_hidden {
            if path
                .file_name()
                .map(|n| n.as_encoded_bytes().starts_with(b"."))
                .unwrap_or(false)
            {
                return true;
            }
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();

        if !self.include_only.is_empty() {
            if !self.include_only.iter().any(|p| glob_match(&name, p)) {
                return true;
            }
        }

        if self.patterns.iter().any(|p| glob_match(&name, p)) {
            return true;
        }

        if self.folders.iter().any(|p| folder_matches(path, p)) {
            return true;
        }

        false
    }
}

/// Walk a directory tree and insert entries into the index.
pub fn walk(root: &Path, index: &mut Index, excludes: &Excludes) -> usize {
    let mut count = 0;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if excludes.is_excluded(&dir) && dir != root {
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = match entry.file_type() {
                Ok(ft) => ft.is_dir(),
                Err(_) => path.is_dir(),
            };

            if excludes.is_excluded(&path) {
                continue;
            }

            index.insert(path.to_str().unwrap_or(""), is_dir);
            count += 1;

            if is_dir {
                stack.push(path);
            }
        }
    }

    count
}

/// Very small glob matcher supporting `*` and `?`.
fn glob_match(name: &str, pattern: &str) -> bool {
    let mut chars = name.chars().peekable();
    let mut pat = pattern.chars().peekable();

    while let Some(p) = pat.next() {
        match p {
            '*' => {
                while pat.peek() == Some(&'*') {
                    pat.next();
                }
                let next = pat.peek().copied();
                if next.is_none() {
                    return true;
                }
                while let Some(c) = chars.peek().copied() {
                    if Some(c) == next {
                        let text_rest: String = chars.clone().collect();
                        let pat_rest: String = pat.clone().collect();
                        if glob_match(&text_rest, &pat_rest) {
                            return true;
                        }
                    }
                    chars.next();
                }
                return false;
            }
            '?' => {
                if chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    chars.next().is_none()
}

/// Check if a path matches a folder exclude pattern like `**/node_modules`.
fn folder_matches(path: &Path, pattern: &str) -> bool {
    let normalized = pattern.strip_prefix("**/").unwrap_or(pattern);
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if glob_match(s, normalized) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests;
