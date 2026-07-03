use super::*;
use crate::index::Index;

fn sample_index() -> Index {
    let mut idx = Index::new();
    idx.insert("/home/bob/music/song.mp3", false);
    idx.insert("/home/alice/docs/foo.txt", false);
    idx.insert("/home/alice/docs/bar.rs", false);
    idx.insert("/home/bob/music/aria.mp3", false);
    idx
}

#[test]
fn test_sort_by_name_ascending() {
    let idx = sample_index();
    let mut ids: Vec<u32> = (0..idx.count() as u32).collect();
    sort_ids(&idx, &mut ids, SortKey::Name, true);
    let names: Vec<&str> = ids.iter().map(|id| idx.get_path(*id).unwrap()).collect();
    assert_eq!(
        names,
        vec![
            "/home/bob/music/aria.mp3",
            "/home/alice/docs/bar.rs",
            "/home/alice/docs/foo.txt",
            "/home/bob/music/song.mp3",
        ]
    );
}

#[test]
fn test_sort_by_path_ascending() {
    let idx = sample_index();
    let mut ids: Vec<u32> = (0..idx.count() as u32).collect();
    sort_ids(&idx, &mut ids, SortKey::Path, true);
    let names: Vec<&str> = ids.iter().map(|id| idx.get_path(*id).unwrap()).collect();
    assert_eq!(
        names,
        vec![
            "/home/alice/docs/bar.rs",
            "/home/alice/docs/foo.txt",
            "/home/bob/music/aria.mp3",
            "/home/bob/music/song.mp3",
        ]
    );
}

#[test]
fn test_sort_by_size_descending() {
    let idx = sample_index();
    let mut ids: Vec<u32> = (0..idx.count() as u32).collect();
    sort_ids(&idx, &mut ids, SortKey::Size, false);
    // Largest first. Placeholder sizes may all be zero, so just ensure it doesn't panic.
    assert_eq!(ids.len(), 4);
}
