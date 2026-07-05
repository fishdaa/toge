use crate::{handle_request, DaemonState, WatcherStatus};
use std::process::Command;
use std::sync::{Arc, Mutex};

use needle_core::config::Config;
use needle_core::index::Index;
use needle_core::ipc::{OutputFormat, QueryRequest, Request, Response};

/// Helper to build and run the daemon binary with given args.
fn run_needled(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "needled", "--"])
        .args(args)
        .output()
        .expect("failed to run needled")
}

#[test]
fn needled_help_exits_zero() {
    let output = run_needled(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("needled") || stdout.contains("Options"));
    assert!(output.status.success());
}

#[test]
fn needled_version_prints_version() {
    let output = run_needled(&["-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("needled 0.1.1"));
    assert!(output.status.success());
}

#[test]
fn query_before_ready_returns_not_ready_error() {
    let temp = std::env::temp_dir().join(format!("needled-unit-{}", std::process::id()));
    let state = Arc::new(Mutex::new(DaemonState {
        index: Index::new(),
        is_ready: false,
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
