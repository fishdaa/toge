//! Phase 7/8 CLI display and export integration tests.

use std::process::Command;

fn run_ndl(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "ndl", "--"])
        .args(args)
        .output()
        .expect("failed to run ndl")
}

#[test]
#[ignore = "integration test requires implemented daemon"]
fn ndl_csv_output_has_header_and_crlf() {
    let output = run_ndl(&["-csv", "foo"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Name"));
    assert!(stdout.contains("\r\n"));
}

#[test]
#[ignore = "integration test requires implemented daemon"]
fn ndl_get_result_count_prints_number_only() {
    let output = run_ndl(&["-get-result-count", "foo"]);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.parse::<usize>().is_ok());
}

#[test]
#[ignore = "integration test requires implemented daemon"]
fn ndl_export_csv_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.csv");
    let path_str = path.to_str().unwrap();
    let output = run_ndl(&["-export-csv", path_str, "foo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(path.exists());
}
