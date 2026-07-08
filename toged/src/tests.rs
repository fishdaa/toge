use crate::{
    apply_highlight_ranges, canonical_starts_with, ensure_private_dir, handle_request,
    highlight_path, is_own_path, term_needles, DaemonState, WatcherStatus,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::{Arc, Mutex};

use toge_core::config::Config;
use toge_core::index::Index;
use toge_core::ipc::{DaemonStatus, OutputFormat, QueryRequest, Request, Response};
use toge_core::query::{Query, SearchMode, Sort, TextTerm};

/// Helper to build and run the daemon binary with given args.
fn run_needled(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "toged", "--"])
        .args(args)
        .output()
        .expect("failed to run toged")
}

#[test]
fn needled_help_exits_zero() {
    let output = run_needled(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("toged") || stdout.contains("Options"));
    assert!(output.status.success());
}

#[test]
fn needled_version_prints_version() {
    let output = run_needled(&["-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("toged 0.1.1"));
    assert!(output.status.success());
}

#[test]
fn query_before_ready_returns_not_ready_error() {
    let temp = std::env::temp_dir().join(format!("toged-unit-{}", std::process::id()));
    let state = Arc::new(Mutex::new(DaemonState {
        index: Index::new(),
        status: DaemonStatus::Starting,
        status_message: String::new(),
        build_duration_ms: 0,
        watcher: WatcherStatus::default(),
    }));

    let resp = handle_request(
        Request::Query(QueryRequest {
            id: 1,
            raw: "foo".into(),
            max_results: 10,
            offset: 0,
            format: OutputFormat::Default,
            highlight: false,
        }),
        &temp,
        &Config::default_config(),
        &state,
    );

    assert_eq!(resp, Response::Error("daemon not ready".into()));
}

#[test]
fn highlight_ranges_merge_overlapping_matches() {
    let highlighted = apply_highlight_ranges("foobar", &mut [(0, 3), (3, 6)]);
    assert_eq!(highlighted, "*foobar*");
}

#[test]
fn highlight_path_marks_multiple_terms() {
    let query = Query {
        raw: "foo bar".into(),
        mode: SearchMode::Substring,
        match_case: false,
        match_whole_word: false,
        match_path: false,
        require_file: false,
        require_folder: false,
        whole_filename: false,
        terms: vec![
            TextTerm::Substring("foo".into()),
            TextTerm::Substring("bar".into()),
        ],
        ext: None,
        path_filter: None,
        size: None,
        date_modified: None,
        date_created: None,
        date_accessed: None,
        attributes: None,
        offset: 0,
        max_results: usize::MAX,
        sort: Sort::NameAsc,
    };

    assert_eq!(
        highlight_path("/tmp/foo_bar.txt", &query),
        "/tmp/*foo*_*bar*.txt"
    );
}

#[test]
fn term_needles_ignores_negated_terms() {
    let needles = term_needles(&TextTerm::Not(Box::new(TextTerm::Substring("foo".into()))));
    assert!(needles.is_empty());
}

#[test]
fn highlight_path_leaves_non_matching_name_unchanged() {
    let query = Query {
        raw: "missing".into(),
        mode: SearchMode::Substring,
        match_case: false,
        match_whole_word: false,
        match_path: false,
        require_file: false,
        require_folder: false,
        whole_filename: false,
        terms: vec![TextTerm::Substring("missing".into())],
        ext: None,
        path_filter: None,
        size: None,
        date_modified: None,
        date_created: None,
        date_accessed: None,
        attributes: None,
        offset: 0,
        max_results: usize::MAX,
        sort: Sort::NameAsc,
    };

    assert_eq!(
        highlight_path("/tmp/foo_bar.txt", &query),
        "/tmp/foo_bar.txt"
    );
}

#[test]
fn highlight_ranges_ignore_invalid_spans() {
    let highlighted = apply_highlight_ranges("foobar", &mut [(10, 12), (4, 4)]);
    assert_eq!(highlighted, "foobar");
}

#[test]
fn ensure_private_dir_sets_owner_only_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let private = dir.path().join("state");
    ensure_private_dir(&private).unwrap();
    let mode = fs::metadata(&private).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn canonical_starts_with_handles_symlinked_children() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let target = dir.path().join("target");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&target).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, state.join("link")).unwrap();

    let linked_child = state.join("link").join("file.txt");
    fs::write(target.join("file.txt"), "x").unwrap();

    assert!(!canonical_starts_with(&linked_child, &state));
}

#[test]
fn is_own_path_uses_canonical_paths() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let config = dir.path().join("config");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&config).unwrap();
    let file = state.join("index.bin");
    fs::write(&file, "x").unwrap();

    assert!(is_own_path(file.to_str().unwrap(), &state, &config));
}

#[test]
fn is_own_path_fails_closed_for_nonexistent_path() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let config = dir.path().join("config");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&config).unwrap();

    let missing = state.join("missing").join("index.bin");
    assert!(!is_own_path(missing.to_str().unwrap(), &state, &config));
}

#[test]
fn canonical_starts_with_returns_false_when_root_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("child");
    fs::write(&path, "x").unwrap();

    assert!(!canonical_starts_with(
        &path,
        &dir.path().join("missing-root")
    ));
}
