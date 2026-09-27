//! Index persistence: save/load binary format.

use crate::index::{
    Entry, Index, entry_id, fnv1a_64, fnv1a_extend, lowered_bytes, push_index_value,
    unique_trigrams,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

// Hash bytes as they are written, excluding the checksum field at offsets 12..20.
// The serialized index never needs to exist as a second in-memory copy.
struct IndexWriter<W> {
    inner: W,
    checksum: u64,
    bytes_written: u64,
}

impl<W: Write> Write for IndexWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        let start = self.bytes_written;
        let end = start + written as u64;
        if start < 12 {
            // `end.min(12) - start` is bounded to 0..=12, so it always fits in a usize.
            let split = usize::try_from(end.min(12) - start).unwrap_or(usize::MAX);
            self.checksum = fnv1a_extend(self.checksum, &buf[..split]);
        }
        if end > 20 {
            // `20 - start` is bounded to 0..=20 here, so it always fits in a usize.
            let skip = usize::try_from(20u64.saturating_sub(start)).unwrap_or(usize::MAX);
            self.checksum = fnv1a_extend(self.checksum, &buf[skip..written]);
        }
        self.bytes_written = end;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// Each concurrent save owns a different sibling file. A failed save removes
// only its own temporary file; rename still publishes one complete index.
struct SaveTemp(PathBuf);

impl Drop for SaveTemp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn create_save_temp(path: &Path) -> io::Result<(SaveTemp, fs::File)> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing index filename"))?;
    loop {
        let mut name = filename.to_os_string();
        name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = path.with_file_name(name);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temporary) {
            Ok(file) => return Ok((SaveTemp(temporary), file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

impl Index {
    pub fn save(&self, path: &Path) -> io::Result<SaveStats> {
        let mut header = Vec::new();

        // Header placeholder.
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&VERSION.to_le_bytes());
        let entry_count = u32::try_from(self.entries.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "too many entries to serialize")
        })?;
        header.extend_from_slice(&entry_count.to_le_bytes());
        header.extend_from_slice(&0u64.to_le_bytes()); // checksum placeholder
        header.extend_from_slice(&0u32.to_le_bytes()); // tier flags
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .cast_signed();
        header.extend_from_slice(&timestamp.to_le_bytes());
        header.resize(64, 0); // pad header to 64 bytes

        let (temporary, file) = create_save_temp(path)?;
        let mut data = IndexWriter {
            inner: BufWriter::new(file),
            checksum: fnv1a_64(&[]),
            bytes_written: 0,
        };
        data.write_all(&header)?;

        // Section 1: paths (null-separated).
        let path_len: u64 = self
            .entries
            .iter()
            .map(|entry| entry.path.len() as u64 + 1)
            .sum();
        data.write_all(&path_len.to_le_bytes())?;
        for entry in &self.entries {
            data.write_all(entry.path.as_bytes())?;
            data.write_all(&[0])?;
        }

        // Section 2: metadata.
        for entry in &self.entries {
            data.write_all(&entry.name_off.to_le_bytes())?;
            data.write_all(&entry.ext_off.to_le_bytes())?;
            data.write_all(&[u8::from(entry.is_dir)])?;
        }
        // Section 2b: optional metadata fields (size, modified, created, accessed).
        for entry in &self.entries {
            data.write_all(&entry.size.to_le_bytes())?;
            data.write_all(&entry.modified.to_le_bytes())?;
            data.write_all(&entry.created.to_le_bytes())?;
            data.write_all(&entry.accessed.to_le_bytes())?;
        }

        // Section 3: by_ext map.
        let mut ext_entries: Vec<_> = self.by_ext.iter().collect();
        ext_entries.sort_by_key(|(k, _)| *k);
        let overflow_err =
            || io::Error::new(io::ErrorKind::InvalidData, "section too large to serialize");
        let ext_entries_count = u32::try_from(ext_entries.len()).map_err(|_| overflow_err())?;
        data.write_all(&ext_entries_count.to_le_bytes())?;
        for (ext, ids) in ext_entries {
            let ext_len = u32::try_from(ext.len()).map_err(|_| overflow_err())?;
            data.write_all(&ext_len.to_le_bytes())?;
            data.write_all(ext.as_bytes())?;
            let ids_count = u32::try_from(ids.len()).map_err(|_| overflow_err())?;
            data.write_all(&ids_count.to_le_bytes())?;
            for id in ids {
                data.write_all(&id.to_le_bytes())?;
            }
        }

        let bytes_written = data.bytes_written;
        let checksum = data.checksum;
        data.flush()?;
        let mut file = data
            .inner
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        file.seek(SeekFrom::Start(12))?;
        file.write_all(&checksum.to_le_bytes())?;
        file.sync_all()?;
        drop(file);

        fs::rename(&temporary.0, path)?;

        Ok(SaveStats {
            entry_count,
            bytes_written,
        })
    }

    pub fn load(path: &Path) -> io::Result<Index> {
        let file = fs::File::open(path)?;
        let mut data = Vec::new();
        file.take(MAX_INDEX_FILE_SIZE as u64 + 1)
            .read_to_end(&mut data)?;
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
        let computed_checksum = fnv1a_extend(fnv1a_64(&data[..12]), &data[20..]);
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
        let path_section_len_u64 = u64::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        offset += 8;
        // Reject values that don't fit in a usize rather than silently truncating them on
        // 32-bit targets, which could otherwise bypass the length check below.
        let path_section_len = usize::try_from(path_section_len_u64).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "path section exceeds limit")
        })?;
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
        let mut paths = path_section
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| {
                std::str::from_utf8(s)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid utf8 path"))
            });
        offset += path_section_len;

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
        for _ in 0..entry_count {
            let path = paths.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "entry count mismatch")
            })??;
            let meta_off = offset;
            offset += 5;
            let name_off = u16::from_le_bytes([data[meta_off], data[meta_off + 1]]);
            let ext_off = u16::from_le_bytes([data[meta_off + 2], data[meta_off + 3]]);
            let is_dir = data[meta_off + 4] != 0;
            // `Entry::name` and `Entry::extension` slice `path` at these offsets.
            let ext_start = usize::from(ext_off);
            if !path.is_char_boundary(usize::from(name_off))
                || (ext_off > name_off && !path.is_char_boundary(ext_start))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "entry offset outside its path",
                ));
            }
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

        if paths.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry count mismatch",
            ));
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

        // Release the serialized bytes before allocating the search indexes.
        drop(data);

        let mut path_to_id = HashMap::with_capacity(entry_count);
        for (id, entry) in entries.iter().enumerate() {
            let path_hash = fnv1a_64(entry.path.as_bytes());
            path_to_id.insert(path_hash, entry_id(id));
        }

        // Rebuild trigram and prefix indexes from loaded entries.
        let mut trigrams = HashMap::new();
        let mut prefix_first_byte = HashMap::new();
        for (id, entry) in entries.iter().enumerate() {
            let id = entry_id(id);
            let name_lower = lowered_bytes(entry.name());
            for trigram in unique_trigrams(&name_lower) {
                push_index_value(trigrams.entry(trigram).or_insert_with(Vec::new), id);
            }
            if let Some(&first_byte) = name_lower.first() {
                push_index_value(
                    prefix_first_byte.entry(first_byte).or_insert_with(Vec::new),
                    id,
                );
            }
        }

        let mut index = Index {
            entries,
            by_ext,
            path_to_id,
            trigrams,
            prefix_first_byte,
            ..Index::default()
        };
        index.compact();
        Ok(index)
    }
}

#[cfg(test)]
mod tests;
