use super::*;

fn sample_index() -> Index {
    let mut idx = Index::new();
    idx.insert("/home/alice/docs/foo.txt", false);
    idx.insert("/home/alice/docs/bar.rs", false);
    idx.insert("/home/alice/docs/dir1", true);
    idx.insert("/home/bob/music/song.mp3", false);
    idx.insert("/home/bob/music/README", false);
    idx
}

#[test]
fn test_insert_assigns_sequential_ids() {
    let mut idx = Index::new();
    assert_eq!(idx.insert("/a.txt", false), 0);
    assert_eq!(idx.insert("/b.rs", false), 1);
    assert_eq!(idx.insert("/dir", true), 2);
}

#[test]
fn test_count() {
    let idx = sample_index();
    assert_eq!(idx.count(), 5);
}

#[test]
fn test_get_path() {
    let idx = sample_index();
    assert_eq!(idx.get_path(0), Some("/home/alice/docs/foo.txt"));
    assert_eq!(idx.get_path(4), Some("/home/bob/music/README"));
    assert_eq!(idx.get_path(99), None);
}

#[test]
fn test_search_substring_case_insensitive() {
    let idx = sample_index();
    let mut ids = idx.search_substring("foo");
    ids.sort();
    assert_eq!(ids, vec![0]);

    let mut ids = idx.search_substring("TXT");
    ids.sort();
    assert_eq!(ids, vec![0]);
}

#[test]
fn test_search_substring_matches_filename_only_by_default() {
    let idx = sample_index();
    // "docs" appears in parent path but not in filename.
    let ids = idx.search_substring("docs");
    assert!(ids.is_empty(), "default search should match filename only");
}

#[test]
fn test_search_substring_multiple_matches() {
    let idx = sample_index();
    let mut ids = idx.search_substring("o");
    ids.sort();
    // foo.txt and song.mp3 contain 'o' in their filenames; README does not.
    assert_eq!(ids, vec![0, 3]);
}

#[test]
fn test_search_prefix() {
    let idx = sample_index();
    let mut ids = idx.search_prefix("foo");
    ids.sort();
    assert_eq!(ids, vec![0]);

    let mut ids = idx.search_prefix("bar");
    ids.sort();
    assert_eq!(ids, vec![1]);
}

#[test]
fn test_search_prefix_empty_matches_all_entries() {
    let idx = sample_index();
    let mut ids = idx.search_prefix("");
    ids.sort();
    assert_eq!(ids, vec![0, 1, 2, 3, 4]);
}

#[test]
fn test_search_substring_dedups_repeated_trigram_matches() {
    let mut idx = Index::new();
    idx.insert("/tmp/aaaa.txt", false);
    assert_eq!(idx.search_substring("aaa"), vec![0]);
}

#[test]
fn test_search_substring_case_insensitive_unicode() {
    let mut idx = Index::new();
    idx.insert("/tmp/Äpfel.txt", false);
    assert_eq!(idx.search_substring("äpf"), vec![0]);
}

#[test]
fn test_remove_entry() {
    let mut idx = sample_index();
    assert!(idx.remove("/home/alice/docs/foo.txt"));
    assert_eq!(idx.count(), 4);
    assert!(idx.search_substring("foo").is_empty());
    assert!(!idx.remove("/does/not/exist"));
}

#[test]
fn test_remove_swapped_entry_remains_searchable() {
    let mut idx = Index::new();
    idx.insert("/tmp/alpha.log", false);
    idx.insert("/tmp/bravo.log", false);
    idx.insert("/tmp/charlie.log", false);

    assert!(idx.remove("/tmp/bravo.log"));

    let ids = idx.search_substring("charlie");
    assert_eq!(ids.len(), 1);
    assert_eq!(idx.get_path(ids[0]), Some("/tmp/charlie.log"));
}

#[test]
fn test_remove_swapped_entry_can_be_removed_after_prior_delete() {
    let mut idx = Index::new();
    idx.insert("/tmp/alpha.log", false);
    idx.insert("/tmp/cobra-a.log", false);
    idx.insert("/tmp/cobra-b.log", false);
    idx.insert("/tmp/cobra-c.log", false);
    idx.insert("/tmp/cobra-d.log", false);

    assert!(idx.remove("/tmp/cobra-a.log"));

    let trigram = pack_trigram(b'o', b'b', b'r');
    let list = idx
        .trigrams
        .get(&trigram)
        .expect("cobra trigram bucket exists");
    assert!(
        list.windows(2).all(|w| w[0] < w[1]),
        "trigram posting list must remain sorted after swap_remove, got {:?}",
        list
    );
    assert!(
        idx.remove("/tmp/cobra-d.log"),
        "swapped entry should still be removable"
    );
    assert!(idx.search_substring("cobra-d").is_empty());
}

#[test]
fn test_update_metadata_no_panic() {
    let mut idx = sample_index();
    assert!(idx.update_metadata("/home/alice/docs/foo.txt"));
    assert!(!idx.update_metadata("/does/not/exist"));
}

#[test]
fn test_metadata_size_is_non_zero_with_entries() {
    let idx = sample_index();
    assert!(idx.metadata_size() > 0);
}

#[test]
fn test_entry_name_and_extension_offsets() {
    let mut idx = Index::new();
    idx.insert("/home/user/archive.tar.gz", false);
    let entry = &idx.entries[0];
    assert_eq!(entry.path, "/home/user/archive.tar.gz");
    assert_eq!(entry.name_off, 11);
    // Extension offset points at "gz" (last dot + 1) -> byte 23.
    assert_eq!(entry.ext_off, 23);
    assert!(!entry.is_dir);
}

#[test]
fn test_insert_directory_sets_is_dir() {
    let mut idx = Index::new();
    idx.insert("/home/user/projects", true);
    assert!(idx.entries[0].is_dir);
}
