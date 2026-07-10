use super::*;
#[cfg(target_os = "linux")]
use crate::sys::FanotifyWatcher;
use std::fs;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

/// A fake watcher for testing higher-level code without touching fanotify.
pub struct FakeWatcher {
    pub watches: Vec<String>,
    pub pending: Vec<WatchEvent>,
}

impl FakeWatcher {
    pub fn new() -> Self {
        Self {
            watches: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn push(&mut self, event: WatchEvent) {
        self.pending.push(event);
    }
}

impl FsWatcher for FakeWatcher {
    fn watch(&mut self, path: &Path) -> io::Result<()> {
        self.watches.push(path.to_string_lossy().to_string());
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> io::Result<()> {
        let s = path.to_string_lossy().to_string();
        self.watches.retain(|w| w != &s);
        Ok(())
    }

    fn poll_events(&mut self) -> io::Result<Vec<WatchEvent>> {
        Ok(std::mem::take(&mut self.pending))
    }
}

#[test]
fn fake_watcher_records_watches() {
    let mut w = FakeWatcher::new();
    w.watch(Path::new("/tmp")).unwrap();
    w.watch(Path::new("/home")).unwrap();
    assert_eq!(w.watches, vec!["/tmp", "/home"]);
}

#[test]
fn fake_watcher_returns_pending_events() {
    let mut w = FakeWatcher::new();
    w.push(WatchEvent::Create {
        path: "/tmp/x".into(),
        is_dir: false,
    });
    let events = w.poll_events().unwrap();
    assert_eq!(events.len(), 1);
    assert!(w.poll_events().unwrap().is_empty());
}

#[test]
fn fake_watcher_unwatch_removes_watch() {
    let mut w = FakeWatcher::new();
    w.watch(Path::new("/tmp/a")).unwrap();
    w.watch(Path::new("/tmp/b")).unwrap();
    assert_eq!(w.watches, vec!["/tmp/a", "/tmp/b"]);

    w.unwatch(Path::new("/tmp/a")).unwrap();
    assert_eq!(w.watches, vec!["/tmp/b"]);
}

#[test]
fn fake_watcher_unwatch_is_idempotent() {
    let mut w = FakeWatcher::new();
    w.watch(Path::new("/tmp/a")).unwrap();
    w.unwatch(Path::new("/tmp/nonexistent")).unwrap();
    assert_eq!(w.watches, vec!["/tmp/a"]);
}

#[test]
fn simulate_delete_event_unwatches_directory() {
    let mut watcher = FakeWatcher::new();
    let mut watched: Vec<String> = Vec::new();

    watcher.watch(Path::new("/project/src")).unwrap();
    watched.push("/project/src".to_string());
    watcher.watch(Path::new("/project/lib")).unwrap();
    watched.push("/project/lib".to_string());
    assert_eq!(watcher.watches.len(), 2);

    let event = WatchEvent::Delete {
        path: "/project/src".into(),
    };

    if let WatchEvent::Delete { path } = &event {
        watched.retain(|w| w != path);
        let _ = watcher.unwatch(Path::new(path));
    }

    assert_eq!(watcher.watches, vec!["/project/lib"]);
    assert_eq!(watched, vec!["/project/lib"]);
}

#[test]
fn simulate_move_event_unwatches_source() {
    let mut watcher = FakeWatcher::new();
    let mut watched: Vec<String> = Vec::new();

    watcher.watch(Path::new("/project/old_dir")).unwrap();
    watched.push("/project/old_dir".to_string());
    assert_eq!(watcher.watches, vec!["/project/old_dir"]);

    let event = WatchEvent::Move {
        from: "/project/old_dir".into(),
        to: "/project/new_dir".into(),
    };

    if let WatchEvent::Move { from, .. } = &event {
        watched.retain(|w| w != from);
        let _ = watcher.unwatch(Path::new(from));
    }

    assert_eq!(watcher.watches, Vec::<String>::new());
    assert_eq!(watched, Vec::<String>::new());
}

#[test]
fn simulate_move_event_watches_destination_if_exists() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old_dir");
    let new_path = dir.path().join("new_dir");
    fs::create_dir(&old_path).unwrap();
    fs::create_dir(&new_path).unwrap();

    let mut watcher = FakeWatcher::new();
    let mut watched: Vec<String> = Vec::new();

    watcher.watch(&old_path).unwrap();
    watched.push(old_path.to_string_lossy().to_string());

    let event = WatchEvent::Move {
        from: old_path.to_string_lossy().to_string(),
        to: new_path.to_string_lossy().to_string(),
    };

    if let WatchEvent::Move { from, to } = &event {
        watched.retain(|w| w != from);
        let _ = watcher.unwatch(Path::new(from));

        if Path::new(to).is_dir() {
            let _ = watcher.watch(Path::new(to));
            watched.push(to.clone());
        }
    }

    assert_eq!(watcher.watches.len(), 1);
    assert!(
        watcher
            .watches
            .contains(&new_path.to_string_lossy().to_string())
    );
    assert_eq!(watched.len(), 1);
    assert!(watched.contains(&new_path.to_string_lossy().to_string()));
}

#[test]
fn simulate_overflow_reindex_does_not_accumulate_watches() {
    let mut watcher = FakeWatcher::new();

    let dirs = vec![
        "/project/src".to_string(),
        "/project/lib".to_string(),
        "/project/test".to_string(),
    ];

    for dir in &dirs {
        watcher.watch(Path::new(dir)).unwrap();
    }
    assert_eq!(watcher.watches.len(), 3);

    let new_dirs = vec![
        "/project/src".to_string(),
        "/project/lib".to_string(),
        "/project/test".to_string(),
        "/project/docs".to_string(),
    ];

    for dir in &dirs {
        let _ = watcher.unwatch(Path::new(dir));
    }
    assert_eq!(watcher.watches.len(), 0);

    for dir in &new_dirs {
        watcher.watch(Path::new(dir)).unwrap();
    }
    assert_eq!(watcher.watches.len(), 4);

    assert!(watcher.watches.contains(&"/project/src".to_string()));
    assert!(watcher.watches.contains(&"/project/docs".to_string()));
}

#[test]
fn simulate_creating_nested_directory_watches_it() {
    let mut watcher = FakeWatcher::new();
    let mut watched: Vec<String> = Vec::new();

    watcher.watch(Path::new("/project")).unwrap();
    watched.push("/project".to_string());

    let event = WatchEvent::Create {
        path: "/project/src".into(),
        is_dir: true,
    };

    if let WatchEvent::Create { path, is_dir: true } = &event {
        let _ = watcher.watch(Path::new(path));
        watched.push(path.clone());
    }

    assert_eq!(watcher.watches.len(), 2);
    assert!(watcher.watches.contains(&"/project".to_string()));
    assert!(watcher.watches.contains(&"/project/src".to_string()));
}

#[test]
fn simulate_file_create_does_not_add_watch() {
    let mut watcher = FakeWatcher::new();
    let mut watched: Vec<String> = Vec::new();

    watcher.watch(Path::new("/project")).unwrap();
    watched.push("/project".to_string());

    let event = WatchEvent::Create {
        path: "/project/file.txt".into(),
        is_dir: false,
    };

    if let WatchEvent::Create { path, is_dir: true } = &event {
        let _ = watcher.watch(Path::new(path));
        watched.push(path.clone());
    }

    assert_eq!(watcher.watches.len(), 1);
    assert_eq!(watched.len(), 1);
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_can_be_constructed() {
    let _ = FanotifyWatcher::new();
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_trait_object() {
    fn takes_watcher(_: &mut dyn FsWatcher) {}
    if let Ok(mut w) = FanotifyWatcher::new() {
        takes_watcher(&mut w);
    }
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_observes_create_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fanotify-test.mkv");

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }

    fs::write(&path, b"hello").unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_create = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.iter().any(|event| {
            matches!(
                event,
                WatchEvent::Create { path: ep, is_dir: false }
                    if ep == path.to_string_lossy().as_ref()
            )
        }) {
            saw_create = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(saw_create, "expected create event for {}", path.display());

    fs::remove_file(&path).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_delete = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.iter().any(|event| {
            matches!(
                event,
                WatchEvent::Delete { path: ep }
                    if ep == path.to_string_lossy().as_ref()
            )
        }) {
            saw_delete = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(saw_delete, "expected delete event for {}", path.display());
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_observes_modify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fanotify-modify.mkv");
    fs::write(&path, b"initial").unwrap();

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }

    // Drain any create events from the initial write above.
    drain_events(&mut watcher, Duration::from_millis(200));

    fs::write(&path, b"updated content").unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_modify = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.iter().any(|event| {
            matches!(
                event,
                WatchEvent::Modify { path: ep }
                    if ep == path.to_string_lossy().as_ref()
            )
        }) {
            saw_modify = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(saw_modify, "expected modify event for {}", path.display());
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_observes_directory_create() {
    let dir = tempfile::tempdir().unwrap();
    let new_dir = dir.path().join("new_subdir");

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }

    fs::create_dir(&new_dir).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_dir_create = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.iter().any(|event| {
            matches!(
                event,
                WatchEvent::Create { path: ep, is_dir: true }
                    if ep == new_dir.to_string_lossy().as_ref()
            )
        }) {
            saw_dir_create = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        saw_dir_create,
        "expected directory create event for {}",
        new_dir.display()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_rename_produces_delete_and_create() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.mkv");
    let new_path = dir.path().join("new.mkv");
    fs::write(&old_path, b"content").unwrap();

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }

    // Drain initial create events.
    drain_events(&mut watcher, Duration::from_millis(200));

    fs::rename(&old_path, &new_path).unwrap();

    let old_str = old_path.to_string_lossy().to_string();
    let new_str = new_path.to_string_lossy().to_string();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_delete = false;
    let mut saw_create = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        for event in &events {
            match event {
                WatchEvent::Delete { path } if path == &old_str => saw_delete = true,
                WatchEvent::Create {
                    path,
                    is_dir: false,
                } if path == &new_str => saw_create = true,
                _ => {}
            }
        }
        if saw_delete && saw_create {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        saw_delete,
        "expected delete event for renamed-from {}",
        old_str
    );
    assert!(
        saw_create,
        "expected create event for renamed-to {}",
        new_str
    );
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_fs_dedup_does_not_double_mark() {
    let dir = tempfile::tempdir().unwrap();
    let sibling = dir.path().join("sibling.mkv");

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }

    let mounts_after_first = watcher.fs_count();
    assert_eq!(mounts_after_first, 1, "first watch should mark one mount");

    // Watching a second path on the same mount must not add another mark.
    watcher.watch(dir.path()).expect("re-watch same path");
    assert_eq!(
        watcher.fs_count(),
        mounts_after_first,
        "re-watching the same mount must not duplicate the mark"
    );

    // Creating a sibling file still produces events — proving the single
    // mount mark covers the whole tree.
    fs::write(&sibling, b"dedup").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.iter().any(|event| {
            matches!(
                event,
                WatchEvent::Create { path: ep, is_dir: false }
                    if ep == sibling.to_string_lossy().as_ref()
            )
        }) {
            saw = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(saw, "expected create event for {}", sibling.display());
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_poll_returns_empty_when_idle() {
    let dir = tempfile::tempdir().unwrap();

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }

    // No filesystem activity — non-blocking poll must return an empty vec.
    let events = watcher.poll_events().unwrap();
    assert!(events.is_empty(), "idle poll should return no events");
}

#[test]
#[cfg(target_os = "linux")]
fn fanotify_watcher_unwatch_is_noop() {
    let dir = tempfile::tempdir().unwrap();

    let Ok(mut watcher) = FanotifyWatcher::new() else {
        return;
    };
    if watcher.watch(dir.path()).is_err() {
        return;
    }
    // fanotify mount marks are not removed per-path; unwatch must not error.
    watcher
        .unwatch(dir.path())
        .expect("unwatch should not error");
}

#[cfg(target_os = "linux")]
fn drain_events(watcher: &mut FanotifyWatcher, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.is_empty() {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
