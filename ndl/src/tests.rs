use std::process::Command;

fn run_ndl(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--bin", "ndl", "--"])
        .args(args)
        .output()
        .expect("failed to run ndl")
}

#[test]
fn ndl_binary_prints_not_implemented() {
    let output = run_ndl(&["foo"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not yet implemented"));
    assert!(!output.status.success());
}
