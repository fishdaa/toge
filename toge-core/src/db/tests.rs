use super::*;
use std::fs;

fn sample_index() -> Index {
    let mut idx = Index::new();
    idx.insert("/home/alice/docs/foo.txt", false);
    idx.insert("/home/alice/docs/bar.rs", false);
    idx.insert("/home/alice/docs/dir1", true);
    idx.insert("/home/bob/music/song.mp3", false);
    idx
}

#[test]
fn test_save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.bin");

    let original = sample_index();
    original.save(&path).unwrap();
    assert!(path.exists());

    let loaded = Index::load(&path).unwrap();
    assert_eq!(loaded.count(), original.count());

    for id in 0..original.count() as u32 {
        assert_eq!(
            loaded.get_path(id),
            original.get_path(id),
            "path mismatch at id {}",
            id
        );
    }
}

#[test]
fn test_load_missing_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.bin");
    assert!(Index::load(&path).is_err());
}

#[test]
fn test_load_corrupt_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.bin");
    fs::write(&path, b"NOTANINDEX").unwrap();
    assert!(Index::load(&path).is_err());
}

#[test]
fn test_save_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.bin");

    let original = sample_index();
    original.save(&path).unwrap();

    // There should never be a partial .tmp file left behind.
    let mut found_tmp = false;
    for entry in fs::read_dir(dir.path()).unwrap() {
        let name = entry.unwrap().file_name();
        if name.to_string_lossy().contains(".tmp") {
            found_tmp = true;
            break;
        }
    }
    assert!(!found_tmp, "atomic save left a temp file behind");
}

#[test]
fn test_metadata_size_reported_by_saved_index() {
    let idx = sample_index();
    assert!(idx.metadata_size() > 0);
}

#[test]
fn test_load_rejects_huge_path_section_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge-paths.bin");

    let mut data = Vec::new();
    data.extend_from_slice(b"NDL1");
    data.extend_from_slice(&VERSION.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0i64.to_le_bytes());
    data.resize(64, 0);
    data.extend_from_slice(&((MAX_PATH_SECTION_LEN as u64) + 1).to_le_bytes());
    let checksum = crate::index::fnv1a_64(&[&data[..12], &data[20..]].concat());
    data[12..20].copy_from_slice(&checksum.to_le_bytes());

    fs::write(&path, data).unwrap();
    let err = Index::load(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("path section exceeds limit"));
}

#[test]
fn test_save_restricts_index_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.bin");

    sample_index().save(&path).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn test_load_rejects_excessive_entry_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("too-many-entries.bin");

    let mut data = Vec::new();
    data.extend_from_slice(b"NDL1");
    data.extend_from_slice(&VERSION.to_le_bytes());
    data.extend_from_slice(&((MAX_ENTRY_COUNT as u32) + 1).to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0i64.to_le_bytes());
    data.resize(64, 0);
    let checksum = crate::index::fnv1a_64(&[&data[..12], &data[20..]].concat());
    data[12..20].copy_from_slice(&checksum.to_le_bytes());

    fs::write(&path, data).unwrap();
    let err = Index::load(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("entry count exceeds limit"));
}

#[test]
fn test_load_rejects_excessive_ext_key_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ext-key-too-large.bin");

    let mut data = Vec::new();
    data.extend_from_slice(b"NDL1");
    data.extend_from_slice(&VERSION.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0i64.to_le_bytes());
    data.resize(64, 0);
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&((MAX_EXT_KEY_LEN as u32) + 1).to_le_bytes());
    let checksum = crate::index::fnv1a_64(&[&data[..12], &data[20..]].concat());
    data[12..20].copy_from_slice(&checksum.to_le_bytes());

    fs::write(&path, data).unwrap();
    let err = Index::load(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("ext key exceeds limit"));
}

#[test]
fn test_load_rejects_legacy_index_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-version.bin");

    let mut data = Vec::new();
    data.extend_from_slice(b"NDL1");
    data.extend_from_slice(&(VERSION - 1).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0i64.to_le_bytes());
    data.resize(64, 0);
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    let checksum = crate::index::fnv1a_64(&[&data[..12], &data[20..]].concat());
    data[12..20].copy_from_slice(&checksum.to_le_bytes());

    fs::write(&path, data).unwrap();
    let err = Index::load(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("unsupported version"));
}

#[test]
fn streamed_save_keeps_v3_checksum_metadata_and_search_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.bin");
    let mut original = sample_index();
    original.insert_with_metadata("/tmp/日本語.txt", false, 123, 456, 789, 1011);
    let stats = original.save(&path).unwrap();
    let data = fs::read(&path).unwrap();
    assert_eq!(stats.bytes_written, data.len() as u64);
    assert_eq!(stats.entry_count as usize, original.count());
    let legacy_checksum = fnv1a_64(&[&data[..12], &data[20..]].concat());
    assert_eq!(&data[12..20], &legacy_checksum.to_le_bytes());
    let loaded = Index::load(&path).unwrap();
    assert_eq!(loaded.entries, original.entries);
    for term in ["foo", "日本語", "song"] {
        assert_eq!(
            loaded.search_substring(term),
            original.search_substring(term)
        );
    }
    assert_eq!(loaded.search_prefix("bar"), original.search_prefix("bar"));
    assert_eq!(loaded.by_extension("txt"), original.by_extension("txt"));
    let mut corrupt = data;
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&path, corrupt).unwrap();
    assert!(
        Index::load(&path)
            .unwrap_err()
            .to_string()
            .contains("checksum mismatch")
    );
}

#[test]
fn streaming_checksum_handles_partial_writes_across_header_boundaries() {
    struct PartialWriter(Vec<u8>);
    impl Write for PartialWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let len = buf.len().min(7);
            self.0.extend_from_slice(&buf[..len]);
            Ok(len)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let bytes: Vec<u8> = (0..100).collect();
    let mut writer = IndexWriter {
        inner: PartialWriter(Vec::new()),
        checksum: fnv1a_64(&[]),
        bytes_written: 0,
    };
    writer.write_all(&bytes).unwrap();
    assert_eq!(writer.inner.0, bytes);
    assert_eq!(writer.bytes_written, bytes.len() as u64);
    assert_eq!(
        writer.checksum,
        fnv1a_64(&[&bytes[..12], &bytes[20..]].concat())
    );
}

#[test]
fn concurrent_saves_publish_one_complete_index_without_temp_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.bin");
    let writers = 8;
    let barrier = std::sync::Barrier::new(writers);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for writer in 0..writers {
            let (barrier, path) = (&barrier, &path);
            handles.push(scope.spawn(move || {
                let mut index = Index::new();
                for row in 0..2_000 {
                    index.insert(&format!("/writer-{writer}/row-{row:05}.txt"), false);
                }
                barrier.wait();
                index.save(path).unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    });
    let loaded = Index::load(&path).unwrap();
    assert_eq!(loaded.count(), 2_000);
    let prefix = loaded.entries[0].path.split('/').nth(1).unwrap();
    assert!(
        loaded
            .entries
            .iter()
            .all(|entry| entry.path.split('/').nth(1) == Some(prefix))
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
