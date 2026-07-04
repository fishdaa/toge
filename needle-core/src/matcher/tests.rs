use super::*;
use crate::index::Index;
use crate::query::{Query, RangeFilter, SearchMode, TextTerm};

fn sample_index() -> Index {
    let mut idx = Index::new();
    idx.insert("/home/alice/docs/foo.txt", false);
    idx.insert("/home/alice/docs/bar.rs", false);
    idx.insert("/home/alice/docs/dir1", true);
    idx.insert("/home/bob/music/song.mp3", false);
    idx.insert("/home/bob/music/README", false);
    idx.entries[0].size = 1024; // placeholder metadata for size filter test
    idx
}

fn substring_query(text: &str) -> Query {
    Query {
        raw: text.into(),
        mode: SearchMode::Substring,
        match_case: false,
        match_whole_word: false,
        match_path: false,
        require_file: false,
        require_folder: false,
        whole_filename: false,
        terms: vec![TextTerm::Substring(text.into())],
        ext: None,
        path_filter: None,
        size: None,
        date_modified: None,
        date_created: None,
        date_accessed: None,
        attributes: None,
        offset: 0,
        max_results: usize::MAX,
        sort: crate::query::Sort::NameAsc,
    }
}

#[test]
fn test_substring_match_filename() {
    let idx = sample_index();
    let q = substring_query("foo");
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![0]);
}

#[test]
fn test_substring_case_insensitive() {
    let idx = sample_index();
    let mut q = substring_query("FOO");
    q.match_case = false;
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![0]);
}

#[test]
fn test_match_path_includes_parent_directories() {
    let idx = sample_index();
    let mut q = substring_query("docs");
    q.match_path = true;
    let mut ids = match_query(&idx, &q);
    ids.sort();
    assert_eq!(ids, vec![0, 1, 2]);
}

#[test]
fn test_wildcard_whole_filename() {
    let idx = sample_index();
    let mut q = substring_query("*.mp3");
    q.mode = SearchMode::Wildcard;
    q.whole_filename = true;
    q.terms = vec![TextTerm::Wildcard("*.mp3".into())];
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![3]);
}

#[test]
fn test_ext_filter() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.ext = Some(vec!["rs".into()]);
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![1]);
}

#[test]
fn test_file_modifier_skips_dirs() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.require_file = true;
    let mut ids = match_query(&idx, &q);
    ids.sort();
    // dir1 excluded.
    assert_eq!(ids, vec![0, 1, 3, 4]);
}

#[test]
fn test_folder_modifier_includes_only_dirs() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.require_folder = true;
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![2]);
}

#[test]
fn test_size_filter() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.size = Some(RangeFilter {
        min: Some(500),
        max: Some(1500),
    });
    let ids = match_query(&idx, &q);
    // foo.txt is 1024 bytes (placeholder metadata).
    assert_eq!(ids, vec![0]);
}

#[test]
fn test_not_term() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.terms = vec![TextTerm::Not(Box::new(TextTerm::Substring("foo".into())))];
    let mut ids = match_query(&idx, &q);
    ids.sort();
    assert_eq!(ids, vec![1, 2, 3, 4]);
}

#[test]
fn test_or_term_matches_either_side() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.terms = vec![TextTerm::Or(vec![
        TextTerm::Substring("foo".into()),
        TextTerm::Substring("bar".into()),
    ])];
    let mut ids = match_query(&idx, &q);
    ids.sort();
    assert_eq!(ids, vec![0, 1]);
}

#[test]
fn test_whole_word_substring_does_not_match_partial_word() {
    let idx = sample_index();
    let mut q = substring_query("read");
    q.match_whole_word = true;
    let ids = match_query(&idx, &q);
    assert!(ids.is_empty());

    q.terms = vec![TextTerm::Substring("README".into())];
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![4]);
}

#[test]
fn test_whole_word_regex_requires_full_word_match() {
    let idx = sample_index();
    let mut q = substring_query("");
    q.mode = SearchMode::Regex;
    q.match_whole_word = true;
    q.terms = vec![TextTerm::Regex("read".into())];
    let ids = match_query(&idx, &q);
    assert!(ids.is_empty());

    q.terms = vec![TextTerm::Regex("readme".into())];
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![4]);
}

#[test]
fn test_matcher_case_insensitive_unicode_substring() {
    let mut idx = Index::new();
    idx.insert("/tmp/Äpfel.txt", false);
    let q = substring_query("äpf");
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![0]);
}

#[test]
fn test_matcher_whole_word_unicode_substring() {
    let mut idx = Index::new();
    idx.insert("/tmp/straße.rs", false);
    let mut q = substring_query("straße");
    q.match_whole_word = true;
    let ids = match_query(&idx, &q);
    assert_eq!(ids, vec![0]);
}
