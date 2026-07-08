use super::*;
#[cfg(target_os = "linux")]
use notify::event::{EventAttributes, ModifyKind, RenameMode};
#[cfg(target_os = "linux")]
use notify::{Event, EventKind};
#[cfg(target_os = "linux")]
use std::fs;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

/// A fake watcher for testing higher-level code without touching inotify.
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
#[cfg(target_os = "linux")]
fn inotify_watcher_can_be_constructed() {
    // May fail inside restricted containers; allow it.
    let _ = InotifyWatcher::new();
}

#[test]
#[cfg(target_os = "linux")]
fn inotify_watcher_trait_object() {
    fn takes_watcher(_: &mut dyn FsWatcher) {}
    if let Ok(mut w) = InotifyWatcher::new() {
        takes_watcher(&mut w);
    }
}

#[test]
#[cfg(target_os = "linux")]
fn notify_maps_rename_from_to_delete() {
    let events = InotifyWatcher::map_event(Event {
        kind: EventKind::Modify(ModifyKind::Name(RenameMode::From)),
        paths: vec!["/tmp/movie.mkv".into()],
        attrs: EventAttributes::new(),
    });

    assert_eq!(
        events,
        vec![WatchEvent::Delete {
            path: "/tmp/movie.mkv".into()
        }]
    );
}

#[test]
#[cfg(target_os = "linux")]
fn notify_maps_rename_both_to_move() {
    let events = InotifyWatcher::map_event(Event {
        kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        paths: vec!["/tmp/movie.part".into(), "/tmp/movie.mkv".into()],
        attrs: EventAttributes::new(),
    });

    assert_eq!(
        events,
        vec![WatchEvent::Move {
            from: "/tmp/movie.part".into(),
            to: "/tmp/movie.mkv".into()
        }]
    );
}

#[test]
#[cfg(target_os = "linux")]
fn inotify_watcher_observes_real_create_and_delete_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live-watch.mkv");

    let Ok(mut watcher) = InotifyWatcher::new() else {
        return;
    };
    watcher.watch(dir.path()).expect("watch temp dir");

    fs::write(&path, b"hello").unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_create = false;
    while Instant::now() < deadline {
        let events = watcher.poll_events().unwrap();
        if events.iter().any(|event| {
            matches!(
                event,
                WatchEvent::Create { path: event_path, is_dir: false }
                    if event_path == path.to_string_lossy().as_ref()
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
                WatchEvent::Delete { path: event_path }
                    if event_path == path.to_string_lossy().as_ref()
            )
        }) {
            saw_delete = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(saw_delete, "expected delete event for {}", path.display());
}
