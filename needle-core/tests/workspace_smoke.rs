//! Cross-crate integration smoke tests.

use std::process::Command;

#[test]
fn cargo_build_workspace_succeeds() {
    let output = Command::new("cargo")
        .args(["build", "--workspace"])
        .output()
        .expect("cargo build failed");
    assert!(
        output.status.success(),
        "workspace build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn binaries_exist() {
    let output = Command::new("cargo")
        .args(["build", "--workspace"])
        .output()
        .expect("cargo build failed");
    assert!(output.status.success());

    let needle_core_manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(needle_core_manifest).parent().unwrap();
    let target = format!("{}/target/debug", workspace_root.display());
    assert!(std::path::Path::new(&format!("{}/needled", target)).exists());
    assert!(std::path::Path::new(&format!("{}/ndl", target)).exists());
}
