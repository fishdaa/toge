//! Filesystem walking and exclusion logic.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::index::Index;

/// Simple exclusion rules used while walking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excludes {
    pub skip_hidden: bool,
    pub skip_system_paths: bool,
    pub patterns: Vec<String>,
    pub folders: Vec<String>,
    pub paths: Vec<PathBuf>,
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

        if self.paths.iter().any(|root| path.starts_with(root)) {
            return true;
        }

        if self.skip_hidden
            && path
                .file_name()
                .map(|n| n.as_encoded_bytes().starts_with(b"."))
                .unwrap_or(false)
        {
            return true;
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();

        if !self.include_only.is_empty() && !self.include_only.iter().any(|p| glob_match(&name, p))
        {
            return true;
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

pub fn has_hidden_ancestor_dir(path: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        parent.components().any(|component| {
            let name = component.as_os_str().as_encoded_bytes();
            name.len() > 1 && name.starts_with(b".")
        })
    })
}

pub fn is_hidden_dir_path(path: &Path, is_dir: bool) -> bool {
    has_hidden_ancestor_dir(path)
        || (is_dir
            && path
                .file_name()
                .map(|n| {
                    let name = n.as_encoded_bytes();
                    name.len() > 1 && name.starts_with(b".")
                })
                .unwrap_or(false))
}

/// Walk a directory tree and insert entries into the index.
/// When `fetch_metadata` is false, size/timestamps are set to 0 to avoid a `stat` syscall per file.
pub fn walk(root: &Path, index: &mut Index, excludes: &Excludes, fetch_metadata: bool) -> usize {
    visit(root, excludes, fetch_metadata, |path, is_dir, metadata| {
        let (size, modified, created, accessed) = metadata;
        index.insert_with_metadata(path, is_dir, size, modified, created, accessed);
    })
}

/// Reconcile a persisted index with the current contents of the configured roots.
///
/// Existing entries are updated in place, new entries are inserted, and entries that
/// are no longer present (or are now excluded) are removed.
pub fn reconcile(
    roots: &[PathBuf],
    index: &mut Index,
    excludes: &Excludes,
    fetch_metadata: bool,
) -> usize {
    let mut seen = HashSet::with_capacity(index.count());
    let mut count = 0;

    for root in roots {
        count += visit(root, excludes, fetch_metadata, |path, is_dir, metadata| {
            seen.insert(path.to_string());
            let (size, modified, created, accessed) = metadata;
            index.insert_with_metadata(path, is_dir, size, modified, created, accessed);
        });
    }

    let stale: Vec<String> = index
        .entries
        .iter()
        .filter(|entry| !seen.contains(&entry.path))
        .map(|entry| entry.path.clone())
        .collect();
    for path in stale {
        index.remove(&path);
    }

    count
}

fn visit(
    root: &Path,
    excludes: &Excludes,
    fetch_metadata: bool,
    mut on_entry: impl FnMut(&str, bool, (u64, i64, i64, i64)),
) -> usize {
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
                Ok(ft) => {
                    if ft.is_symlink() {
                        continue;
                    }
                    ft.is_dir()
                }
                Err(_) => match fs::symlink_metadata(&path) {
                    Ok(md) => {
                        if md.file_type().is_symlink() {
                            continue;
                        }
                        md.is_dir()
                    }
                    Err(_) => path.is_dir(),
                },
            };

            if has_hidden_ancestor_dir(&path) || is_hidden_dir_path(&path, is_dir) {
                continue;
            }

            if excludes.is_excluded(&path) {
                continue;
            }

            let metadata = if fetch_metadata {
                let metadata = fs::symlink_metadata(&path).ok();
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let created = metadata
                    .as_ref()
                    .and_then(|m| m.created().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let accessed = metadata
                    .as_ref()
                    .and_then(|m| m.accessed().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                (size, modified, created, accessed)
            } else {
                (0, 0, 0, 0)
            };
            on_entry(path.to_str().unwrap_or(""), is_dir, metadata);
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
        if let Some(s) = component.as_os_str().to_str()
            && glob_match(s, normalized)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
