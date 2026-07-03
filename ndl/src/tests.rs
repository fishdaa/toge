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
    assert!(stdout.contains("ndl 0.1.0"));
    assert!(output.status.success());
}
