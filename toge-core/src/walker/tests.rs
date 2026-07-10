use super::*;
use std::fs;
use std::path::{Path, PathBuf};

fn visible_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("workspace-")
        .tempdir_in(std::env::temp_dir())
        .unwrap();
    let root = dir.path().to_path_buf();
    (dir, root)
}

fn temp_dir_with_files() -> (tempfile::TempDir, Vec<String>) {
    let (dir, root) = visible_root();
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
    (
        dir,
        paths
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    )
}

#[test]
fn test_walk_indexes_all_files_and_dirs() {
    let (dir, _paths) = temp_dir_with_files();
    let mut idx = Index::new();
    let count = walk(dir.path(), &mut idx, &Excludes::new(), false);
    assert!(
        count >= 6,
        "expected at least 4 files + 2 dirs, got {}",
        count
    );
    assert_eq!(idx.count(), count);

    let txt_ids = idx.search_substring("foo.txt");
    assert_eq!(txt_ids.len(), 1);

    let mp3_ids = idx.search_substring("music.mp3");
    assert_eq!(mp3_ids.len(), 1);
}

#[test]
fn test_walk_always_skips_hidden_directories() {
    let (_dir, root) = visible_root();
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden").join("secret.txt"), "x").unwrap();
    fs::write(root.join("visible.txt"), "x").unwrap();

    let mut idx_all = Index::new();
    walk(&root, &mut idx_all, &Excludes::new(), false);
    assert!(idx_all.search_substring("secret.txt").is_empty());
    assert!(!idx_all.search_substring("visible.txt").is_empty());
}

#[test]
fn test_walk_skips_hidden_files_only_when_configured() {
    let (_dir, root) = visible_root();
    fs::write(root.join(".secret.txt"), "x").unwrap();
    fs::write(root.join("visible.txt"), "x").unwrap();

    let mut idx_all = Index::new();
    walk(&root, &mut idx_all, &Excludes::new(), false);
    assert!(!idx_all.search_substring("secret.txt").is_empty());

    let mut idx_hidden = Index::new();
    let mut ex = Excludes::new();
    ex.skip_hidden = true;
    walk(&root, &mut idx_hidden, &ex, false);
    assert!(idx_hidden.search_substring("secret.txt").is_empty());
    assert!(!idx_hidden.search_substring("visible.txt").is_empty());
}

#[test]
fn test_walk_skips_pattern_matches() {
    let (_dir, root) = visible_root();
    fs::write(root.join("keep.txt"), "x").unwrap();
    fs::write(root.join("drop.tmp"), "x").unwrap();
    fs::write(root.join("drop.swp"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.patterns = vec!["*.tmp".into(), "*.swp".into()];

    let mut idx = Index::new();
    walk(&root, &mut idx, &ex, false);
    assert!(!idx.search_substring("keep.txt").is_empty());
    assert!(idx.search_substring("drop.tmp").is_empty());
    assert!(idx.search_substring("drop.swp").is_empty());
}

#[test]
fn test_walk_skips_folder_patterns() {
    let (_dir, root) = visible_root();
    fs::create_dir(root.join("node_modules")).unwrap();
    fs::write(root.join("node_modules").join("pkg.js"), "x").unwrap();
    fs::write(root.join("app.js"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.folders = vec!["**/node_modules".into()];

    let mut idx = Index::new();
    walk(&root, &mut idx, &ex, false);
    assert!(idx.search_substring("pkg.js").is_empty());
    assert!(!idx.search_substring("app.js").is_empty());
}

#[test]
fn test_walk_skips_explicit_paths() {
    let (_dir, root) = visible_root();
    let trash = root.join(".local").join("share").join("Trash");
    fs::create_dir_all(trash.join("files")).unwrap();
    fs::write(trash.join("files").join("trashed.mkv"), "x").unwrap();
    fs::write(root.join("kept.mkv"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.paths = vec![trash];

    let mut idx = Index::new();
    walk(&root, &mut idx, &ex, false);
    assert!(idx.search_substring("trashed.mkv").is_empty());
    assert!(!idx.search_substring("kept.mkv").is_empty());
}

#[test]
fn test_walk_include_only_restricts_to_patterns() {
    let (_dir, root) = visible_root();
    fs::write(root.join("a.txt"), "x").unwrap();
    fs::write(root.join("b.rs"), "x").unwrap();

    let mut ex = Excludes::new();
    ex.include_only = vec!["*.rs".into()];

    let mut idx = Index::new();
    walk(&root, &mut idx, &ex, false);
    assert!(idx.search_substring("a.txt").is_empty());
    assert!(!idx.search_substring("b.rs").is_empty());
}

#[test]
fn test_walk_without_metadata_leaves_fields_zeroed() {
    let (_dir, root) = visible_root();
    fs::write(root.join("file.txt"), "hello").unwrap();

    let mut idx = Index::new();
    walk(&root, &mut idx, &Excludes::new(), false);

    let id = idx.search_substring("file.txt")[0] as usize;
    let entry = &idx.entries[id];
    assert_eq!(entry.size, 0);
    assert_eq!(entry.modified, 0);
    assert_eq!(entry.created, 0);
    assert_eq!(entry.accessed, 0);
}

#[test]
fn test_walk_with_metadata_populates_file_fields() {
    let (_dir, root) = visible_root();
    fs::write(root.join("file.txt"), "hello").unwrap();

    let mut idx = Index::new();
    walk(&root, &mut idx, &Excludes::new(), true);

    let id = idx.search_substring("file.txt")[0] as usize;
    let entry = &idx.entries[id];
    assert_eq!(entry.size, 5);
    assert!(entry.modified > 0);
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

#[cfg(unix)]
#[test]
fn test_walk_skips_symlink_entries() {
    use std::os::unix::fs::symlink;

    let (_dir, root) = visible_root();
    fs::create_dir(root.join("real")).unwrap();
    fs::write(root.join("real").join("inside.txt"), "x").unwrap();
    symlink(root.join("real"), root.join("linked-real")).unwrap();
    symlink(
        root.join("real").join("inside.txt"),
        root.join("linked-file"),
    )
    .unwrap();

    let mut idx = Index::new();
    walk(&root, &mut idx, &Excludes::new(), true);

    assert!(idx.search_substring("linked-real").is_empty());
    assert!(idx.search_substring("linked-file").is_empty());
    assert!(!idx.search_substring("inside.txt").is_empty());
}
