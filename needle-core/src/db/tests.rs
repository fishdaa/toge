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
