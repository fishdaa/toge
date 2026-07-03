//! Performance benchmarks for needle-core.
//! Run with: cargo run --release --example bench

use needle_core::index::Index;
use std::fs;
use std::time::Instant;

fn temp_dir() -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    let id: u128 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    dir.push(format!("needle-bench-{}", id));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn main() {
    let sizes = [10_000, 100_000, 500_000];

    println!("{:=^60}", " NEEDLE BENCHMARKS ");
    println!();

    for &n in &sizes {
        bench_insert(n);
        bench_search(n);
    }

    bench_save_load(100_000);
    bench_walk_synthetic();
}

fn bench_insert(n: usize) {
    let start = Instant::now();
    let mut idx = Index::new();
    for i in 0..n {
        let path = format!("/home/user/docs/sub/deep/folder/file_{:08}.txt", i);
        idx.insert(&path, false);
    }
    let elapsed = start.elapsed();
    let rate = n as f64 / elapsed.as_secs_f64();
    println!(
        "insert {:>6} entries: {:>8.1} ms  ({:>10.0} entries/s)",
        n,
        elapsed.as_secs_f64() * 1000.0,
        rate
    );
}

fn bench_search(n: usize) {
    let mut idx = Index::new();
    for i in 0..n {
        let path = format!("/home/user/docs/sub/deep/folder/file_{:08}.txt", i);
        idx.insert(&path, false);
    }

    let start = Instant::now();
    let results = idx.search_substring("zzzzzzzz");
    let elapsed = start.elapsed();
    println!(
        "search {:>6} entries: {:>8.1} ms  ({:>3} hits)",
        n,
        elapsed.as_secs_f64() * 1000.0,
        results.len()
    );

    let start = Instant::now();
    let results = idx.search_prefix("file_000");
    let elapsed = start.elapsed();
    println!(
        "prefix {:>6} entries: {:>8.1} ms  ({:>3} hits)",
        n,
        elapsed.as_secs_f64() * 1000.0,
        results.len()
    );
}

fn bench_save_load(n: usize) {
    let mut idx = Index::new();
    for i in 0..n {
        let path = format!("/home/user/docs/sub/deep/folder/file_{:08}.txt", i);
        idx.insert(&path, false);
    }

    let dir = temp_dir();
    let path = dir.join("bench.bin");

    let start = Instant::now();
    idx.save(&path).unwrap();
    let save_ms = start.elapsed().as_secs_f64() * 1000.0;

    let start = Instant::now();
    let loaded = Index::load(&path).unwrap();
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;

    let size = fs::metadata(&path).unwrap().len();
    println!();
    println!("persistence {:>6} entries:", n);
    println!(
        "  save: {:>10.1} ms  ({:.1} MB)",
        save_ms,
        size as f64 / 1_000_000.0
    );
    println!("  load: {:>10.1} ms", load_ms);
    assert_eq!(loaded.count(), n);

    fs::remove_dir_all(&dir).ok();
}

fn bench_walk_synthetic() {
    let dir = temp_dir();
    let root = dir.join("data");
    fs::create_dir(&root).unwrap();

    let levels = 3;
    let per_level = 20;
    let mut expected = 0;
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        let depth = d.strip_prefix(&root).unwrap().components().count();
        if depth >= levels {
            for i in 0..per_level {
                fs::write(d.join(format!("file_{:03}.txt", i)), "data").unwrap();
                expected += 1;
            }
        } else {
            for i in 0..per_level {
                let sub = d.join(format!("dir_{:03}", i));
                fs::create_dir(&sub).unwrap();
                stack.push(sub);
                expected += 1;
            }
        }
        for i in 0..per_level / 2 {
            fs::write(d.join(format!("file_{:03}.dat", i)), "data").unwrap();
            expected += 1;
        }
    }

    let start = Instant::now();
    let mut idx = Index::new();
    let excludes = needle_core::walker::Excludes::new();
    let count = needle_core::walker::walk(&root, &mut idx, &excludes);
    let elapsed = start.elapsed();

    println!();
    println!(
        "walk  {} entries: {:>10.1} ms  ({:>8} dirs/files)",
        fmt_count(expected),
        elapsed.as_secs_f64() * 1000.0,
        count
    );

    fs::remove_dir_all(&dir).ok();
}

fn fmt_count(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
