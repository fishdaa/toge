//! Sorting utilities and fast-sort indexes.

use crate::index::Index;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Path,
    Size,
    Modified,
    Created,
    Accessed,
    Extension,
}

pub fn sort_ids(index: &Index, ids: &mut [u32], key: SortKey, ascending: bool) {
    match key {
        SortKey::Name => {
            let mut cached: Vec<(u32, &str)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].name()))
                .collect();
            cached.sort_by(|a, b| cmp_str(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
        SortKey::Path => {
            let mut cached: Vec<(u32, &str)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].path.as_str()))
                .collect();
            cached.sort_by(|a, b| cmp_str(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
        SortKey::Extension => {
            let mut cached: Vec<(u32, &str)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].extension()))
                .collect();
            cached.sort_by(|a, b| cmp_str(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
        SortKey::Size => {
            let mut cached: Vec<(u32, u64)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].size))
                .collect();
            cached.sort_by(|a, b| cmp_u64(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
        SortKey::Modified => {
            let mut cached: Vec<(u32, i64)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].modified))
                .collect();
            cached.sort_by(|a, b| cmp_i64(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
        SortKey::Created => {
            let mut cached: Vec<(u32, i64)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].created))
                .collect();
            cached.sort_by(|a, b| cmp_i64(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
        SortKey::Accessed => {
            let mut cached: Vec<(u32, i64)> = ids
                .iter()
                .map(|&id| (id, index.entries[id as usize].accessed))
                .collect();
            cached.sort_by(|a, b| cmp_i64(a.1, b.1, ascending));
            for (i, &(id, _)) in cached.iter().enumerate() {
                ids[i] = id;
            }
        }
    }
}

#[inline]
fn cmp_str(a: &str, b: &str, ascending: bool) -> Ordering {
    if ascending {
        a.cmp(b)
    } else {
        b.cmp(a)
    }
}

#[inline]
fn cmp_u64(a: u64, b: u64, ascending: bool) -> Ordering {
    if ascending {
        a.cmp(&b)
    } else {
        b.cmp(&a)
    }
}

#[inline]
fn cmp_i64(a: i64, b: i64, ascending: bool) -> Ordering {
    if ascending {
        a.cmp(&b)
    } else {
        b.cmp(&a)
    }
}

#[cfg(test)]
mod tests;
