//! Phase 9 daemon lifecycle integration tests.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

fn socket_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("needle-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir.join("needled.sock")
}

fn spawn_needled(args: &[&str]) -> Child {
    Command::new("cargo")
        .args(["run", "--bin", "needled", "--"])
        .args(args)
        .spawn()
        .expect("failed to spawn needled")
}

fn wait_for_socket(path: &PathBuf, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
#[ignore = "integration test requires implemented daemon"]
fn daemon_creates_socket_on_startup() {
    let sock = socket_path();
    let _ = fs::remove_file(&sock);
    let mut child = spawn_needled(&["--socket", sock.to_str().unwrap()]);
    assert!(wait_for_socket(&sock, 2000), "daemon did not create socket");
    let _ = child.kill();
    let _ = fs::remove_file(&sock);
}

#[test]
#[ignore = "integration test requires implemented daemon"]
fn daemon_responds_to_status_query() {
    let sock = socket_path();
    let _ = fs::remove_file(&sock);
    let mut child = spawn_needled(&["--socket", sock.to_str().unwrap()]);
    assert!(wait_for_socket(&sock, 2000));

    // Connect and send a status request.
    let mut stream = std::os::unix::net::UnixStream::connect(&sock).unwrap();
    use std::io::{Read, Write};
    stream.write_all(b"status\n").unwrap();
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf).unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]);
    assert!(resp.chars().next().unwrap().is_ascii_digit(), "status should start with a count");

    let _ = child.kill();
    let _ = fs::remove_file(&sock);
}
