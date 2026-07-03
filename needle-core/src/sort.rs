//! Sorting utilities and fast-sort indexes.

use crate::index::Index;

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
    ids.sort_by(|a, b| {
        let a = &index.entries[*a as usize];
        let b = &index.entries[*b as usize];
        let ord = match key {
            SortKey::Name => a.name().cmp(b.name()),
            SortKey::Path => a.path.cmp(&b.path),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Created => a.created.cmp(&b.created),
            SortKey::Accessed => a.accessed.cmp(&b.accessed),
            SortKey::Extension => a.extension().cmp(b.extension()),
        };
        if ascending {
            ord
        } else {
            ord.reverse()
        }
    });
}

#[cfg(test)]
mod tests;
