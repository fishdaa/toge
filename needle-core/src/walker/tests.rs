use super::*;
use std::fs;
use std::path::Path;

fn temp_dir_with_files() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("docs")).unwrap();
    fs::create_dir(root.join("docs").join("sub")).unwrap();
    fs::write(root.join("docs").join("foo.txt"), "hello").unwrap();
    fs::write(root.join("docs").join("bar.rs"), "fn main() {}").unwrap();
    fs::write(root.join("docs").join("sub").join("baz.md"), "# x").unwrap();
    fs::write(root.join("music.mp3"), "").unwrap();

    let paths = vec![
        root.join("docs").join("foo.txt"),
        root.join("docs").join("bar.rs"),
        root.join("docs").join("sub").join("baz.md"),
        root.join("music.mp3"),
    ];
    (dir, paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect())
}

#[test]
fn test_walk_indexes_all_files_and_dirs() {
    let (dir, _paths) = temp_dir_with_files();
    let mut idx = Index::new();
    let count = walk(dir.path(), &mut idx, &Excludes::new());
    assert!(count >= 6, "expected at least 4 files + 2 dirs, got {}", count);
    assert_eq!(idx.count(), count);

    let txt_ids = idx.search_substring("foo.txt");
    assert_eq!(txt_ids.len(), 1);

    let mp3_ids = idx.search_substring("music.mp3");
    assert_eq!(mp3_ids.len(), 1);
}

#[test]
fn test_walk_skips_hidden_when_configured() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".hidden")).unwrap();
    fs::write(dir.path().join(".hidden").join("secret.txt"), "x").unwrap();
    fs::write(dir.path().join("visible.txt"), "x").unwrap();

    let mut idx_all = Index::new();
    walk(dir.path(), &mut idx_all, &Excludes::new());
    assert!(!idx_all.search_substring("secret.txt").is_empty());

    let mut idx_hidden = Index::new();
    let mut ex = Excludes::new();
    ex.skip_hidden = true;
    walk(dir.path(), &mut idx_hidden, &ex);
    assert!(idx_hidden.search_substring("secret.txt").is_empty());
    assert!(!idx_hidden.search_substring("visible.txt").is_empty());
}

#[test]
fn test_walk_skips_pattern_matches() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("keep.txt"), "x").unwrap();
    fs::write(dir.path().join("drop.tmp"), "x").unwrap();
    fs::write(dir.path().join("drop.swp"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.patterns = vec!["*.tmp".into(), "*.swp".into()];

    let mut idx = Index::new();
    walk(dir.path(), &mut idx, &ex);
    assert!(!idx.search_substring("keep.txt").is_empty());
    assert!(idx.search_substring("drop.tmp").is_empty());
    assert!(idx.search_substring("drop.swp").is_empty());
}

#[test]
fn test_walk_skips_folder_patterns() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules").join("pkg.js"), "x").unwrap();
    fs::write(dir.path().join("app.js"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.folders = vec!["**/node_modules".into()];

    let mut idx = Index::new();
    walk(dir.path(), &mut idx, &ex);
    assert!(idx.search_substring("pkg.js").is_empty());
    assert!(!idx.search_substring("app.js").is_empty());
}

#[test]
fn test_walk_include_only_restricts_to_patterns() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "x").unwrap();
    fs::write(dir.path().join("b.rs"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.include_only = vec!["*.rs".into()];

    let mut idx = Index::new();
    walk(dir.path(), &mut idx, &ex);
    assert!(idx.search_substring("a.txt").is_empty());
    assert!(!idx.search_substring("b.rs").is_empty());
}

#[test]
fn test_excludes_system_paths() {
    let ex = Excludes {
        skip_system_paths: true,
        ..Excludes::new()
    };
    assert!(ex.is_excluded(Path::new("/proc")));
    assert!(ex.is_excluded(Path::new("/sys")));
    assert!(ex.is_excluded(Path::new("/dev")));
    assert!(!ex.is_excluded(Path::new("/home/user")));
}
