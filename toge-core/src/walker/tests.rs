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
        "expected at least 4 files + 2 dirs, got {count}"
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
fn test_reconcile_repairs_offline_add_delete_and_rename_drift() {
    let (_dir, root) = visible_root();
    let removed = root.join("removed.txt");
    let renamed_from = root.join("before.txt");
    fs::write(&removed, "old").unwrap();
    fs::write(&renamed_from, "rename me").unwrap();

    let mut idx = Index::new();
    walk(&root, &mut idx, &Excludes::new(), false);

    fs::remove_file(&removed).unwrap();
    let renamed_to = root.join("after.txt");
    fs::rename(&renamed_from, &renamed_to).unwrap();
    let added = root.join("added.txt");
    fs::write(&added, "new").unwrap();

    reconcile(&[root], &mut idx, &Excludes::new(), false);

    assert!(idx.search_substring("removed.txt").is_empty());
    assert!(idx.search_substring("before.txt").is_empty());
    assert_eq!(idx.search_substring("after.txt").len(), 1);
    assert_eq!(idx.search_substring("added.txt").len(), 1);
}

#[test]
fn test_reconcile_refreshes_metadata_changed_while_offline() {
    let (_dir, root) = visible_root();
    let file = root.join("file.txt");
    fs::write(&file, "a").unwrap();

    let mut idx = Index::new();
    walk(&root, &mut idx, &Excludes::new(), true);
    fs::write(&file, "a longer value").unwrap();

    reconcile(&[root], &mut idx, &Excludes::new(), true);

    let id = idx.search_substring("file.txt")[0] as usize;
    assert_eq!(idx.entries[id].size, 14);
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

#[test]
fn reconcile_tracks_seen_entries_when_file_types_change_and_ids_move() {
    let (_dir, root) = visible_root();
    let changed = root.join("changed");
    let kept = root.join("kept.txt");
    fs::create_dir(&changed).unwrap();
    fs::write(&kept, "keep").unwrap();
    // Deliberately put the kept entry last, so replacing changed moves its ID.
    let mut idx = Index::new();
    idx.insert(changed.to_str().unwrap(), false);
    idx.insert(root.join("stale.txt").to_str().unwrap(), false);
    idx.insert(kept.to_str().unwrap(), false);
    reconcile(&[root.clone(), root], &mut idx, &Excludes::new(), false);
    assert_eq!(idx.count(), 2);
    assert!(idx.entries[idx.id_by_path(changed.to_str().unwrap()).unwrap() as usize].is_dir);
    assert_eq!(idx.search_substring("kept").len(), 1);
    assert!(idx.search_substring("stale").is_empty());
}

#[test]
fn reconcile_preserves_already_seen_entry_swapped_by_a_type_change() {
    let (_dir, root) = visible_root();
    let first = root.join("first");
    let second = root.join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let kept = first.join("kept.txt");
    let changed = second.join("changed");
    fs::write(&kept, "keep").unwrap();
    fs::write(&changed, "now a file").unwrap();
    let mut idx = Index::new();
    idx.insert(changed.to_str().unwrap(), true);
    idx.insert(kept.to_str().unwrap(), false);
    reconcile(&[first, second], &mut idx, &Excludes::new(), false);
    assert_eq!(idx.count(), 2);
    assert_eq!(idx.search_substring("kept").len(), 1);
    assert!(!idx.entries[idx.id_by_path(changed.to_str().unwrap()).unwrap() as usize].is_dir);
}

#[test]
fn reconcile_live_keeps_entries_added_by_other_writers_during_the_walk() {
    let (_dir, root) = visible_root();
    for i in 0..(RECONCILE_BATCH + 10) {
        fs::write(root.join(format!("f{i}.txt")), "").unwrap();
    }
    let mut idx = Index::new();
    idx.insert(root.join("stale.txt").to_str().unwrap(), false);

    let late = root.join("late.txt");
    let mut steps = 0;
    reconcile_live(
        std::slice::from_ref(&root),
        &Excludes::new(),
        false,
        |step| {
            steps += 1;
            if steps == 2 {
                // A watcher indexes a file created after the walk passed its directory.
                fs::write(&late, "").unwrap();
                idx.insert(late.to_str().unwrap(), false);
            }
            step(&mut idx);
            true
        },
    );

    assert_eq!(idx.search_substring("late.txt").len(), 1);
    assert!(idx.search_substring("stale.txt").is_empty());
    assert_eq!(idx.count(), RECONCILE_BATCH + 11);
}

#[test]
fn reconcile_live_stops_once_the_index_is_replaced() {
    let (_dir, root) = visible_root();
    for i in 0..(RECONCILE_BATCH * 3) {
        fs::write(root.join(format!("f{i}.txt")), "").unwrap();
    }
    let mut idx = Index::new();
    idx.insert(root.join("stale.txt").to_str().unwrap(), false);

    let mut steps = 0;
    reconcile_live(&[root], &Excludes::new(), false, |step| {
        steps += 1;
        if steps > 1 {
            return false;
        }
        step(&mut idx);
        true
    });

    assert_eq!(steps, 2);
    assert_eq!(idx.count(), RECONCILE_BATCH + 1);
    assert_eq!(idx.search_substring("stale.txt").len(), 1);
}

#[test]
fn reconcile_drops_entries_under_newly_excluded_folders() {
    let (_dir, root) = visible_root();
    fs::create_dir(root.join("build")).unwrap();
    fs::write(root.join("build").join("out.o"), "").unwrap();
    let mut idx = Index::new();
    walk(&root, &mut idx, &Excludes::new(), false);
    assert_eq!(idx.search_substring("out.o").len(), 1);

    let excludes = Excludes {
        folders: vec!["build".into()],
        ..Excludes::new()
    };
    reconcile(&[root], &mut idx, &excludes, false);

    assert!(idx.search_substring("out.o").is_empty());
}

#[test]
fn excluded_under_roots_checks_every_ancestor_below_the_root() {
    let roots = [PathBuf::from("/home/u")];
    let excludes = Excludes {
        folders: vec!["target".into()],
        ..Default::default()
    };
    assert!(excluded_under_roots(
        Path::new("/home/u/p/target/debug/a.o"),
        &roots,
        &excludes
    ));
    assert!(excluded_under_roots(
        Path::new("/home/u/p/target"),
        &roots,
        &excludes
    ));
    assert!(!excluded_under_roots(
        Path::new("/home/u/p/src/target.rs"),
        &roots,
        &excludes
    ));
    assert!(!excluded_under_roots(
        Path::new("/elsewhere/target/x"),
        &roots,
        &excludes
    ));
    assert!(!excluded_under_roots(
        Path::new("/home/u"),
        &roots,
        &excludes
    ));
}
