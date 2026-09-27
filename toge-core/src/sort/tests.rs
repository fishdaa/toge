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

#[test]
fn cached_orders_replay_removals_and_renumbered_ties() {
    let mut idx = Index::new();
    let names = ["dup", "a", "dup", "z", "m", "dup", "b"];
    for i in 0..40 {
        idx.insert(&format!("/{i}/{}", names[i % names.len()]), false);
    }
    let mut cache = OrderCache::default();
    let check = |idx: &Index, cache: &mut OrderCache| {
        for key in [SortKey::Name, SortKey::Path] {
            for ascending in [true, false] {
                let all: Vec<u32> = (0..idx.count() as u32).collect();
                let mut expected = all.clone();
                sort_ids(idx, &mut expected, key, ascending);
                let mut actual = all;
                cache.sort(idx, &mut actual, key, ascending);
                assert_eq!(actual, expected, "{key:?} {ascending}");
            }
        }
    };
    check(&idx, &mut cache);
    // Deterministic mix: removals move the last ID into low slots (including
    // entries appended after the cache was built), interleaved with inserts.
    let mut seed = 7u32;
    for step in 0..60 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let id = (seed >> 8) % idx.count() as u32;
        let path = idx.get_path(id).unwrap().to_string();
        idx.remove(&path);
        if step % 3 == 0 {
            idx.insert(&format!("/new{step}/{}", names[step % names.len()]), false);
        }
        if step % 4 == 0 {
            check(&idx, &mut cache);
        }
    }
    assert!(idx.removals_since(0).is_some());
    check(&idx, &mut cache);

    // A replaced index cannot be replayed; the cache rebuilds.
    let mut replacement = Index::new();
    replacement.insert("/only/one", false);
    replacement.succeed(&idx);
    assert!(replacement.removals_since(idx.epoch()).is_none());
    check(&replacement, &mut cache);
}

#[test]
fn small_match_sets_skip_a_stale_cache_but_sort_identically() {
    let mut idx = Index::new();
    for i in 0..128 {
        idx.insert(&format!("/d/{}.txt", 127 - i), false);
    }
    let mut cache = OrderCache::default();
    let mut all: Vec<u32> = (0..idx.count() as u32).collect();
    cache.sort(&idx, &mut all, SortKey::Name, true);
    let built_at = cache.name.as_ref().unwrap().epoch;

    idx.remove("/d/5.txt");
    for ascending in [true, false] {
        let mut small: Vec<u32> = vec![3, 40, 90];
        let mut expected = small.clone();
        sort_ids(&idx, &mut expected, SortKey::Name, ascending);
        cache.sort(&idx, &mut small, SortKey::Name, ascending);
        assert_eq!(small, expected);
    }
    assert_eq!(cache.name.as_ref().unwrap().epoch, built_at);

    // A large set still brings the cache up to date.
    let mut all: Vec<u32> = (0..idx.count() as u32).collect();
    let mut expected = all.clone();
    sort_ids(&idx, &mut expected, SortKey::Name, true);
    cache.sort(&idx, &mut all, SortKey::Name, true);
    assert_eq!(all, expected);
    assert_eq!(cache.name.as_ref().unwrap().epoch, idx.epoch());
}
