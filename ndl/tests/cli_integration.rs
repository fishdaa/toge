//! CLI display and export integration tests using a real needled instance.

use needle_core::ipc::{Request, Response};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

fn test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("needle-cli-test-{}-{}", std::process::id(), name))
}

fn socket_path(name: &str) -> PathBuf {
    test_dir(name).join("state").join("needled.sock")
}

fn needled_binary() -> PathBuf {
    sibling_binary("needled")
}

fn ndl_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_ndl")
        .map(PathBuf::from)
        .unwrap_or_else(|| sibling_binary("ndl"))
}

fn sibling_binary(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    exe.parent()
        .and_then(Path::parent)
        .expect("target debug dir")
        .join(name)
}

fn spawn_needled(args: &[&str]) -> Child {
    Command::new(needled_binary())
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn needled")
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

fn run_ndl(socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(ndl_binary())
        .env("NEEDLE_SOCKET", socket)
        .args(args)
        .output()
        .expect("failed to run ndl")
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

fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = test_dir(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();

    let root = dir.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("foo.txt"), "hello").unwrap();
    fs::write(root.join("food.txt"), "world!").unwrap();

    let cfg = dir.join("config.toml");
    let contents = format!(
        r#"
[roots]
include = ["{}"]

[index]
size = true
"#,
        root.display()
    );
    fs::write(&cfg, contents).unwrap();
    (dir, state, cfg, root)
}

fn cleanup(dir: &PathBuf, child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ndl_csv_output_has_header_and_crlf() {
    let (dir, state, cfg, root) = setup("csv");
    let sock = socket_path("csv");
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
    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let output = run_ndl(&sock, &["-csv", "foo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("Name\r\n"));
    assert!(stdout.contains(&format!("\"{}\"", root.join("foo.txt").display())));

    cleanup(&dir, &mut child);
}

#[test]
fn ndl_csv_no_header_omits_header() {
    let (dir, state, cfg, root) = setup("csv-no-header");
    let sock = socket_path("csv-no-header");
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
    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let output = run_ndl(&sock, &["-csv", "-no-header", "foo.txt"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Name\r\n"));
    assert_eq!(
        stdout,
        format!("\"{}\"\r\n", root.join("foo.txt").display())
    );

    cleanup(&dir, &mut child);
}

#[test]
fn ndl_get_result_count_prints_number_only() {
    let (dir, state, cfg, _root) = setup("count");
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

    assert!(wait_for_socket(&sock, 2_000), "socket not created");
    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let output = run_ndl(&sock, &["-get-result-count", "foo"]);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.parse::<usize>().is_ok());
    assert_eq!(stdout, "2");

    cleanup(&dir, &mut child);
}

#[test]
fn ndl_get_total_size_prints_number_only() {
    let (dir, state, cfg, _root) = setup("total-size");
    let sock = socket_path("total-size");
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
    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let output = run_ndl(&sock, &["-get-total-size", "foo"]);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.parse::<u64>().is_ok());
    assert_eq!(stdout, "11");

    cleanup(&dir, &mut child);
}

#[test]
fn ndl_export_csv_creates_file() {
    let (dir, state, cfg, _root) = setup("export");
    let sock = socket_path("export");
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
    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let path = dir.join("out.csv");
    let path_str = path.to_str().unwrap();
    let output = run_ndl(&sock, &["-export-csv", path_str, "foo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(path.exists());

    cleanup(&dir, &mut child);
}
