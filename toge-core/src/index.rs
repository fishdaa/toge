//! Core in-memory index.

use std::collections::HashMap;

/// An entry in the search index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub name_off: u16,
    pub ext_off: u16,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
}

impl Entry {
    /// Return the filename portion of the path.
    pub fn name(&self) -> &str {
        &self.path[self.name_off as usize..]
    }

    /// Return the extension (without dot), or empty string if none.
    pub fn extension(&self) -> &str {
        let name_start = self.name_off as usize;
        let ext_start = self.ext_off as usize;
        if ext_start > name_start && ext_start < self.path.len() {
            &self.path[ext_start..]
        } else {
            ""
        }
    }
}

pub(crate) fn fnv1a_64(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a_extend(FNV_OFFSET, data)
}

pub(crate) fn fnv1a_extend(mut hash: u64, data: &[u8]) -> u64 {
    const FNV_PRIME: u64 = 0x000_0100_0000_01b3;
    for &b in data {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Lowercase a string and return the byte vector (used at insert/rebuild time only).
#[inline]
pub(crate) fn lowered_bytes(s: &str) -> Vec<u8> {
    s.to_lowercase().into_bytes()
}

/// Pack 3 ASCII bytes into a u32 trigram key.
#[inline]
pub(crate) fn pack_trigram(a: u8, b: u8, c: u8) -> u32 {
    u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c)
}

/// Extract trigram keys from a lowercased byte slice.
#[inline]
pub(crate) fn extract_trigrams(name_lower: &[u8]) -> Vec<u32> {
    if name_lower.len() < 3 {
        return Vec::new();
    }
    let mut trigrams = Vec::with_capacity(name_lower.len() - 2);
    for i in 0..name_lower.len() - 2 {
        trigrams.push(pack_trigram(
            name_lower[i],
            name_lower[i + 1],
            name_lower[i + 2],
        ));
    }
    trigrams
}

#[inline]
pub(crate) fn unique_trigrams(name_lower: &[u8]) -> Vec<u32> {
    let mut trigrams = extract_trigrams(name_lower);
    trigrams.sort_unstable();
    trigrams.dedup();
    trigrams
}

/// Intersect sorted trigram posting lists, starting with the smallest list.
/// Returns entries appearing in all lists.
pub(crate) fn intersect_trigram_lists(trigrams: &HashMap<u32, Vec<u32>>, keys: &[u32]) -> Vec<u32> {
    if keys.is_empty() {
        return Vec::new();
    }

    let mut lists = Vec::<&[u32]>::with_capacity(keys.len());
    for key in keys {
        let Some(list) = trigrams.get(key) else {
            return Vec::new();
        };
        lists.push(list);
    }
    lists.sort_unstable_by_key(|list| list.len());

    if lists.len() == 1 {
        return lists[0].to_vec();
    }

    let mut idx: Vec<usize> = vec![0; lists.len()];
    let mut result = Vec::new();

    'outer: while idx[0] < lists[0].len() {
        let candidate = lists[0][idx[0]];

        for i in 1..lists.len() {
            while idx[i] < lists[i].len() && lists[i][idx[i]] < candidate {
                idx[i] += 1;
            }
            if idx[i] >= lists[i].len() || lists[i][idx[i]] != candidate {
                idx[0] += 1;
                continue 'outer;
            }
        }

        result.push(candidate);
        for cursor in idx.iter_mut().take(lists.len()) {
            *cursor += 1;
        }
    }

    result
}

/// Zero-allocation case-insensitive substring check.
/// `needle_lower` must already be lowercased; `haystack` is lowercased byte-by-byte during comparison.
#[inline]
pub(crate) fn contains_ignore_case(haystack: &str, needle_lower: &[u8]) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if !haystack.is_ascii() || !needle_lower.is_ascii() {
        let needle = std::str::from_utf8(needle_lower).unwrap_or_default();
        return haystack.to_lowercase().contains(needle);
    }
    let hb = haystack.as_bytes();
    if needle_lower.len() > hb.len() {
        return false;
    }
    if needle_lower.len() == 1 {
        let n = needle_lower[0].to_ascii_lowercase();
        return hb.iter().any(|&b| b.to_ascii_lowercase() == n);
    }
    hb.windows(needle_lower.len()).any(|w| {
        w.iter()
            .zip(needle_lower)
            .all(|(&a, &b)| a.to_ascii_lowercase() == b)
    })
}

/// Zero-allocation case-insensitive prefix check.
#[inline]
pub(crate) fn starts_with_ignore_case(haystack: &str, prefix_lower: &[u8]) -> bool {
    if !haystack.is_ascii() || !prefix_lower.is_ascii() {
        let prefix = std::str::from_utf8(prefix_lower).unwrap_or_default();
        return haystack.to_lowercase().starts_with(prefix);
    }
    let hb = haystack.as_bytes();
    hb.len() >= prefix_lower.len()
        && hb
            .iter()
            .zip(prefix_lower)
            .all(|(&a, &b)| a.to_ascii_lowercase() == b)
}

/// Grow index vectors by at most 25% (or a few elements for small lists),
/// rather than doubling a large compacted index on the next watcher insert.
pub(crate) fn push_index_value<T>(values: &mut Vec<T>, value: T) {
    if values.len() == values.capacity() {
        values.reserve_exact((values.len() / 4).max(4));
    }
    values.push(value);
}

/// Tiered search index.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub entries: Vec<Entry>,
    pub(crate) by_ext: HashMap<String, Vec<u32>>,
    pub(crate) path_to_id: HashMap<u64, u32>,
    pub(crate) trigrams: HashMap<u32, Vec<u32>>,
    pub(crate) prefix_first_byte: HashMap<u8, Vec<u32>>,
    /// Bumped whenever existing IDs may refer to different entries (removal
    /// swaps the last entry into the freed slot, or the index is replaced).
    pub(crate) epoch: u64,
    /// Bumped whenever the set of entries changes.
    pub(crate) revision: u64,
    /// Bumped whenever an existing entry's size or timestamps change, so
    /// results that filter or sort by metadata can refresh. Name and path
    /// orders ignore it.
    pub(crate) metadata_revision: u64,
    /// Recent removals as `(removed_id, moved_from_id)`: record `i` moved the
    /// index from epoch `removal_base + i` to the next. Lets ID-keyed caches
    /// renumber in place instead of rebuilding.
    pub(crate) removals: Vec<(u32, u32)>,
    pub(crate) removal_base: u64,
}

/// Converts a position in [`Index::entries`] to its entry ID.
///
/// [`Index::insert_with_metadata`] refuses to grow an index to `u32::MAX`
/// entries, so every position and the entry count itself fit in a `u32`.
pub fn entry_id(position: usize) -> u32 {
    u32::try_from(position).expect("an index holds fewer than u32::MAX entries")
}

/// Removal records kept for [`Index::removals_since`]; older caches rebuild.
const REMOVAL_LOG_LIMIT: usize = 1 << 16;

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    fn entry_count_u32(&self) -> u32 {
        entry_id(self.entries.len())
    }

    /// Counter that changes whenever previously returned IDs may be stale.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Counter that changes whenever an entry is added or removed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Counter that changes whenever an existing entry's size or timestamps
    /// change through [`Index::insert_with_metadata`].
    pub fn metadata_revision(&self) -> u64 {
        self.metadata_revision
    }

    /// Mark this index as the replacement of `previous`, invalidating IDs
    /// handed out by it.
    pub fn succeed(&mut self, previous: &Index) {
        self.epoch = previous.epoch.max(self.epoch) + 1;
        self.revision = previous.revision.max(self.revision) + 1;
        self.metadata_revision = previous.metadata_revision.max(self.metadata_revision) + 1;
        self.removals.clear();
        self.removal_base = self.epoch;
    }

    /// The removals that took this index from `epoch` to the current one, as
    /// `(removed_id, moved_from_id)` in order, or `None` if they are no longer
    /// known (the index was replaced, or too much has changed since).
    pub fn removals_since(&self, epoch: u64) -> Option<&[(u32, u32)]> {
        let start = epoch.checked_sub(self.removal_base)?;
        let start = usize::try_from(start).ok()?;
        self.removals.get(start..)
    }

    /// Where `removals` (see [`Index::removals_since`]) moved the IDs of the
    /// first `count` entries: `Some(new_id)` for a moved entry, `None` for a
    /// removed one. IDs missing from the map still refer to the same entry.
    pub fn renumbering(removals: &[(u32, u32)], count: u32) -> HashMap<u32, Option<u32>> {
        // Which original ID currently occupies a slot, for slots touched so far.
        let mut owner: HashMap<u32, Option<u32>> = HashMap::new();
        let mut remap: HashMap<u32, Option<u32>> = HashMap::new();
        let owner_of = |owner: &HashMap<u32, Option<u32>>, slot: u32| {
            owner
                .get(&slot)
                .copied()
                .unwrap_or((slot < count).then_some(slot))
        };
        for &(removed, moved_from) in removals {
            if let Some(original) = owner_of(&owner, removed) {
                remap.insert(original, None);
            }
            if removed != moved_from {
                let moved = owner_of(&owner, moved_from);
                owner.insert(removed, moved);
                if let Some(original) = moved {
                    remap.insert(original, Some(removed));
                }
            }
            owner.insert(moved_from, None);
        }
        remap
    }

    /// Insert or update `path`, returning its entry ID. See
    /// [`Index::insert_with_metadata`] for when this returns `None`.
    pub fn insert(&mut self, path: &str, is_dir: bool) -> Option<u32> {
        self.insert_with_metadata(path, is_dir, 0, 0, 0, 0)
    }

    /// Insert or update `path` with its metadata, returning its entry ID.
    ///
    /// Returns `None`, leaving the index unchanged, when the path cannot be
    /// stored: its filename or extension starts past byte `u16::MAX` (entries
    /// keep those offsets as `u16`), or the index already holds
    /// `u32::MAX - 1` entries and has no ID left to hand out.
    pub fn insert_with_metadata(
        &mut self,
        path: &str,
        is_dir: bool,
        size: u64,
        modified: i64,
        created: i64,
        accessed: i64,
    ) -> Option<u32> {
        let path_hash = fnv1a_64(path.as_bytes());
        let mut replaces_existing = false;
        if let Some(&id) = self.path_to_id.get(&path_hash) {
            let entry = &mut self.entries[id as usize];
            replaces_existing = entry.path == path;
            if replaces_existing && entry.is_dir == is_dir {
                if (entry.size, entry.modified, entry.created, entry.accessed)
                    != (size, modified, created, accessed)
                {
                    self.metadata_revision += 1;
                }
                entry.size = size;
                entry.modified = modified;
                entry.created = created;
                entry.accessed = accessed;
                return Some(id);
            }
        }

        let name_start = path.rfind('/').map_or(0, |i| i + 1);
        let name = &path[name_start..];
        let ext_start = if is_dir {
            0
        } else {
            name.rfind('.').map_or(0, |i| name_start + i + 1)
        };
        let (Ok(name_off), Ok(ext_off)) = (u16::try_from(name_start), u16::try_from(ext_start))
        else {
            return None;
        };
        if self.entry_count_u32() == u32::MAX {
            return None;
        }
        if replaces_existing {
            self.remove(path);
        }
        let id = self.entry_count_u32();
        self.revision += 1;

        let entry = Entry {
            path: path.to_string(),
            name_off,
            ext_off,
            is_dir,
            size,
            modified,
            created,
            accessed,
        };
        push_index_value(&mut self.entries, entry);

        self.path_to_id.insert(path_hash, id);

        if !is_dir {
            let ext = if ext_off > name_off {
                &path[ext_off as usize..]
            } else {
                ""
            };
            push_index_value(self.by_ext.entry(ext.to_string()).or_default(), id);
        }

        // Insert into trigram and prefix indexes using a temporary lowered copy.
        let name_lower = lowered_bytes(name);
        for trigram in unique_trigrams(&name_lower) {
            push_index_value(self.trigrams.entry(trigram).or_default(), id);
        }
        if let Some(&first_byte) = name_lower.first() {
            push_index_value(self.prefix_first_byte.entry(first_byte).or_default(), id);
        }

        Some(id)
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let path_hash = fnv1a_64(path.as_bytes());
        let Some(&id) = self.path_to_id.get(&path_hash) else {
            return false;
        };
        let entry = &self.entries[id as usize];
        if entry.path != path {
            return false;
        }

        self.path_to_id.remove(&path_hash);
        if self.removal_base + self.removals.len() as u64 != self.epoch {
            self.removals.clear();
            self.removal_base = self.epoch;
        }
        if self.removals.len() >= REMOVAL_LOG_LIMIT {
            self.removals.drain(..REMOVAL_LOG_LIMIT / 2);
            self.removal_base += (REMOVAL_LOG_LIMIT / 2) as u64;
        }
        self.removals.push((id, self.entry_count_u32() - 1));
        self.epoch += 1;
        self.revision += 1;

        let is_dir = entry.is_dir;
        let name_lower = lowered_bytes(entry.name());
        let ext = if is_dir {
            String::new()
        } else {
            entry.extension().to_string()
        };

        // Remove from trigram index.
        for trigram in unique_trigrams(&name_lower) {
            if let Some(list) = self.trigrams.get_mut(&trigram)
                && let Ok(pos) = list.binary_search(&id)
            {
                list.remove(pos);
            }
        }

        // Remove from prefix first-byte bucket.
        if let Some(&first_byte) = name_lower.first()
            && let Some(list) = self.prefix_first_byte.get_mut(&first_byte)
            && let Ok(pos) = list.binary_search(&id)
        {
            list.remove(pos);
        }

        // Remove from by_ext.
        if !is_dir
            && let Some(list) = self.by_ext.get_mut(&ext)
            && let Ok(pos) = list.binary_search(&id)
        {
            list.remove(pos);
        }

        let old_last_id = self.entry_count_u32() - 1;
        self.entries.swap_remove(id as usize);

        if id != old_last_id {
            let swapped_entry = &self.entries[id as usize];
            let swapped_hash = fnv1a_64(swapped_entry.path.as_bytes());
            self.path_to_id.insert(swapped_hash, id);

            let swapped_name_lower = lowered_bytes(swapped_entry.name());
            for trigram in unique_trigrams(&swapped_name_lower) {
                if let Some(list) = self.trigrams.get_mut(&trigram) {
                    replace_sorted_id(list, old_last_id, id);
                }
            }
            if let Some(&first_byte) = swapped_name_lower.first()
                && let Some(list) = self.prefix_first_byte.get_mut(&first_byte)
            {
                replace_sorted_id(list, old_last_id, id);
            }
            if !swapped_entry.is_dir {
                let swapped_ext = swapped_entry.extension().to_string();
                if let Some(list) = self.by_ext.get_mut(&swapped_ext) {
                    replace_sorted_id(list, old_last_id, id);
                }
            }
        }

        true
    }

    pub fn update_metadata(&mut self, path: &str) -> bool {
        let path_hash = fnv1a_64(path.as_bytes());
        let Some(&id) = self.path_to_id.get(&path_hash) else {
            return false;
        };
        if self.entries[id as usize].path != path {
            return false;
        }
        self.update_metadata_by_id(id)
    }

    /// Refresh an entry without allocating a copy of its path.
    pub fn update_metadata_by_id(&mut self, id: u32) -> bool {
        let Some(entry) = self.entries.get_mut(id as usize) else {
            return false;
        };
        if let Ok(metadata) = std::fs::metadata(&entry.path) {
            entry.size = metadata.len();
            if let Ok(t) = metadata.modified()
                && let Ok(d) = t.duration_since(std::time::UNIX_EPOCH)
            {
                entry.modified = d.as_secs().cast_signed();
            }
            if let Ok(t) = metadata.created()
                && let Ok(d) = t.duration_since(std::time::UNIX_EPOCH)
            {
                entry.created = d.as_secs().cast_signed();
            }
            if let Ok(t) = metadata.accessed()
                && let Ok(d) = t.duration_since(std::time::UNIX_EPOCH)
            {
                entry.accessed = d.as_secs().cast_signed();
            }
        }
        true
    }

    pub fn search_substring(&self, text: &str) -> Vec<u32> {
        let needle = text.to_lowercase();
        let needle_bytes = needle.as_bytes();

        if needle_bytes.len() >= 3 {
            let trigrams_keys = unique_trigrams(needle_bytes);
            let candidates = intersect_trigram_lists(&self.trigrams, &trigrams_keys);
            candidates
                .into_iter()
                .filter(|&id| {
                    let entry = &self.entries[id as usize];
                    contains_ignore_case(entry.name(), needle_bytes)
                })
                .collect()
        } else if needle_bytes.is_empty() {
            (0..self.entry_count_u32()).collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| contains_ignore_case(e.name(), needle_bytes))
                .map(|(i, _)| entry_id(i))
                .collect()
        }
    }

    pub fn search_prefix(&self, prefix: &str) -> Vec<u32> {
        let prefix_lower = prefix.to_lowercase();
        let prefix_bytes = prefix_lower.as_bytes();

        if prefix_bytes.is_empty() {
            return (0..self.entry_count_u32()).collect();
        }

        if let Some(first_byte) = prefix_bytes.first()
            && let Some(bucket) = self.prefix_first_byte.get(first_byte)
        {
            return bucket
                .iter()
                .filter(|&&id| {
                    let entry = &self.entries[id as usize];
                    starts_with_ignore_case(entry.name(), prefix_bytes)
                })
                .copied()
                .collect();
        }

        Vec::new()
    }

    pub fn get_path(&self, id: u32) -> Option<&str> {
        self.entries.get(id as usize).map(|e| e.path.as_str())
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Release spare capacity after a bulk build or reconciliation.
    /// Search IDs and ordering are unchanged; subsequent inserts remain supported.
    pub fn compact(&mut self) {
        self.entries.shrink_to_fit();
        for entry in &mut self.entries {
            entry.path.shrink_to_fit();
        }
        self.by_ext.retain(|_, ids| !ids.is_empty());
        self.trigrams.retain(|_, ids| !ids.is_empty());
        self.prefix_first_byte.retain(|_, ids| !ids.is_empty());
        for ids in self
            .by_ext
            .values_mut()
            .chain(self.trigrams.values_mut())
            .chain(self.prefix_first_byte.values_mut())
        {
            ids.shrink_to_fit();
        }
        self.by_ext.shrink_to_fit();
        self.path_to_id.shrink_to_fit();
        self.trigrams.shrink_to_fit();
        self.prefix_first_byte.shrink_to_fit();
    }

    /// Estimate allocated index storage, including spare vector capacity and postings.
    /// Hash table control bytes and allocator overhead are not included.
    pub fn metadata_size(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<Entry>()
            + self
                .entries
                .iter()
                .map(|e| e.path.capacity())
                .sum::<usize>()
            + self.by_ext.capacity() * std::mem::size_of::<(String, Vec<u32>)>()
            + self
                .by_ext
                .keys()
                .map(std::string::String::capacity)
                .sum::<usize>()
            + self.path_to_id.capacity() * std::mem::size_of::<(u64, u32)>()
            + self.trigrams.capacity() * std::mem::size_of::<(u32, Vec<u32>)>()
            + self.prefix_first_byte.capacity() * std::mem::size_of::<(u8, Vec<u32>)>()
            + self
                .by_ext
                .values()
                .chain(self.trigrams.values())
                .chain(self.prefix_first_byte.values())
                .map(|ids| ids.capacity() * std::mem::size_of::<u32>())
                .sum::<usize>()
    }

    #[allow(dead_code)]
    fn rebuild_maps(&mut self) {
        self.path_to_id.clear();
        self.by_ext.clear();
        self.trigrams.clear();
        self.prefix_first_byte.clear();
        for (id, entry) in self.entries.iter().enumerate() {
            let id = entry_id(id);
            let path_hash = fnv1a_64(entry.path.as_bytes());
            self.path_to_id.insert(path_hash, id);
            if !entry.is_dir {
                let ext = entry.extension().to_string();
                push_index_value(self.by_ext.entry(ext).or_default(), id);
            }
            let name_lower = lowered_bytes(entry.name());
            for trigram in unique_trigrams(&name_lower) {
                push_index_value(self.trigrams.entry(trigram).or_default(), id);
            }
            if let Some(&first_byte) = name_lower.first() {
                push_index_value(self.prefix_first_byte.entry(first_byte).or_default(), id);
            }
        }
    }

    /// Look up entries by extension (used by the matcher).
    pub fn by_extension(&self, ext: &str) -> Option<&[u32]> {
        self.by_ext.get(ext).map(std::vec::Vec::as_slice)
    }

    /// Look up an entry id by full path.
    pub fn id_by_path(&self, path: &str) -> Option<u32> {
        let path_hash = fnv1a_64(path.as_bytes());
        self.path_to_id.get(&path_hash).copied()
    }
}

fn replace_sorted_id(list: &mut Vec<u32>, old_id: u32, new_id: u32) {
    if old_id == new_id {
        return;
    }
    if let Ok(pos) = list.binary_search(&old_id) {
        list.remove(pos);
        let insert_at = list.binary_search(&new_id).unwrap_or_else(|pos| pos);
        list.insert(insert_at, new_id);
    }
}

#[cfg(test)]
mod tests;
