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
fn needled_binary_prints_not_implemented() {
    let output = run_needled(&[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not yet implemented"));
    assert!(!output.status.success());
}
