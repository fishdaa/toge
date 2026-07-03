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

/// Tiered search index.
#[derive(Debug, Default, Clone)]
pub struct Index {
    pub entries: Vec<Entry>,
    pub(crate) by_ext: HashMap<String, Vec<u32>>,
    pub(crate) path_to_id: HashMap<String, u32>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, is_dir: bool) -> u32 {
        let id = self.entries.len() as u32;
        let name_off = path.rfind('/').map(|i| i + 1).unwrap_or(0) as u16;
        let name = &path[name_off as usize..];
        let ext_off = if is_dir {
            0
        } else {
            name
                .rfind('.')
                .map(|i| name_off as usize + i + 1)
                .unwrap_or(0) as u16
        };

        let entry = Entry {
            path: path.to_string(),
            name_off,
            ext_off,
            is_dir,
            size: 0,
            modified: 0,
            created: 0,
            accessed: 0,
        };
        self.entries.push(entry);
        self.path_to_id.insert(path.to_string(), id);

        if !is_dir {
            let ext = if ext_off > name_off {
                &path[ext_off as usize..]
            } else {
                ""
            };
            self.by_ext.entry(ext.to_string()).or_default().push(id);
        }

        id
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let Some(id) = self.path_to_id.remove(path) else {
            return false;
        };

        self.entries.swap_remove(id as usize);
        self.rebuild_maps();
        true
    }

    pub fn update_metadata(&mut self, path: &str) -> bool {
        self.path_to_id.contains_key(path)
    }

    pub fn search_substring(&self, text: &str) -> Vec<u32> {
        let needle = text.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name().to_lowercase().contains(&needle))
            .map(|(i, _)| i as u32)
            .collect()
    }

    pub fn search_prefix(&self, prefix: &str) -> Vec<u32> {
        let prefix_lower = prefix.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name().to_lowercase().starts_with(&prefix_lower))
            .map(|(i, _)| i as u32)
            .collect()
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
    }

    fn rebuild_maps(&mut self) {
        self.path_to_id.clear();
        self.by_ext.clear();
        for (id, entry) in self.entries.iter().enumerate() {
            let id = id as u32;
            self.path_to_id.insert(entry.path.clone(), id);
            if !entry.is_dir {
                let ext = entry.extension().to_string();
                self.by_ext.entry(ext).or_default().push(id);
            }
        }
    }

    /// Look up entries by extension (used by the matcher).
    pub fn by_extension(&self, ext: &str) -> Option<&[u32]> {
        self.by_ext.get(ext).map(|v| v.as_slice())
    }

    /// Look up an entry id by full path.
    pub fn id_by_path(&self, path: &str) -> Option<u32> {
        self.path_to_id.get(path).copied()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod performance_tests;
