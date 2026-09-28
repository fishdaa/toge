//! CLI display and export integration tests using a real toged instance.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;
use toge_core::ipc::{Request, Response};

fn test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("toge-cli-test-{}-{}", std::process::id(), name))
}

fn socket_path(name: &str) -> PathBuf {
    test_dir(name).join("state").join("toged.sock")
}

fn needled_binary() -> PathBuf {
    sibling_binary("toged")
}

fn ndl_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_toge").map_or_else(|| sibling_binary("toge"), PathBuf::from)
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

fn run_ndl(socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(ndl_binary())
        .env("TOGE_SOCKET", socket)
        .args(args)
        .output()
        .expect("failed to run toge")
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
    let len = usize::try_from(u64::from_le_bytes(len_buf)).unwrap();
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

fn uds_available(name: &str) -> bool {
    let dir = test_dir(&format!("probe-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("probe.sock");
    let available = UnixListener::bind(&sock).is_ok();
    let _ = fs::remove_dir_all(&dir);
    available
}

#[test]
fn ndl_status_recovers_from_stale_socket_by_starting_daemon() {
    if !uds_available("stale-socket") {
        return;
    }

    let dir = test_dir("stale-socket");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("toged.sock");

    let listener = UnixListener::bind(&sock).unwrap();
    drop(listener);
    assert!(sock.exists(), "expected stale socket file");

    let output = Command::new(ndl_binary())
        .env("HOME", &dir)
        .env("TOGE_SOCKET", &sock)
        .args(["-status"])
        .output()
        .expect("failed to run toge");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("status:"), "unexpected stdout: {stdout}");

    let mut stream = UnixStream::connect(&sock).unwrap();
    send_request(&mut stream, &Request::Quit);
    let _ = read_response(&mut stream);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn ndl_csv_output_has_header_and_crlf() {
    if !uds_available("csv") {
        return;
    }
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
    if !uds_available("csv-no-header") {
        return;
    }
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
    if !uds_available("count") {
        return;
    }
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
    if !uds_available("total-size") {
        return;
    }
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
    if !uds_available("export") {
        return;
    }
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

#[test]
fn streaming_cli_matches_sorted_output_and_exports_multiple_batches() {
    if !uds_available("stream") {
        return;
    }
    let (dir, state, cfg, root) = setup("stream");
    for i in 0..300 {
        fs::write(root.join(format!("file-{i:04}.txt")), "abc").unwrap();
    }
    let sock = socket_path("stream");
    let mut child = spawn_needled(&[
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
        "--clean",
    ]);
    assert!(wait_for_ready(&sock, 10_000));
    let normal = run_ndl(&sock, &["--csv", "--sort", "name-asc", "file-"]);
    let streamed = run_ndl(&sock, &["--stream", "--csv", "--sort", "name-asc", "file-"]);
    assert!(
        normal.status.success(),
        "{}",
        String::from_utf8_lossy(&normal.stderr)
    );
    assert!(
        streamed.status.success(),
        "{}",
        String::from_utf8_lossy(&streamed.stderr)
    );
    assert_eq!(normal.stdout, streamed.stdout);
    assert_eq!(
        String::from_utf8_lossy(&streamed.stdout)
            .matches("Name\r\n")
            .count(),
        1
    );
    let limited = run_ndl(
        &sock,
        &[
            "--stream",
            "--sort",
            "name-asc",
            "--offset",
            "10",
            "--max-results",
            "3",
            "file-",
        ],
    );
    assert!(limited.status.success());
    assert_eq!(String::from_utf8_lossy(&limited.stdout).lines().count(), 3);
    assert!(String::from_utf8_lossy(&limited.stdout).contains("file-0010.txt"));
    let count = run_ndl(&sock, &["--stream", "--get-result-count", "file-"]);
    assert!(count.status.success());
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "300");
    let size = run_ndl(&sock, &["--stream", "--get-total-size", "file-"]);
    assert!(size.status.success());
    assert_eq!(String::from_utf8_lossy(&size.stdout).trim(), "900");
    let export = dir.join("stream.csv");
    let output = run_ndl(
        &sock,
        &[
            "--stream",
            "--sort",
            "name-asc",
            "--export-csv",
            export.to_str().unwrap(),
            "file-",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&export).unwrap(), normal.stdout);
    let failed = run_ndl(
        &sock,
        &[
            "--stream",
            "--export-csv",
            export.to_str().unwrap(),
            "regex:(",
        ],
    );
    assert!(!failed.status.success());
    assert_eq!(
        fs::read(&export).unwrap(),
        normal.stdout,
        "failed stream replaced completed export"
    );
    assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
    let empty = run_ndl(
        &sock,
        &[
            "--stream",
            "--csv",
            "--hide-empty-search-results",
            "no-match",
        ],
    );
    assert!(empty.status.success());
    assert!(empty.stdout.is_empty());
    let missing = run_ndl(&sock, &["--stream", "--no-result-error", "no-match"]);
    assert_eq!(missing.status.code(), Some(9));
    cleanup(&dir, &mut child);
}

#[test]
fn ndl_json_output_and_status_are_machine_readable() {
    if !uds_available("json") {
        return;
    }
    let (dir, state, cfg, root) = setup("json");
    fs::write(root.join("-dash.txt"), "x").unwrap();
    let sock = socket_path("json");
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

    let output = run_ndl(&sock, &["--json", "--no-wait", "sort:name-asc", "foo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "unexpected stdout: {stdout}");
    assert_eq!(
        lines[0],
        format!(
            "{{\"path\":\"{}\",\"name\":\"foo.txt\",\"parent\":\"{}\",\"ext\":\"txt\",\"is_dir\":false,\"size\":5,",
            root.join("foo.txt").display(),
            root.display()
        ) + &lines[0][lines[0].find("\"modified\"").unwrap()..]
    );
    assert!(lines[1].contains("\"name\":\"food.txt\""));

    let streamed = run_ndl(&sock, &["--json", "--stream", "food"]);
    assert!(streamed.status.success());
    assert!(String::from_utf8_lossy(&streamed.stdout).contains("\"name\":\"food.txt\""));

    let dashed = run_ndl(&sock, &["--json", "--", "-dash"]);
    assert!(
        dashed.status.success(),
        "{}",
        String::from_utf8_lossy(&dashed.stderr)
    );
    assert!(String::from_utf8_lossy(&dashed.stdout).contains("\"name\":\"-dash.txt\""));

    let status = run_ndl(&sock, &["--status", "--json"]);
    assert!(status.status.success());
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.starts_with("{\"status\":\"ready\",\"ready\":true,"),
        "{status}"
    );

    cleanup(&dir, &mut child);
}
