//! Phase 9 daemon lifecycle integration tests.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;
use toge_core::ipc::{Request, Response};

fn test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("toge-test-{}-{}", std::process::id(), name))
}

fn socket_path(name: &str) -> PathBuf {
    test_dir(name).join("state").join("toged.sock")
}

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_toged"))
}

fn spawn_needled(args: &[&str]) -> Child {
    Command::new(binary_path())
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn toged")
}

fn wait_for_socket(path: &Path, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn wait_for_ready(sock: &Path, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if let Ok(mut s) = UnixStream::connect(sock) {
            send_request(&mut s, &Request::Status);
            match read_response(&mut s) {
                Response::Status(st) if st.status == toge_core::ipc::DaemonStatus::Ready => {
                    return true;
                }
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn send_request(stream: &mut UnixStream, req: &Request) {
    let bytes = req.encode();
    stream
        .write_all(&(bytes.len() as u64).to_le_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    stream.flush().unwrap();
}

fn read_response(stream: &mut UnixStream) -> Response {
    let mut len_buf = [0u8; 8];
    stream.read_exact(&mut len_buf).unwrap();
    let len = u64::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).unwrap();
    Response::decode(&buf).unwrap()
}

fn query_count(sock: &Path, query: &str) -> usize {
    let mut stream = UnixStream::connect(sock).unwrap();
    send_request(
        &mut stream,
        &Request::Query(toge_core::ipc::QueryRequest {
            id: 1,
            raw: query.to_string(),
            max_results: 10,
            offset: 0,
            format: toge_core::ipc::OutputFormat::Default,
            highlight: false,
        }),
    );
    match read_response(&mut stream) {
        Response::Results(results) => results.total_count,
        other => panic!("expected results, got {:?}", other),
    }
}

fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = test_dir(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();

    let root = dir.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("foo.txt"), "hello").unwrap();

    let cfg = dir.join("config.toml");
    let contents = format!(
        r#"
[roots]
include = ["{}"]
"#,
        root.display()
    );
    fs::write(&cfg, contents).unwrap();
    (dir, state, cfg)
}

fn cleanup(dir: &PathBuf, child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(dir);
}

fn uds_available(name: &str) -> bool {
    let dir = test_dir(&format!("probe-{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("probe.sock");
    let available = UnixListener::bind(&sock).is_ok();
    let _ = fs::remove_dir_all(&dir);
    available
}

#[test]
fn daemon_starts_and_reports_ready() {
    if !uds_available("ready") {
        return;
    }
    let (dir, state, cfg) = setup("ready");
    let sock = socket_path("ready");

    let mut child = spawn_needled(&[
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
        "--clean",
    ]);

    assert!(wait_for_socket(&sock, 2_000), "socket not created");
    assert!(wait_for_ready(&sock, 5_000), "daemon never became ready");
    cleanup(&dir, &mut child);
}

#[test]
fn daemon_status_returns_entry_count() {
    if !uds_available("count") {
        return;
    }
    let (dir, state, cfg) = setup("count");
    let sock = socket_path("count");

    let mut child = spawn_needled(&[
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
        "--clean",
    ]);

    assert!(wait_for_ready(&sock, 10_000), "daemon not ready");

    let mut stream = UnixStream::connect(&sock).unwrap();
    send_request(&mut stream, &Request::Status);
    match read_response(&mut stream) {
        Response::Status(s) => {
            assert_eq!(s.status, toge_core::ipc::DaemonStatus::Ready);
            assert!(
                s.indexed_count >= 1,
                "expected at least foo.txt, got {}",
                s.indexed_count
            );
        }
        other => panic!("expected status, got {:?}", other),
    }

    cleanup(&dir, &mut child);
}

#[test]
fn daemon_query_returns_real_file_size() {
    if !uds_available("size") {
        return;
    }
    let (dir, state, cfg) = setup("size");
    let sock = socket_path("size");

    let mut child = spawn_needled(&[
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
        "--clean",
    ]);

    assert!(wait_for_ready(&sock, 10_000), "daemon not ready");

    let mut stream = UnixStream::connect(&sock).unwrap();
    send_request(
        &mut stream,
        &Request::Query(toge_core::ipc::QueryRequest {
            id: 1,
            raw: "foo".to_string(),
            max_results: 10,
            offset: 0,
            format: toge_core::ipc::OutputFormat::Default,
            highlight: false,
        }),
    );

    match read_response(&mut stream) {
        Response::Results(results) => {
            assert_eq!(results.total_count, 1);
            assert_eq!(results.rows.len(), 1);
            assert_eq!(results.rows[0].name, "foo.txt");
            assert_eq!(results.rows[0].size, 5);
            assert_eq!(results.total_size, 5);
        }
        other => panic!("expected results, got {:?}", other),
    }

    cleanup(&dir, &mut child);
}

#[test]
fn daemon_startup_reconciles_changes_made_while_stopped() {
    if !uds_available("startup-reconcile") {
        return;
    }
    let (dir, state, cfg) = setup("startup-reconcile");
    let sock = socket_path("startup-reconcile");
    let args = [
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
    ];

    let mut child = spawn_needled(&[
        args[0], args[1], args[2], args[3], args[4], args[5], "--clean",
    ]);
    assert!(wait_for_ready(&sock, 10_000), "initial daemon not ready");

    let mut stream = UnixStream::connect(&sock).unwrap();
    send_request(&mut stream, &Request::Quit);
    assert_eq!(read_response(&mut stream), Response::Ok);
    assert!(child.wait().unwrap().success());

    let root = dir.join("root");
    fs::remove_file(root.join("foo.txt")).unwrap();
    fs::write(root.join("bar.txt"), "created while stopped").unwrap();

    let mut child = spawn_needled(&args);
    assert!(wait_for_ready(&sock, 10_000), "restarted daemon not ready");
    assert_eq!(query_count(&sock, "foo.txt"), 0);
    assert_eq!(query_count(&sock, "bar.txt"), 1);

    cleanup(&dir, &mut child);
}
