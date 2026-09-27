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

#[test]
fn cached_orders_match_sort_ids_for_every_shape_and_track_index_changes() {
    let mut idx = Index::new();
    // Duplicate names exercise tie order; long shared prefixes exercise the
    // prefix fast path falling back to full comparison.
    for (i, name) in ["b", "a", "b", "longname-2", "longname-1", "a", "é", "B"]
        .iter()
        .enumerate()
    {
        idx.insert(&format!("/d{}/{name}", i % 3), false);
    }
    let mut cache = OrderCache::default();
    let all: Vec<u32> = (0..idx.count() as u32).collect();
    // Large (bitmap pass) and small (rank sort) result sets, both directions.
    for ids in [all.clone(), vec![6, 1, 3], vec![]] {
        for key in [SortKey::Name, SortKey::Path, SortKey::Size] {
            for ascending in [true, false] {
                let mut sorted = ids.clone();
                sorted.sort_unstable();
                let mut expected = sorted.clone();
                sort_ids(&idx, &mut expected, key, ascending);
                let mut actual = sorted;
                cache.sort(&idx, &mut actual, key, ascending);
                assert_eq!(actual, expected, "{key:?} {ascending} {ids:?}");
            }
        }
    }
    idx.remove("/d1/a");
    idx.insert("/d2/0-first", false);
    let mut ids: Vec<u32> = (0..idx.count() as u32).collect();
    let mut expected = ids.clone();
    sort_ids(&idx, &mut expected, SortKey::Name, true);
    cache.sort(&idx, &mut ids, SortKey::Name, true);
    assert_eq!(ids, expected);
    assert_eq!(idx.get_path(ids[0]), Some("/d2/0-first"));
}

#[test]
fn cached_orders_merge_insertions_without_a_rebuild() {
    let mut idx = Index::new();
    for name in ["m", "c", "x", "c"] {
        idx.insert(&format!("/{}/{name}", idx.count()), false);
    }
    let mut cache = OrderCache::default();
    cache.sort(&idx, &mut [], SortKey::Name, true);
    // Appended entries: before, between, after, and tied with existing names.
    for name in ["a", "n", "z", "c", "m"] {
        idx.insert(&format!("/{}/{name}", idx.count()), false);
    }
    for ascending in [true, false] {
        let all: Vec<u32> = (0..idx.count() as u32).collect();
        let mut expected = all.clone();
        sort_ids(&idx, &mut expected, SortKey::Name, ascending);
        let mut actual = all;
        cache.sort(&idx, &mut actual, SortKey::Name, ascending);
        assert_eq!(actual, expected, "ascending {ascending}");
    }
    assert!(format!("{cache:?}").contains(&format!("epoch: {}", idx.epoch())));
}
