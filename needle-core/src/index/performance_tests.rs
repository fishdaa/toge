//! Phase 10 performance regression tests.
//! Run with `cargo test -- --ignored` to include these.

use super::*;
use std::time::Instant;

#[test]
#[ignore = "performance regression test"]
fn test_insert_one_million_entries_under_two_seconds() {
    let mut idx = Index::new();
    let start = Instant::now();
    for i in 0..1_000_000 {
        let path = format!("/home/user/files/file_{:07}.txt", i);
        idx.insert(&path, false);
    }
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 2.0, "insert took {:.3}s", elapsed.as_secs_f64());
    assert_eq!(idx.count(), 1_000_000);
}

#[test]
#[ignore = "performance regression test"]
fn test_substring_search_one_million_entries_under_ten_ms() {
    let mut idx = Index::new();
    for i in 0..1_000_000 {
        let path = format!("/home/user/files/file_{:07}.txt", i);
        idx.insert(&path, false);
    }

    let start = Instant::now();
    let results = idx.search_substring("1234567");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs_f64() * 1000.0 < 10.0,
        "search took {:.3}ms",
        elapsed.as_secs_f64() * 1000.0
    );
    assert_eq!(results.len(), 1);
}

#[test]
#[ignore = "performance regression test"]
fn test_save_load_one_million_entries_under_one_second() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("perf.bin");

    let mut idx = Index::new();
    for i in 0..1_000_000 {
        let path_str = format!("/home/user/files/file_{:07}.txt", i);
        idx.insert(&path_str, false);
    }

    let start = Instant::now();
    idx.save(&path).unwrap();
    let saved = start.elapsed();

    let start = Instant::now();
    let loaded = Index::load(&path).unwrap();
    let loaded_time = start.elapsed();

    assert!(saved.as_secs_f64() < 1.0, "save took {:.3}s", saved.as_secs_f64());
    assert!(loaded_time.as_secs_f64() < 1.0, "load took {:.3}s", loaded_time.as_secs_f64());
    assert_eq!(loaded.count(), 1_000_000);
}
