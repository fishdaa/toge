use std::process::Command;

/// Helper to build and run the daemon binary with given args.
fn run_needled(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "needled", "--"])
        .args(args)
        .output()
        .expect("failed to run needled")
}

#[test]
fn needled_help_exits_zero() {
    let output = run_needled(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("needled") || stdout.contains("Options"));
    assert!(output.status.success());
}

#[test]
fn needled_version_prints_version() {
    let output = run_needled(&["-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("needled 0.1.0"));
    assert!(output.status.success());
}
