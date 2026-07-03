//! Core in-memory index.

/// An entry in the search index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub name_off: u16,
    pub ext_off: u16,
    pub is_dir: bool,
}

/// Tiered search index.
#[derive(Debug, Default, Clone)]
pub struct Index {
    pub entries: Vec<Entry>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, is_dir: bool) -> u32 {
        let _ = path;
        let _ = is_dir;
        todo!()
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let _ = path;
        todo!()
    }

    pub fn update_metadata(&mut self, path: &str) -> bool {
        let _ = path;
        todo!()
    }

    pub fn search_substring(&self, text: &str) -> Vec<u32> {
        let _ = text;
        todo!()
    }

    pub fn search_prefix(&self, prefix: &str) -> Vec<u32> {
        let _ = prefix;
        todo!()
    }

    pub fn get_path(&self, id: u32) -> Option<&str> {
        let _ = id;
        todo!()
    }

    pub fn count(&self) -> usize {
        todo!()
    }

    pub fn metadata_size(&self) -> usize {
        todo!()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod performance_tests;
