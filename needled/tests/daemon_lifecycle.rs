//! Phase 9 daemon lifecycle integration tests.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
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
                Response::Status(st) if st.is_ready => return true,
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

#[test]
fn daemon_starts_and_reports_ready() {
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
            assert!(s.is_ready);
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
