//! Sorting utilities and fast-sort indexes.

use crate::index::Index;
use std::cmp::Ordering;
use std::collections::HashMap;

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

/// A stale [`OrderCache`] is skipped for match sets of at most
/// 1/`DIRECT_SORT_DIVISOR` of the index.
const DIRECT_SORT_DIVISOR: usize = 32;

/// Name and path orders of the whole index, reused across queries. Sorting a
/// query's matches by a cached order is a rank lookup instead of a string sort,
/// which dominates query time for large result sets. The cache merges entries
/// added since it was built, replays recent removals, and rebuilds only when
/// the index was replaced or the removal log no longer reaches back far enough.
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
            // `position` is bounded by `order.len()`, well under `u32::MAX`;
            // see `Index::entry_count_u32`.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "position < order.len(), which fits in u32; see Index::entry_count_u32"
            )]
            let position = position as u32;
            rank[id as usize] = position;
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
        // `id` is bounded by `index.entries.len()`, well under `u32::MAX`;
        // see `Index::entry_count_u32`.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "id < index.entries.len(), which fits in u32; see Index::entry_count_u32"
        )]
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

    /// Merge entries appended since the cache was built.
    fn extend(&mut self, index: &Index, key: KeyFn) {
        // Bounded by `index.count()`, well under `u32::MAX`; see `Index::entry_count_u32`.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "order.len() and index.count() fit in u32; see Index::entry_count_u32"
        )]
        let mut added: Vec<u32> =
            (self.ascending.order.len() as u32..index.count() as u32).collect();
        let old = std::mem::take(&mut self.ascending.order);
        self.merge(index, key, &old, &mut added);
    }

    /// Merge `added` IDs into `old`, an order of every other current entry.
    fn merge(&mut self, index: &Index, key: KeyFn, old: &[u32], added: &mut [u32]) {
        let key_of = |id: u32| key(&index.entries[id as usize]);
        added.sort_by(|&a, &b| key_of(a).cmp(key_of(b)).then(a.cmp(&b)));
        let mut order = Vec::with_capacity(index.count());
        let mut copied = 0;
        for &id in added.iter() {
            let text = key_of(id);
            // Equal keys stay in ascending ID order.
            let at = copied
                + old[copied..].partition_point(|&existing| {
                    let existing_text = key_of(existing);
                    existing_text < text || (existing_text == text && existing < id)
                });
            order.extend_from_slice(&old[copied..at]);
            order.push(id);
            copied = at;
        }
        order.extend_from_slice(&old[copied..]);
        self.ascending = Ranked::new(order);
        self.descending = None;
        self.epoch = index.epoch();
        self.revision = index.revision();
    }

    /// Replay `removals` (see [`Index::removals_since`]) onto the cached order:
    /// drop removed IDs and renumber moved ones. Keys never change, so only
    /// ties involving a renumbered ID need reordering, instead of a full sort.
    fn renumber(&mut self, index: &Index, key: KeyFn, removals: &[(u32, u32)]) {
        let key_of = |id: u32| key(&index.entries[id as usize]);
        // Bounded by `index.count()`, well under `u32::MAX`; see `Index::entry_count_u32`.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "order.len() fits in u32; see Index::entry_count_u32"
        )]
        let cached_len = self.ascending.order.len() as u32;
        // Which cached ID currently occupies a slot, for slots touched so far.
        let mut owner: HashMap<u32, Option<u32>> = HashMap::new();
        // Cached ID -> its current ID, or None once removed.
        let mut remap: HashMap<u32, Option<u32>> = HashMap::new();
        for &(removed, moved_from) in removals {
            let owner_of = |owner: &HashMap<u32, Option<u32>>, slot: u32| {
                owner
                    .get(&slot)
                    .copied()
                    .unwrap_or((slot < cached_len).then_some(slot))
            };
            if let Some(cached) = owner_of(&owner, removed) {
                remap.insert(cached, None);
            }
            if removed != moved_from {
                let moved = owner_of(&owner, moved_from);
                owner.insert(removed, moved);
                if let Some(cached) = moved {
                    remap.insert(cached, Some(removed));
                }
            }
            owner.insert(moved_from, None);
        }
        let mut order: Vec<u32> = self
            .ascending
            .order
            .iter()
            .filter_map(|&id| remap.get(&id).copied().unwrap_or(Some(id)))
            .collect();

        let count = index.count();
        let mut moved = vec![false; count];
        for &id in remap.values().flatten() {
            moved[id as usize] = true;
        }
        let mut position = 0;
        while position < order.len() {
            if !moved[order[position] as usize] {
                position += 1;
                continue;
            }
            let text = key_of(order[position]);
            let mut start = position;
            while start > 0 && key_of(order[start - 1]) == text {
                start -= 1;
            }
            let mut end = position + 1;
            while end < order.len() && key_of(order[end]) == text {
                end += 1;
            }
            order[start..end].sort_unstable();
            position = end;
        }

        // Entries appended after the cache was built may have moved into
        // lower slots, so find them by absence rather than by ID range.
        let mut present = vec![false; count];
        for &id in &order {
            present[id as usize] = true;
        }
        // Bounded by `index.count()`, well under `u32::MAX`; see `Index::entry_count_u32`.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "count fits in u32; see Index::entry_count_u32"
        )]
        let mut added: Vec<u32> = (0..count as u32)
            .filter(|&id| !present[id as usize])
            .collect();
        self.merge(index, key, &order, &mut added);
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
        // Bringing a stale order up to date costs time proportional to the
        // whole index, which a small match set does not repay while the index
        // keeps changing; sort those matches directly and leave the cache for
        // a larger query.
        let current = slot.as_ref().is_some_and(|cached| {
            cached.epoch == index.epoch() && cached.revision == index.revision()
        });
        if !current && ids.len() <= index.count() / DIRECT_SORT_DIVISOR {
            return sort_ids(index, ids, key, ascending);
        }
        match slot {
            Some(cached)
                if cached.epoch == index.epoch()
                    && cached.ascending.order.len() <= index.count() =>
            {
                if cached.revision != index.revision() {
                    cached.extend(index, key_fn);
                }
            }
            // Removals renumber IDs; patch the order rather than re-sorting
            // every entry while the caller holds the index.
            Some(cached) if let Some(removals) = index.removals_since(cached.epoch) => {
                cached.renumber(index, key_fn, removals);
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
