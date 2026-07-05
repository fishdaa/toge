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
    const FNV_PRIME: u64 = 0x000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Lowercase a string and return the byte vector (used at insert/rebuild time only).
#[inline]
pub(crate) fn lowered_bytes(s: &str) -> Vec<u8> {
    s.to_lowercase().bytes().collect()
}

/// Pack 3 ASCII bytes into a u32 trigram key.
#[inline]
pub(crate) fn pack_trigram(a: u8, b: u8, c: u8) -> u32 {
    (a as u32) << 16 | (b as u32) << 8 | (c as u32)
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

/// Intersect multiple sorted trigram posting lists via galloping merge.
/// Returns entries appearing in all lists.
pub(crate) fn intersect_trigram_lists(trigrams: &HashMap<u32, Vec<u32>>, keys: &[u32]) -> Vec<u32> {
    if keys.is_empty() {
        return Vec::new();
    }

    let lists: Vec<&[u32]> = keys
        .iter()
        .filter_map(|k| trigrams.get(k).map(|v| v.as_slice()))
        .collect();

    if lists.is_empty() {
        return Vec::new();
    }

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

/// Tiered search index.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub entries: Vec<Entry>,
    pub(crate) by_ext: HashMap<String, Vec<u32>>,
    pub(crate) path_to_id: HashMap<u64, u32>,
    pub(crate) trigrams: HashMap<u32, Vec<u32>>,
    pub(crate) prefix_first_byte: HashMap<u8, Vec<u32>>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, is_dir: bool) -> u32 {
        self.insert_with_metadata(path, is_dir, 0, 0, 0, 0)
    }

    pub fn insert_with_metadata(
        &mut self,
        path: &str,
        is_dir: bool,
        size: u64,
        modified: i64,
        created: i64,
        accessed: i64,
    ) -> u32 {
        let id = self.entries.len() as u32;
        let name_off = path.rfind('/').map(|i| i + 1).unwrap_or(0) as u16;
        let name = &path[name_off as usize..];
        let ext_off = if is_dir {
            0
        } else {
            name.rfind('.')
                .map(|i| name_off as usize + i + 1)
                .unwrap_or(0) as u16
        };

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

        let path_hash = fnv1a_64(path.as_bytes());

        self.entries.push(entry);

        self.path_to_id.insert(path_hash, id);

        if !is_dir {
            let ext = if ext_off > name_off {
                &path[ext_off as usize..]
            } else {
                ""
            };
            self.by_ext.entry(ext.to_string()).or_default().push(id);
        }

        // Insert into trigram and prefix indexes using a temporary lowered copy.
        let name_lower = lowered_bytes(name);
        for trigram in unique_trigrams(&name_lower) {
            self.trigrams.entry(trigram).or_default().push(id);
        }
        if let Some(&first_byte) = name_lower.first() {
            self.prefix_first_byte
                .entry(first_byte)
                .or_default()
                .push(id);
        }

        id
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

        let is_dir = entry.is_dir;
        let name_lower = lowered_bytes(entry.name());
        let ext = if !is_dir {
            entry.extension().to_string()
        } else {
            String::new()
        };

        // Remove from trigram index.
        for trigram in unique_trigrams(&name_lower) {
            if let Some(list) = self.trigrams.get_mut(&trigram) {
                if let Ok(pos) = list.binary_search(&id) {
                    list.remove(pos);
                }
            }
        }

        // Remove from prefix first-byte bucket.
        if let Some(&first_byte) = name_lower.first() {
            if let Some(list) = self.prefix_first_byte.get_mut(&first_byte) {
                if let Ok(pos) = list.binary_search(&id) {
                    list.remove(pos);
                }
            }
        }

        // Remove from by_ext.
        if !is_dir {
            if let Some(list) = self.by_ext.get_mut(&ext) {
                if let Ok(pos) = list.binary_search(&id) {
                    list.remove(pos);
                }
            }
        }

        let old_last_id = self.entries.len() as u32 - 1;
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
            if let Some(&first_byte) = swapped_name_lower.first() {
                if let Some(list) = self.prefix_first_byte.get_mut(&first_byte) {
                    replace_sorted_id(list, old_last_id, id);
                }
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
        let entry = &mut self.entries[id as usize];
        if entry.path != path {
            return false;
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            entry.size = metadata.len();
            if let Ok(t) = metadata.modified() {
                if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                    entry.modified = d.as_secs() as i64;
                }
            }
            if let Ok(t) = metadata.created() {
                if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                    entry.created = d.as_secs() as i64;
                }
            }
            if let Ok(t) = metadata.accessed() {
                if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                    entry.accessed = d.as_secs() as i64;
                }
            }
        }
        true
    }

    pub fn search_substring(&self, text: &str) -> Vec<u32> {
        let needle = text.to_lowercase();
        let needle_bytes = needle.as_bytes();

        if needle_bytes.len() >= 3 {
            let trigrams_keys = extract_trigrams(needle_bytes);
            let candidates = intersect_trigram_lists(&self.trigrams, &trigrams_keys);
            candidates
                .into_iter()
                .filter(|&id| {
                    let entry = &self.entries[id as usize];
                    contains_ignore_case(entry.name(), needle_bytes)
                })
                .collect()
        } else if needle_bytes.is_empty() {
            (0..self.entries.len() as u32).collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| contains_ignore_case(e.name(), needle_bytes))
                .map(|(i, _)| i as u32)
                .collect()
        }
    }

    pub fn search_prefix(&self, prefix: &str) -> Vec<u32> {
        let prefix_lower = prefix.to_lowercase();
        let prefix_bytes = prefix_lower.as_bytes();

        if prefix_bytes.is_empty() {
            return (0..self.entries.len() as u32).collect();
        }

        if let Some(first_byte) = prefix_bytes.first() {
            if let Some(bucket) = self.prefix_first_byte.get(first_byte) {
                return bucket
                    .iter()
                    .filter(|&&id| {
                        let entry = &self.entries[id as usize];
                        starts_with_ignore_case(entry.name(), prefix_bytes)
                    })
                    .copied()
                    .collect();
            }
        }

        Vec::new()
    }

    pub fn get_path(&self, id: u32) -> Option<&str> {
        self.entries.get(id as usize).map(|e| e.path.as_str())
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn metadata_size(&self) -> usize {
        self.entries
            .iter()
            .map(|e| e.path.len() + std::mem::size_of::<Entry>())
            .sum::<usize>()
            + self.by_ext.capacity() * 16
            + self.path_to_id.capacity() * 16
            + self.trigrams.capacity() * 16
            + self.prefix_first_byte.capacity() * 16
    }

    #[allow(dead_code)]
    fn rebuild_maps(&mut self) {
        self.path_to_id.clear();
        self.by_ext.clear();
        self.trigrams.clear();
        self.prefix_first_byte.clear();
        for (id, entry) in self.entries.iter().enumerate() {
            let id = id as u32;
            let path_hash = fnv1a_64(entry.path.as_bytes());
            self.path_to_id.insert(path_hash, id);
            if !entry.is_dir {
                let ext = entry.extension().to_string();
                self.by_ext.entry(ext).or_default().push(id);
            }
            let name_lower = lowered_bytes(entry.name());
            for trigram in unique_trigrams(&name_lower) {
                self.trigrams.entry(trigram).or_default().push(id);
            }
            if let Some(&first_byte) = name_lower.first() {
                self.prefix_first_byte
                    .entry(first_byte)
                    .or_default()
                    .push(id);
            }
        }
    }

    /// Look up entries by extension (used by the matcher).
    pub fn by_extension(&self, ext: &str) -> Option<&[u32]> {
        self.by_ext.get(ext).map(|v| v.as_slice())
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
