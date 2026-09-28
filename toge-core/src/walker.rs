//! Filesystem walking and exclusion logic.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::index::{Index, fnv1a_64};

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
                .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."))
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
            && path.file_name().is_some_and(|n| {
                let name = n.as_encoded_bytes();
                name.len() > 1 && name.starts_with(b".")
            }))
}

/// Walk a directory tree and insert entries into the index.
/// When `fetch_metadata` is false, size/timestamps are set to 0 to avoid a `stat` syscall per file.
pub fn walk(root: &Path, index: &mut Index, excludes: &Excludes, fetch_metadata: bool) -> usize {
    visit(root, excludes, fetch_metadata, |path, is_dir, metadata| {
        let (size, modified, created, accessed) = metadata;
        index.insert_with_metadata(path, is_dir, size, modified, created, accessed);
        true
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
    reconcile_live(roots, excludes, fetch_metadata, |step| {
        step(index);
        true
    })
}

/// Walked entries applied per [`reconcile_live`] step.
const RECONCILE_BATCH: usize = 4096;
/// Stale entries removed per [`reconcile_live`] step.
const RECONCILE_REMOVE_BATCH: usize = 64;

/// A walked entry: path, whether it is a directory, and (size, modified, created, accessed).
type Walked = (String, bool, (u64, i64, i64, i64));

/// [`reconcile`] an index that stays in use while the roots are walked.
///
/// The walk runs outside `with_index`, which applies each batch of entries (e.g.
/// under the index's lock) and returns false once the index has been replaced,
/// ending the reconcile. Other writers such as a watcher may change the index
/// between batches, so an entry the walk did not see is removed only if it is
/// gone from disk or would no longer be indexed.
pub fn reconcile_live(
    roots: &[PathBuf],
    excludes: &Excludes,
    fetch_metadata: bool,
    mut with_index: impl FnMut(&mut dyn FnMut(&mut Index)) -> bool,
) -> usize {
    // Path hashes rather than IDs: other writers may renumber entries between batches.
    let mut seen: HashSet<u64> = HashSet::new();
    let mut batch: Vec<Walked> = Vec::new();
    let mut live = true;
    let mut apply = |batch: &mut Vec<Walked>| {
        let ok = with_index(&mut |index| {
            for (path, is_dir, (size, modified, created, accessed)) in batch.drain(..) {
                index.insert_with_metadata(&path, is_dir, size, modified, created, accessed);
            }
        });
        batch.clear();
        ok
    };

    let mut count = 0;
    for root in roots {
        count += visit(root, excludes, fetch_metadata, |path, is_dir, metadata| {
            seen.insert(fnv1a_64(path.as_bytes()));
            batch.push((path.to_string(), is_dir, metadata));
            if batch.len() >= RECONCILE_BATCH {
                live = apply(&mut batch);
            }
            live
        });
        if !live {
            return count;
        }
    }
    if !apply(&mut batch) {
        return count;
    }

    let mut unseen: Vec<String> = Vec::new();
    if !with_index(&mut |index| {
        unseen = index
            .entries
            .iter()
            .filter(|entry| !seen.contains(&fnv1a_64(entry.path.as_bytes())))
            .map(|entry| entry.path.clone())
            .collect();
    }) {
        return count;
    }
    // Removals are costly and can number in the thousands after lost watcher
    // events, so apply them in small steps. Each step re-checks the disk while
    // it holds the index, so a path another writer re-created is kept.
    for batch in unseen.chunks(RECONCILE_REMOVE_BATCH) {
        let live = with_index(&mut |index| {
            for path in batch {
                if !still_indexed(Path::new(path), roots, excludes) {
                    index.remove(path);
                }
            }
        });
        if !live {
            break;
        }
    }
    count
}

/// Whether `path` lies under one of `roots` but is itself excluded or inside an
/// excluded directory, so a walk of that root never indexes it. Only the path
/// text is inspected; paths under no root are left to the caller.
pub fn excluded_under_roots(path: &Path, roots: &[PathBuf], excludes: &Excludes) -> bool {
    roots.iter().any(|root| {
        path != root
            && path.starts_with(root)
            && path
                .ancestors()
                .take_while(|ancestor| ancestor != root)
                .any(|ancestor| excludes.is_excluded(ancestor))
    })
}

/// Whether a walk of `roots` would index `path` as it is on disk now.
fn still_indexed(path: &Path, roots: &[PathBuf], excludes: &Excludes) -> bool {
    if has_hidden_ancestor_dir(path) {
        return false;
    }
    let under_root = roots.iter().any(|root| {
        path != root
            && path.starts_with(root)
            && path
                .ancestors()
                .take_while(|ancestor| ancestor != root)
                .all(|ancestor| !excludes.is_excluded(ancestor))
    });
    if !under_root {
        return false;
    }
    match fs::symlink_metadata(path) {
        Ok(md) => !md.file_type().is_symlink() && !is_hidden_dir_path(path, md.is_dir()),
        Err(_) => false,
    }
}

fn visit(
    root: &Path,
    excludes: &Excludes,
    fetch_metadata: bool,
    mut on_entry: impl FnMut(&str, bool, (u64, i64, i64, i64)) -> bool,
) -> usize {
    let mut count = 0;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if excludes.is_excluded(&dir) && dir != root {
            continue;
        }

        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
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
                let size = metadata.as_ref().map_or(0, std::fs::Metadata::len);
                let modified = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs().cast_signed());
                let created = metadata
                    .as_ref()
                    .and_then(|m| m.created().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs().cast_signed());
                let accessed = metadata
                    .as_ref()
                    .and_then(|m| m.accessed().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs().cast_signed());

                (size, modified, created, accessed)
            } else {
                (0, 0, 0, 0)
            };
            let keep_going = on_entry(path.to_str().unwrap_or(""), is_dir, metadata);
            count += 1;
            if !keep_going {
                return count;
            }

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
