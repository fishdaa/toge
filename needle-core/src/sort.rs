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
    let _ = (index, ids, key, ascending);
    todo!()
}

#[cfg(test)]
mod tests;
