use super::*;
use std::io::Write;
use std::path::Path;

#[test]
fn test_default_config_values() {
    let cfg = Config::default_config();
    let expected_home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));
    match expected_home {
        Some(home) => assert_eq!(cfg.roots, vec![home]),
        None => assert!(cfg.roots.is_empty()),
    }
    assert_eq!(cfg.poll_interval_secs, 300);
    assert_eq!(cfg.operator_precedence, OperatorOrder::OrAnd);
    assert!(cfg.index_size);
    assert!(!cfg.index_date_created);
    assert!(!cfg.exclude_hidden);
}

#[test]
fn test_load_config_from_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(
        br#"
[index]
size = true
date_modified = true

[roots]
include = ["/home", "/data"]
exclude_fstypes = ["tmpfs", "nfs4"]

[exclude]
hidden_files = true
patterns = ["*.tmp"]
folders = ["**/node_modules"]
"#,
    )
    .unwrap();

    let cfg = Config::load(&path).unwrap();
    assert!(cfg.index_size);
    assert!(cfg.index_date_modified);
    assert!(cfg.exclude_hidden);
    assert_eq!(cfg.roots, vec![Path::new("/home"), Path::new("/data")]);
    assert!(cfg.exclude_fstypes.contains(&"tmpfs".to_string()));
    assert!(cfg.exclude_patterns.contains(&"*.tmp".to_string()));
    assert!(cfg.exclude_folders.contains(&"**/node_modules".to_string()));
}

#[test]
fn test_load_missing_config_falls_back_to_defaults() {
    let cfg = Config::load(Path::new("/nonexistent/needle/config.toml")).unwrap();
    assert_eq!(cfg, Config::default_config());
}

#[test]
fn test_keyboard_config_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = Config::default_config();
    cfg.keyboard.new_window_hotkey = "Ctrl+N".to_string();
    cfg.keyboard.command_shortcuts = vec![KeyboardShortcutConfig {
        command_id: "window.open_options".to_string(),
        scope: KeyboardScope::Global,
        accelerator: "Ctrl+Comma".to_string(),
    }];

    cfg.save(&path).unwrap();

    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.keyboard.new_window_hotkey, "Ctrl+N");
    assert_eq!(loaded.keyboard.command_shortcuts.len(), 1);
    assert_eq!(
        loaded.keyboard.command_shortcuts[0].command_id,
        "window.open_options"
    );
}
