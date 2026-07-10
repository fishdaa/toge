//! Integration tests for the GUI IPC client against a real toged instance.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

fn test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("toge-gui-test-{}-{}", std::process::id(), name))
}

fn socket_path(name: &str) -> PathBuf {
    test_dir(name).join("state").join("toged.sock")
}

fn daemon_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_toged")
        .map(PathBuf::from)
        .unwrap_or_else(|| sibling_binary("toged"))
}

fn sibling_binary(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    exe.parent()
        .and_then(Path::parent)
        .expect("target debug dir")
        .join(name)
}

fn spawn_daemon(args: &[&str]) -> Child {
    Command::new(daemon_binary())
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn toged")
}

fn wait_for_ready(sock: &Path, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        match toge_gui::ipc_client::status(sock) {
            Ok(s) if s.status == toge_core::ipc::DaemonStatus::Ready => return true,
            _ => {}
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
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
    fs::write(root.join("food.txt"), "world!").unwrap();

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
fn ipc_client_queries_daemon() {
    let (dir, state, cfg) = setup("query");
    let sock = socket_path("query");
    let mut child = spawn_daemon(&[
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
        "--clean",
    ]);

    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let results = toge_gui::ipc_client::query(&sock, 1, "foo", 100, 0).unwrap();
    assert_eq!(results.total_count, 2);
    assert_eq!(results.rows.len(), 2);

    cleanup(&dir, &mut child);
}

#[test]
fn ipc_client_returns_rich_rows() {
    let (dir, state, cfg) = setup("rows");
    let cfg_contents = fs::read_to_string(&cfg).unwrap()
        + "\n[index]\nsize = true\ndate_modified = true\n";
    fs::write(&cfg, cfg_contents).unwrap();

    let sock = socket_path("rows");
    let mut child = spawn_daemon(&[
        "--socket",
        sock.to_str().unwrap(),
        "--config",
        cfg.to_str().unwrap(),
        "--state-dir",
        state.to_str().unwrap(),
        "--clean",
    ]);

    assert!(wait_for_ready(&sock, 5_000), "daemon not ready");

    let results = toge_gui::ipc_client::query(&sock, 1, "foo.txt", 100, 0).unwrap();
    assert_eq!(results.total_count, 1);
    let row = &results.rows[0];
    assert!(row.path.ends_with("foo.txt"));
    assert_eq!(row.name, "foo.txt");
    assert!(row.parent.ends_with("root"));
    assert_eq!(row.extension, "txt");
    assert!(!row.is_dir);
    assert_eq!(row.size, 5);

    cleanup(&dir, &mut child);
}
