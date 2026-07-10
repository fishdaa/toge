//! Index persistence: save/load binary format.

use crate::index::{Entry, Index, fnv1a_64, lowered_bytes, unique_trigrams};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const MAGIC: &[u8] = b"NDL1";
const VERSION: u32 = 3;
const MAX_INDEX_FILE_SIZE: usize = 1024 * 1024 * 1024;
const MAX_PATH_SECTION_LEN: usize = 512 * 1024 * 1024;
const MAX_ENTRY_COUNT: usize = 10_000_000;
const MAX_EXT_KEY_LEN: usize = 1024;
const MAX_EXT_VALUE_COUNT: usize = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStats {
    pub entry_count: u32,
    pub bytes_written: u64,
}

impl Index {
    pub fn save(&self, path: &Path) -> io::Result<SaveStats> {
        let tmp_path = path.with_extension("bin.tmp");
        let mut data = Vec::new();

        // Header placeholder.
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&VERSION.to_le_bytes());
        let entry_count = self.entries.len() as u32;
        data.extend_from_slice(&entry_count.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes()); // checksum placeholder
        data.extend_from_slice(&0u32.to_le_bytes()); // tier flags
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        data.extend_from_slice(&timestamp.to_le_bytes());
        data.resize(64, 0); // pad header to 64 bytes

        // Section 1: paths (null-separated).
        let mut path_buf = Vec::new();
        for entry in &self.entries {
            path_buf.extend_from_slice(entry.path.as_bytes());
            path_buf.push(0);
        }
        data.extend_from_slice(&(path_buf.len() as u64).to_le_bytes());
        data.extend_from_slice(&path_buf);

        // Section 2: metadata.
        for entry in &self.entries {
            data.extend_from_slice(&entry.name_off.to_le_bytes());
            data.extend_from_slice(&entry.ext_off.to_le_bytes());
            data.push(if entry.is_dir { 1 } else { 0 });
        }
        // Section 2b: optional metadata fields (size, modified, created, accessed).
        for entry in &self.entries {
            data.extend_from_slice(&entry.size.to_le_bytes());
            data.extend_from_slice(&entry.modified.to_le_bytes());
            data.extend_from_slice(&entry.created.to_le_bytes());
            data.extend_from_slice(&entry.accessed.to_le_bytes());
        }

        // Section 3: by_ext map.
        let mut ext_entries: Vec<_> = self.by_ext.iter().collect();
        ext_entries.sort_by_key(|(k, _)| *k);
        data.extend_from_slice(&(ext_entries.len() as u32).to_le_bytes());
        for (ext, ids) in ext_entries {
            data.extend_from_slice(&(ext.len() as u32).to_le_bytes());
            data.extend_from_slice(ext.as_bytes());
            data.extend_from_slice(&(ids.len() as u32).to_le_bytes());
            for id in ids {
                data.extend_from_slice(&id.to_le_bytes());
            }
        }

        // Compute checksum over everything except the checksum field itself (bytes 12-19).
        let checksum = fnv1a_64(&[&data[..12], &data[20..]].concat());
        data[12..20].copy_from_slice(&checksum.to_le_bytes());

        let mut file = fs::File::create(&tmp_path)?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(&data)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&tmp_path, path)?;

        Ok(SaveStats {
            entry_count,
            bytes_written: data.len() as u64,
        })
    }

    pub fn load(path: &Path) -> io::Result<Index> {
        let mut file = fs::File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        if data.len() > MAX_INDEX_FILE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "index file too large",
            ));
        }

        if data.len() < 64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "file too short"));
        }
        if &data[0..4] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic"));
        }
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported version",
            ));
        }
        let entry_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        if entry_count > MAX_ENTRY_COUNT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry count exceeds limit",
            ));
        }
        let stored_checksum = u64::from_le_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ]);
        let computed_checksum = fnv1a_64(&[&data[..12], &data[20..]].concat());
        if stored_checksum != computed_checksum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "checksum mismatch",
            ));
        }

        let mut offset = 64;

        // Section 1: paths.
        if offset + 8 > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated path section",
            ));
        }
        let path_section_len = u64::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;
        offset += 8;
        if path_section_len > MAX_PATH_SECTION_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "path section exceeds limit",
            ));
        }
        if offset + path_section_len > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "path section overrun",
            ));
        }
        let path_section = &data[offset..offset + path_section_len];
        let paths: Vec<&str> = path_section
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| {
                std::str::from_utf8(s)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid utf8 path"))
            })
            .collect::<Result<_, _>>()?;
        offset += path_section_len;

        if paths.len() != entry_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry count mismatch",
            ));
        }

        // Section 2: metadata.
        let metadata_size = entry_count
            .checked_mul(5)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "metadata overflow"))?;
        if offset + metadata_size > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated metadata section",
            ));
        }
        let mut entries = Vec::with_capacity(entry_count);
        for path in paths {
            let meta_off = offset;
            offset += 5;
            let name_off = u16::from_le_bytes([data[meta_off], data[meta_off + 1]]);
            let ext_off = u16::from_le_bytes([data[meta_off + 2], data[meta_off + 3]]);
            let is_dir = data[meta_off + 4] != 0;
            entries.push(Entry {
                path: path.to_string(),
                name_off,
                ext_off,
                is_dir,
                size: 0,
                modified: 0,
                created: 0,
                accessed: 0,
            });
        }

        // Section 2b: optional metadata fields (size, modified, created, accessed).
        let meta2_size = entry_count.checked_mul(32).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "metadata section overflow")
        })?;
        if offset + meta2_size <= data.len() {
            for (i, entry) in entries.iter_mut().enumerate() {
                let moff = offset + i * 32;
                entry.size = u64::from_le_bytes([
                    data[moff],
                    data[moff + 1],
                    data[moff + 2],
                    data[moff + 3],
                    data[moff + 4],
                    data[moff + 5],
                    data[moff + 6],
                    data[moff + 7],
                ]);
                entry.modified = i64::from_le_bytes([
                    data[moff + 8],
                    data[moff + 9],
                    data[moff + 10],
                    data[moff + 11],
                    data[moff + 12],
                    data[moff + 13],
                    data[moff + 14],
                    data[moff + 15],
                ]);
                entry.created = i64::from_le_bytes([
                    data[moff + 16],
                    data[moff + 17],
                    data[moff + 18],
                    data[moff + 19],
                    data[moff + 20],
                    data[moff + 21],
                    data[moff + 22],
                    data[moff + 23],
                ]);
                entry.accessed = i64::from_le_bytes([
                    data[moff + 24],
                    data[moff + 25],
                    data[moff + 26],
                    data[moff + 27],
                    data[moff + 28],
                    data[moff + 29],
                    data[moff + 30],
                    data[moff + 31],
                ]);
            }
            offset += meta2_size;
        }

        // Section 3: by_ext map.
        if offset + 4 > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated ext section",
            ));
        }
        let ext_count = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        let mut by_ext = HashMap::with_capacity(ext_count);
        for _ in 0..ext_count {
            if offset + 4 > data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "truncated ext key",
                ));
            }
            let key_len = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;
            if key_len > MAX_EXT_KEY_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ext key exceeds limit",
                ));
            }
            if offset + key_len > data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ext key overrun",
                ));
            }
            let key = std::str::from_utf8(&data[offset..offset + key_len])
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid utf8 ext"))?
                .to_string();
            offset += key_len;

            if offset + 4 > data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "truncated ext values",
                ));
            }
            let value_count = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;
            if value_count > MAX_EXT_VALUE_COUNT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ext value count exceeds limit",
                ));
            }
            let values_size = value_count
                .checked_mul(4)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ext values overflow"))?;
            if offset + values_size > data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ext values overrun",
                ));
            }
            let mut ids = Vec::with_capacity(value_count);
            for j in 0..value_count {
                let id_off = offset + j * 4;
                ids.push(u32::from_le_bytes([
                    data[id_off],
                    data[id_off + 1],
                    data[id_off + 2],
                    data[id_off + 3],
                ]));
            }
            offset += values_size;
            by_ext.insert(key, ids);
        }

        let mut path_to_id = HashMap::with_capacity(entry_count);
        for (id, entry) in entries.iter().enumerate() {
            let path_hash = fnv1a_64(entry.path.as_bytes());
            path_to_id.insert(path_hash, id as u32);
        }

        // Rebuild trigram and prefix indexes from loaded entries.
        let mut trigrams = HashMap::new();
        let mut prefix_first_byte = HashMap::new();
        for (id, entry) in entries.iter().enumerate() {
            let id = id as u32;
            let name_lower = lowered_bytes(entry.name());
            for trigram in unique_trigrams(&name_lower) {
                trigrams.entry(trigram).or_insert_with(Vec::new).push(id);
            }
            if let Some(&first_byte) = name_lower.first() {
                prefix_first_byte
                    .entry(first_byte)
                    .or_insert_with(Vec::new)
                    .push(id);
            }
        }

        Ok(Index {
            entries,
            by_ext,
            path_to_id,
            trigrams,
            prefix_first_byte,
        })
    }
}

#[cfg(test)]
mod tests;
