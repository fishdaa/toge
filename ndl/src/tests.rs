use std::process::Command;

fn run_ndl(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "ndl", "--"])
        .args(args)
        .output()
        .expect("failed to run ndl")
}

#[test]
fn ndl_help_exits_zero() {
    let output = run_ndl(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ndl") || stdout.contains("Search options"));
    assert!(output.status.success());
}

#[test]
fn ndl_version_prints_version() {
    let output = run_ndl(&["-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ndl 0.1.1"));
    assert!(output.status.success());
}

#[test]
fn render_csv_adds_header_and_crlf() {
    let output = super::render_results(
        &["/tmp/foo.txt".into()],
        needle_core::opts::OutputFormat::Csv,
        false,
    );
    assert_eq!(output, "Name\r\n\"/tmp/foo.txt\"\r\n");
}

#[test]
fn render_csv_without_header_omits_header_row() {
    let output = super::render_results(
        &["/tmp/foo.txt".into()],
        needle_core::opts::OutputFormat::Csv,
        true,
    );
    assert_eq!(output, "\"/tmp/foo.txt\"\r\n");
}

#[test]
fn render_tsv_without_header_omits_header_row() {
    let output = super::render_results(
        &["/tmp/foo.txt".into()],
        needle_core::opts::OutputFormat::Tsv,
        true,
    );
    assert_eq!(output, "/tmp/foo.txt\n");
}

#[test]
fn render_default_joins_with_newlines() {
    let output = super::render_results(
        &["/tmp/foo.txt".into(), "/tmp/bar.txt".into()],
        needle_core::opts::OutputFormat::Default,
        false,
    );
    assert_eq!(output, "/tmp/foo.txt\n/tmp/bar.txt\n");
}
