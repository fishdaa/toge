use std::io::Cursor;
use std::process::Command;

fn run_ndl(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "toge", "--"])
        .args(args)
        .output()
        .expect("failed to run toge")
}

#[test]
fn ndl_help_exits_zero() {
    let output = run_ndl(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("toge") || stdout.contains("Search options"));
    assert!(output.status.success());
}

#[test]
fn ndl_version_prints_version() {
    let output = run_ndl(&["-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("toge 0.1.1"));
    assert!(output.status.success());
}

#[test]
fn render_csv_adds_header_and_crlf() {
    let output = super::render_results(
        &["/tmp/foo.txt".into()],
        toge_core::opts::OutputFormat::Csv,
        false,
    );
    assert_eq!(output, "Name\r\n\"/tmp/foo.txt\"\r\n");
}

#[test]
fn render_csv_without_header_omits_header_row() {
    let output = super::render_results(
        &["/tmp/foo.txt".into()],
        toge_core::opts::OutputFormat::Csv,
        true,
    );
    assert_eq!(output, "\"/tmp/foo.txt\"\r\n");
}

#[test]
fn render_tsv_without_header_omits_header_row() {
    let output = super::render_results(
        &["/tmp/foo.txt".into()],
        toge_core::opts::OutputFormat::Tsv,
        true,
    );
    assert_eq!(output, "/tmp/foo.txt\n");
}

#[test]
fn render_default_joins_with_newlines() {
    let output = super::render_results(
        &["/tmp/foo.txt".into(), "/tmp/bar.txt".into()],
        toge_core::opts::OutputFormat::Default,
        false,
    );
    assert_eq!(output, "/tmp/foo.txt\n/tmp/bar.txt\n");
}

#[test]
fn read_response_rejects_large_payloads() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&((toge_core::ipc::MAX_IPC_MESSAGE_SIZE + 1) as u64).to_le_bytes());
    let mut reader = Cursor::new(bytes);

    let err = super::read_response_from(&mut reader).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("response too large"));
}

#[test]
fn read_response_rejects_malformed_body() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.push(99);
    let mut reader = Cursor::new(bytes);

    let err = super::read_response_from(&mut reader).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("unknown response type"));
}
