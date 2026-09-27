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

/// Name and path orders of the whole index, reused across queries. Sorting a
/// query's matches by a cached order is a rank lookup instead of a string sort,
/// which dominates query time for large result sets. The cache rebuilds itself
/// whenever entries were added or removed since it was built.
#[derive(Debug, Default)]
pub struct OrderCache {
    name: Option<CachedOrder>,
    path: Option<CachedOrder>,
}

#[derive(Debug)]
struct CachedOrder {
    epoch: u64,
    revision: u64,
    ascending: Ranked,
    /// Built on first use: descending keys, ties still in ascending ID order,
    /// matching the stable sort in [`sort_ids`].
    descending: Option<Ranked>,
}

#[derive(Debug)]
struct Ranked {
    /// Every entry ID in sort order.
    order: Vec<u32>,
    /// `rank[id]` is the position of `id` in `order`.
    rank: Vec<u32>,
}

impl Ranked {
    fn new(order: Vec<u32>) -> Self {
        let mut rank = vec![0u32; order.len()];
        for (position, &id) in order.iter().enumerate() {
            rank[id as usize] = position as u32;
        }
        Self { order, rank }
    }

    fn sort(&self, ids: &mut [u32]) {
        // Large result sets: one pass over the cached order picks the matches.
        // Small ones: sort the matches by rank.
        if ids.len() > self.order.len() / 16 {
            let mut matched = vec![0u64; self.order.len().div_ceil(64)];
            for &id in ids.iter() {
                matched[id as usize / 64] |= 1 << (id % 64);
            }
            let hits = self
                .order
                .iter()
                .filter(|&&id| matched[id as usize / 64] & (1 << (id % 64)) != 0);
            for (slot, &id) in ids.iter_mut().zip(hits) {
                *slot = id;
            }
        } else {
            ids.sort_unstable_by_key(|&id| self.rank[id as usize]);
        }
    }
}

type KeyFn = fn(&crate::index::Entry) -> &str;

impl CachedOrder {
    fn build(index: &Index, key: KeyFn) -> Self {
        // An 8-byte big-endian prefix orders like the string itself, so most
        // comparisons avoid chasing the string pointer.
        let mut keyed: Vec<(u64, u32, &str)> = index
            .entries
            .iter()
            .enumerate()
            .map(|(id, entry)| {
                let text = key(entry);
                let mut prefix = [0u8; 8];
                let len = text.len().min(8);
                prefix[..len].copy_from_slice(&text.as_bytes()[..len]);
                (u64::from_be_bytes(prefix), id as u32, text)
            })
            .collect();
        keyed.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(b.2)).then(a.1.cmp(&b.1)));
        Self {
            epoch: index.epoch(),
            revision: index.revision(),
            ascending: Ranked::new(keyed.into_iter().map(|(_, id, _)| id).collect()),
            descending: None,
        }
    }

    /// Merge entries appended since the cache was built. Removals change the
    /// epoch and force a full rebuild instead, since they renumber IDs.
    fn extend(&mut self, index: &Index, key: KeyFn) {
        let old = &self.ascending.order;
        let first_new = old.len() as u32;
        let mut added: Vec<u32> = (first_new..index.count() as u32).collect();
        added
            .sort_by(|&a, &b| key(&index.entries[a as usize]).cmp(key(&index.entries[b as usize])));
        let mut order = Vec::with_capacity(index.count());
        let mut copied = 0;
        for id in added {
            let text = key(&index.entries[id as usize]);
            // New IDs sort after every existing ID with an equal key.
            let at = copied
                + old[copied..]
                    .partition_point(|&existing| key(&index.entries[existing as usize]) <= text);
            order.extend_from_slice(&old[copied..at]);
            order.push(id);
            copied = at;
        }
        order.extend_from_slice(&old[copied..]);
        self.ascending = Ranked::new(order);
        self.descending = None;
        self.revision = index.revision();
    }

    fn descending(&mut self, index: &Index, key: KeyFn) -> &Ranked {
        self.descending.get_or_insert_with(|| {
            // Reverse the keys, then restore ascending IDs within each tie run.
            let mut order: Vec<u32> = self.ascending.order.iter().rev().copied().collect();
            let mut start = 0;
            while start < order.len() {
                let text = key(&index.entries[order[start] as usize]);
                let mut end = start + 1;
                while end < order.len() && key(&index.entries[order[end] as usize]) == text {
                    end += 1;
                }
                order[start..end].reverse();
                start = end;
            }
            Ranked::new(order)
        })
    }
}

impl OrderCache {
    /// Sort `ids` (distinct entry IDs in ascending order) exactly like
    /// [`sort_ids`], using a cached whole-index order for name and path keys.
    pub fn sort(&mut self, index: &Index, ids: &mut [u32], key: SortKey, ascending: bool) {
        let (slot, key_fn): (_, KeyFn) = match key {
            SortKey::Name => (&mut self.name, |entry| entry.name()),
            SortKey::Path => (&mut self.path, |entry| entry.path.as_str()),
            _ => return sort_ids(index, ids, key, ascending),
        };
        match slot {
            Some(cached)
                if cached.epoch == index.epoch()
                    && cached.ascending.order.len() <= index.count() =>
            {
                if cached.revision != index.revision() {
                    cached.extend(index, key_fn);
                }
            }
            _ => *slot = Some(CachedOrder::build(index, key_fn)),
        }
        let cached = slot.as_mut().unwrap();
        if ascending {
            cached.ascending.sort(ids);
        } else {
            cached.descending(index, key_fn).sort(ids);
        }
    }
}

#[inline]
fn cmp_str(a: &str, b: &str, ascending: bool) -> Ordering {
    if ascending { a.cmp(b) } else { b.cmp(a) }
}

#[inline]
fn cmp_u64(a: u64, b: u64, ascending: bool) -> Ordering {
    if ascending { a.cmp(&b) } else { b.cmp(&a) }
}

#[inline]
fn cmp_i64(a: i64, b: i64, ascending: bool) -> Ordering {
    if ascending { a.cmp(&b) } else { b.cmp(&a) }
}

#[cfg(test)]
mod tests;
