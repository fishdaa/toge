//! Profiling driver for toge-core hot paths.
//!
//! Run with:
//!   cargo run --release --example profile -- insert
//!   cargo run --release --example profile -- substring-hit 500000 30
//!
//! Record with an external profiler:
//!   perf record --call-graph dwarf cargo run --release --example profile -- insert
//!   perf record --call-graph dwarf cargo run --release --example profile -- substring-miss
//!   samply record cargo run --release --example profile -- walk
//!   heaptrack cargo run --release --example profile -- insert

use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use toge_core::index::Index;
use toge_core::walker::{self, Excludes};

const DEFAULT_SIZE: usize = 500_000;
const DEFAULT_ITERATIONS: usize = 20;
const DEFAULT_PROFILE_ITERATIONS: usize = 100;

fn main() {
    let args: Vec<String> = env::args().collect();
    let scenario = args.get(1).map(|s| s.as_str()).unwrap_or("all");
    let size = parse_usize(args.get(2), DEFAULT_SIZE);
    let iterations = parse_usize(args.get(3), default_iterations_for(scenario));

    if matches!(scenario, "-h" | "--help" | "help") {
        print_help();
        return;
    }

    println!("{:=^72}", " NEEDLE PROFILER ");
    println!("scenario   : {}", scenario);
    println!("size       : {}", size);
    println!("iterations : {}", iterations);
    println!();

    match scenario {
        "all" => {
            profile_insert(size, iterations);
            profile_substring(size, iterations);
            profile_prefix(size, iterations);
            profile_save_load(size.min(100_000), iterations.min(10));
            profile_walk(iterations.min(10));
        }
        "insert" => profile_insert(size, iterations),
        "substring" => profile_substring(size, iterations),
        "substring-miss" => profile_substring_miss(size, iterations),
        "substring-hit" => profile_substring_hit(size, iterations),
        "prefix" => profile_prefix(size, iterations),
        "save-load" => profile_save_load(size, iterations),
        "walk" => profile_walk(iterations),
        other => {
            eprintln!("unknown scenario: {other}");
            eprintln!();
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!("Usage:");
    println!("  cargo run --release --example profile -- <scenario> [size] [iterations]");
    println!();
    println!(
        "Defaults: size={} and scenario-specific iterations ({} for profiling-focused substring runs, {} otherwise)",
        DEFAULT_SIZE, DEFAULT_PROFILE_ITERATIONS, DEFAULT_ITERATIONS
    );
    println!();
    println!("Scenarios:");
    println!("  all         run every profiling workload");
    println!("  insert      repeatedly build an index");
    println!("  substring   run both miss and hit substring searches");
    println!("  substring-miss repeatedly run a zero-hit substring search");
    println!("  substring-hit repeatedly run a single-hit substring search");
    println!("  prefix      repeatedly run prefix search on a prebuilt index");
    println!("  save-load   repeatedly serialize and deserialize an index");
    println!("  walk        repeatedly walk a synthetic directory tree");
}

fn parse_usize(arg: Option<&String>, default: usize) -> usize {
    arg.and_then(|value| value.parse().ok()).unwrap_or(default)
}

fn default_iterations_for(scenario: &str) -> usize {
    match scenario {
        "substring" | "substring-miss" | "substring-hit" => DEFAULT_PROFILE_ITERATIONS,
        _ => DEFAULT_ITERATIONS,
    }
}

fn profile_insert(size: usize, iterations: usize) {
    let started = Instant::now();
    let mut total_entries = 0usize;

    for _ in 0..iterations {
        let mut idx = Index::new();
        for i in 0..size {
            idx.insert(&sample_path(i), false);
        }
        total_entries += idx.count();
        black_box(idx);
    }

    print_summary("insert", total_entries, started.elapsed());
}

fn profile_substring(size: usize, iterations: usize) {
    profile_substring_miss(size, iterations);
    profile_substring_hit(size, iterations);
}

fn profile_substring_miss(size: usize, iterations: usize) {
    let idx = build_index(size);
    let started = Instant::now();
    let mut total_hits = 0usize;

    for _ in 0..iterations {
        total_hits += black_box(idx.search_substring("zzzzzzzz")).len();
    }

    print_summary("substr-miss", total_hits, started.elapsed());
}

fn profile_substring_hit(size: usize, iterations: usize) {
    let idx = build_index(size);
    let started = Instant::now();
    let mut total_hits = 0usize;

    for i in 0..iterations {
        let needle = format!("{:08}", i % size.max(1));
        total_hits += black_box(idx.search_substring(&needle)).len();
    }

    print_summary("substr-hit", total_hits, started.elapsed());
}

fn profile_prefix(size: usize, iterations: usize) {
    let idx = build_index(size);
    let started = Instant::now();
    let mut total_hits = 0usize;

    for i in 0..iterations {
        let prefix = format!("file_{:03}", i % 1_000);
        total_hits += black_box(idx.search_prefix(&prefix)).len();
    }

    print_summary("prefix", total_hits, started.elapsed());
}

fn profile_save_load(size: usize, iterations: usize) {
    let idx = build_index(size);
    let dir = temp_dir();
    let path = dir.join("profile.bin");
    let started = Instant::now();
    let mut total_entries = 0usize;

    for _ in 0..iterations {
        idx.save(&path).unwrap();
        let loaded = Index::load(&path).unwrap();
        total_entries += loaded.count();
        black_box(loaded);
    }

    print_summary("save-load", total_entries, started.elapsed());
    fs::remove_dir_all(dir).ok();
}

fn profile_walk(iterations: usize) {
    let dir = temp_dir();
    let root = create_synthetic_tree(&dir);
    let excludes = Excludes::new();
    let started = Instant::now();
    let mut total_entries = 0usize;

    for _ in 0..iterations {
        let mut idx = Index::new();
        total_entries += walker::walk(&root, &mut idx, &excludes, false);
        black_box(idx);
    }

    print_summary("walk", total_entries, started.elapsed());
    fs::remove_dir_all(dir).ok();
}

fn print_summary(name: &str, units: usize, elapsed: Duration) {
    println!(
        "{name:>10}: {:>8.1} ms total  | {:>12} units | {:>12.0} units/s",
        elapsed.as_secs_f64() * 1000.0,
        units,
        units as f64 / elapsed.as_secs_f64().max(f64::EPSILON)
    );
}

fn build_index(size: usize) -> Index {
    let mut idx = Index::new();
    for i in 0..size {
        idx.insert(&sample_path(i), false);
    }
    idx
}

fn sample_path(i: usize) -> String {
    format!("/home/user/docs/sub/deep/folder/file_{i:08}.txt")
}

fn temp_dir() -> PathBuf {
    let mut dir = env::temp_dir();
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    dir.push(format!("toge-profile-{id}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_synthetic_tree(root_parent: &Path) -> PathBuf {
    let root = root_parent.join("data");
    fs::create_dir(&root).unwrap();

    let levels = 3;
    let per_level = 12;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let depth = dir.strip_prefix(&root).unwrap().components().count();
        if depth >= levels {
            for i in 0..per_level {
                fs::write(dir.join(format!("file_{i:03}.txt")), "data").unwrap();
            }
            continue;
        }

        for i in 0..per_level {
            let subdir = dir.join(format!("dir_{i:03}"));
            fs::create_dir(&subdir).unwrap();
            stack.push(subdir);
        }
        for i in 0..(per_level / 2) {
            fs::write(dir.join(format!("file_{i:03}.dat")), "data").unwrap();
        }
    }

    root
}
