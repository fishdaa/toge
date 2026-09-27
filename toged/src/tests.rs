use crate::{
    DaemonState, IndexChange, WatchScope, WatcherStatus, apply_highlight_ranges,
    canonical_starts_with, discover_roots, ensure_private_dir, hand_off_to_watcher, handle_query,
    handle_request, highlight_path, index_created_path, is_ignored_path, is_own_path,
    is_within_roots, mark_watcher_unavailable, read_request, remove_deleted_path, resolve_events,
    status_response, stream_results, term_needles, write_stream_event,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::{fs, io, thread};

use toge_core::config::Config;
use toge_core::index::Index;
use toge_core::ipc::{DaemonStatus, OutputFormat, QueryRequest, Request, Response};
use toge_core::query::{Query, SearchMode, Sort, TextTerm};

fn visible_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("toged-test-")
        .tempdir_in(std::env::temp_dir())
        .unwrap()
}

#[test]
fn moved_in_directory_is_indexed_recursively() {
    let root = visible_tempdir();
    let torrent = root.path().join("completed-torrent");
    let season = torrent.join("season");
    let episode = season.join("episode.mkv");
    fs::create_dir_all(&season).unwrap();
    fs::write(&episode, b"complete").unwrap();

    let mut state = DaemonState {
        index: Index::new(),
        status: DaemonStatus::Ready,
        status_message: String::new(),
        build_duration_ms: 0,
        last_updated_unix: 0,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: toge_core::sort::OrderCache::default(),
        index_generation: 0,
    };
    index_created_path(
        &mut state,
        torrent.to_str().unwrap(),
        true,
        &Config::default_config(),
    );

    assert!(state.index.id_by_path(episode.to_str().unwrap()).is_some());
    assert!(state.index.id_by_path(season.to_str().unwrap()).is_some());
}

#[test]
fn deleting_directory_removes_indexed_descendants() {
    let mut index = Index::new();
    index.insert("/downloads/torrent", true);
    index.insert("/downloads/torrent/season", true);
    index.insert("/downloads/torrent/season/episode.mkv", false);
    index.insert("/downloads/torrent-2/keep.mkv", false);

    remove_deleted_path(&mut index, "/downloads/torrent", &[]);

    assert!(index.id_by_path("/downloads/torrent").is_none());
    assert!(
        index
            .id_by_path("/downloads/torrent/season/episode.mkv")
            .is_none()
    );
    assert!(index.id_by_path("/downloads/torrent-2/keep.mkv").is_some());
}

#[test]
fn watcher_runtime_failure_marks_daemon_ready_but_degraded() {
    let state = Arc::new(Mutex::new(DaemonState {
        index: Index::new(),
        status: DaemonStatus::StartingWatcher,
        status_message: "Setting up file watcher".to_string(),
        build_duration_ms: 0,
        last_updated_unix: 0,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: toge_core::sort::OrderCache::default(),
        index_generation: 0,
    }));

    mark_watcher_unavailable(&state, "permission denied");

    let state = state.lock().unwrap();
    assert_eq!(state.status, DaemonStatus::Ready);
    assert!(!state.watcher.is_healthy);
    assert_eq!(state.watcher.watch_failure_count, 1);
    assert!(state.status_message.contains("sudo setcap"));
    assert!(
        state
            .watcher_log
            .last()
            .unwrap()
            .contains("permission denied")
    );
}

#[test]
fn watcher_spawn_failure_still_lets_indexing_reach_ready() {
    let mut state = DaemonState {
        index: Index::new(),
        status: DaemonStatus::LoadingIndex,
        status_message: String::new(),
        build_duration_ms: 0,
        last_updated_unix: 0,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: toge_core::sort::OrderCache::default(),
        index_generation: 0,
    };

    hand_off_to_watcher(&mut state, Some("thread spawn error: busy"));
    assert_eq!(state.status, DaemonStatus::Ready);
    assert!(!state.watcher.is_healthy);

    state.status = DaemonStatus::LoadingIndex;
    hand_off_to_watcher(&mut state, None);
    assert_eq!(state.status, DaemonStatus::StartingWatcher);
}

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
    assert_eq!(
        stdout.trim(),
        format!("toged {}", env!("CARGO_PKG_VERSION"))
    );
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
        last_updated_unix: 0,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: toge_core::sort::OrderCache::default(),
        index_generation: 0,
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
fn modified_sort_refreshes_timestamps_when_metadata_indexing_is_disabled() {
    let root = visible_tempdir();
    let older = root.path().join("older.txt");
    let newer = root.path().join("newer.txt");
    fs::write(&older, b"older").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    fs::write(&newer, b"newer").unwrap();

    let mut index = Index::new();
    index.insert_with_metadata(older.to_str().unwrap(), false, 0, 0, 0, 0);
    index.insert_with_metadata(newer.to_str().unwrap(), false, 0, 0, 0, 0);

    let response = handle_query(
        &mut index,
        &mut toge_core::sort::OrderCache::default(),
        &QueryRequest {
            id: 1,
            raw: "sort:modified-desc".into(),
            max_results: 10,
            offset: 0,
            format: OutputFormat::Default,
            highlight: false,
        },
        false,
    );

    let Response::Results(results) = response else {
        panic!("expected sorted results");
    };
    assert_eq!(results.rows[0].path, newer.to_str().unwrap());
    assert!(results.rows[0].modified_unix > results.rows[1].modified_unix);
}

#[test]
fn status_response_uses_the_last_real_index_update_time() {
    let state = DaemonState {
        index: Index::new(),
        status: DaemonStatus::Ready,
        status_message: String::new(),
        build_duration_ms: 0,
        last_updated_unix: 1_700_000_000,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: toge_core::sort::OrderCache::default(),
        index_generation: 0,
    };

    assert_eq!(status_response(&state).last_updated_unix, 1_700_000_000);
    assert_eq!(status_response(&state).last_updated_unix, 1_700_000_000);
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
    let dir = visible_tempdir();
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

#[test]
fn is_ignored_path_matches_hidden_directory_contents() {
    let dir = visible_tempdir();
    let state = dir.path().join("state");
    let config = dir.path().join("config");
    let hidden = dir.path().join(".hidden");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&hidden).unwrap();
    let hidden_file = hidden.join("episode.mkv");
    fs::write(&hidden_file, "x").unwrap();

    assert!(is_ignored_path(
        hidden_file.to_str().unwrap(),
        &state,
        &config,
        false
    ));
}

#[test]
fn is_ignored_path_keeps_hidden_files_outside_hidden_dirs() {
    let dir = visible_tempdir();
    let state = dir.path().join("state");
    let config = dir.path().join("config");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&config).unwrap();
    let dotfile = dir.path().join(".episode.mkv");
    fs::write(&dotfile, "x").unwrap();

    assert!(!is_ignored_path(
        dotfile.to_str().unwrap(),
        &state,
        &config,
        false
    ));
}

#[test]
fn discover_roots_falls_back_to_home_directory() {
    let cfg = Config::default_config();
    let expected_home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));

    match expected_home {
        Some(home) => assert_eq!(
            discover_roots(&Config {
                roots: Vec::new(),
                ..cfg
            }),
            vec![home]
        ),
        None => assert!(
            discover_roots(&Config {
                roots: Vec::new(),
                ..cfg
            })
            .is_empty()
        ),
    }
}

#[test]
fn is_within_roots_rejects_paths_outside_home_root() {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));

    let Some(home) = home else {
        return;
    };

    assert!(is_within_roots(
        &home.join("documents/file.txt").to_string_lossy(),
        std::slice::from_ref(&home)
    ));
    assert!(!is_within_roots("/var/tmp/outside.txt", &[home]));
}

#[test]
fn unlimited_query_returns_every_match_and_handles_nonzero_offsets() {
    let mut index = Index::new();
    for i in 0..10_003 {
        index.insert(&format!("/unlimited/file-{i:05}.txt"), false);
    }
    for offset in [0, 3, usize::MAX] {
        let Response::Results(results) = handle_query(
            &mut index,
            &mut toge_core::sort::OrderCache::default(),
            &QueryRequest {
                id: 7,
                raw: String::new(),
                max_results: usize::MAX,
                offset,
                format: OutputFormat::Default,
                highlight: false,
            },
            false,
        ) else {
            panic!("expected results")
        };
        assert_eq!(results.total_count, 10_003);
        assert_eq!(results.rows.len(), 10_003 - offset.min(10_003));
        if offset < 10_003 {
            assert_eq!(
                results.rows[0].path,
                format!("/unlimited/file-{offset:05}.txt")
            );
            assert_eq!(
                results.rows.last().unwrap().path,
                "/unlimited/file-10002.txt"
            );
        }
    }
}

#[test]
fn stream_sends_bounded_batches_and_final_totals_in_both_orders() {
    use toge_core::ipc::{STREAM_BATCH_SIZE, StreamOrder, StreamQueryRequest};
    for order in [StreamOrder::Index, StreamOrder::Sorted] {
        let mut index = Index::new();
        for i in (0..300).rev() {
            index.insert_with_metadata(&format!("/tmp/file-{i:04}.txt"), false, i, 0, 0, 0);
        }
        let expected_total_size = index.entries.iter().map(|entry| entry.size).sum::<u64>();
        let request = StreamQueryRequest {
            query: QueryRequest {
                id: 7,
                raw: "file".into(),
                max_results: 270,
                offset: 5,
                format: OutputFormat::Default,
                highlight: false,
            },
            order,
        };
        let (mut server, mut client) = UnixStream::pair().unwrap();
        let producer_request = request.clone();
        let producer = thread::spawn(move || {
            assert_eq!(
                read_request(&mut server).unwrap(),
                Some(Request::StreamQuery(producer_request.clone()))
            );
            stream_results(
                &mut server,
                &producer_request,
                &mut index,
                &mut toge_core::sort::OrderCache::default(),
                false,
            )
        });
        let mut paths = Vec::new();
        let mut batch_sizes = Vec::new();
        let summary = toge_core::ipc::stream_query(&mut client, &request, |rows| {
            batch_sizes.push(rows.len());
            paths.extend(rows.iter().map(|row| row.path.clone()));
            Ok(())
        })
        .unwrap();
        producer.join().unwrap().unwrap();
        assert_eq!(batch_sizes, [STREAM_BATCH_SIZE, STREAM_BATCH_SIZE, 14]);
        assert_eq!(summary.total_count, 300);
        assert_eq!(summary.returned_count, 270);
        assert_eq!(summary.total_size, expected_total_size);
        match order {
            StreamOrder::Index => assert_eq!(paths[0], "/tmp/file-0294.txt"),
            StreamOrder::Sorted => assert_eq!(paths[0], "/tmp/file-0005.txt"),
        }
    }
}

#[test]
fn disconnected_stream_and_expired_write_stop_promptly() {
    let mut index = Index::new();
    index.insert("/tmp/foo.txt", false);
    let request = toge_core::ipc::StreamQueryRequest {
        query: QueryRequest {
            id: 1,
            raw: String::new(),
            max_results: usize::MAX,
            offset: 0,
            format: OutputFormat::Default,
            highlight: false,
        },
        order: toge_core::ipc::StreamOrder::Index,
    };
    let (mut server, client) = UnixStream::pair().unwrap();
    drop(client);
    assert!(
        stream_results(
            &mut server,
            &request,
            &mut index,
            &mut toge_core::sort::OrderCache::default(),
            false
        )
        .is_err()
    );
    let (mut server, _client) = UnixStream::pair().unwrap();
    let event = toge_core::ipc::StreamEvent::Done(toge_core::ipc::StreamSummary {
        id: 1,
        total_count: 0,
        total_size: 0,
        returned_count: 0,
    });
    assert_eq!(
        write_stream_event(&mut server, &event, Instant::now())
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
}

struct NullWatcher;
impl toge_core::sys::FsWatcher for NullWatcher {
    fn watch(&mut self, _: &std::path::Path) -> io::Result<()> {
        Ok(())
    }
    fn unwatch(&mut self, _: &std::path::Path) -> io::Result<()> {
        Ok(())
    }
    fn poll_events(&mut self) -> io::Result<Vec<toge_core::sys::WatchEvent>> {
        Ok(Vec::new())
    }
}

#[test]
fn watcher_drops_events_in_excluded_folders_before_locking() {
    use toge_core::sys::WatchEvent;
    let root = visible_tempdir();
    let roots = [root.path().to_path_buf()];
    let excludes = toge_core::walker::Excludes {
        folders: vec!["target".into()],
        ..Default::default()
    };
    let other = visible_tempdir();
    let scope = WatchScope::new(&roots, excludes, other.path(), other.path());
    let at = |name: &str| root.path().join(name).to_str().unwrap().to_string();
    let changes = resolve_events(
        vec![
            WatchEvent::Create {
                path: at("target/debug/a.o"),
                is_dir: false,
            },
            WatchEvent::Delete {
                path: at("target/debug/a.o"),
            },
            WatchEvent::Create {
                path: at("src/main.rs"),
                is_dir: false,
            },
            WatchEvent::Move {
                from: at("src/lib.rs"),
                to: at("target/lib.rs"),
            },
        ],
        &scope,
        &mut NullWatcher,
    );
    assert!(matches!(
        changes.as_slice(),
        [IndexChange::Create { path, .. }, IndexChange::Delete { path: moved }]
            if *path == at("src/main.rs") && *moved == at("src/lib.rs")
    ));
}

#[test]
fn deleting_a_file_leaves_entries_sharing_its_prefix() {
    let mut index = Index::new();
    index.insert("/data/report", false);
    index.insert("/data/report.bak", false);
    index.insert("/data/reports/q1.txt", false);

    remove_deleted_path(&mut index, "/data/report", &[]);

    assert!(index.id_by_path("/data/report").is_none());
    assert!(index.id_by_path("/data/report.bak").is_some());
    assert!(index.id_by_path("/data/reports/q1.txt").is_some());
}

#[test]
fn deleting_an_unindexed_root_removes_only_its_descendants() {
    let root = std::path::PathBuf::from("/downloads/torrent");
    let mut index = Index::new();
    index.insert("/downloads/torrent/season", true);
    index.insert("/downloads/torrent/season/episode.mkv", false);
    index.insert("/downloads/torrent-2/keep.mkv", false);
    assert!(index.id_by_path(root.to_str().unwrap()).is_none());
    remove_deleted_path(
        &mut index,
        root.to_str().unwrap(),
        std::slice::from_ref(&root),
    );
    assert_eq!(index.count(), 1);
    assert!(index.id_by_path("/downloads/torrent-2/keep.mkv").is_some());
}
