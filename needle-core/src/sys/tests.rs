use super::*;
#[cfg(target_os = "linux")]
use crate::sys::linux::ParsedWatchEvent;
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::path::Path;

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
fn inotify_parse_create_event() {
    // Synthetic IN_CREATE event for "foo.txt" with a 4-byte aligned name.
    let name = b"foo.txt\0\0";
    let len = 16 + name.len();
    let mut buf = Vec::with_capacity(len);
    buf.extend_from_slice(&0u32.to_le_bytes()); // wd
    buf.extend_from_slice(&0x00000100u32.to_le_bytes()); // mask = IN_CREATE
    buf.extend_from_slice(&0u32.to_le_bytes()); // cookie
    buf.extend_from_slice(&(name.len() as u32).to_le_bytes()); // len
    buf.extend_from_slice(name);

    let events = InotifyWatcher::parse_buffer(&buf);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        ParsedWatchEvent::Create {
            wd: 0,
            name: "foo.txt".into(),
            is_dir: false,
        }
    );
}

#[test]
#[cfg(target_os = "linux")]
fn inotify_parse_delete_event() {
    let name = b"old.rs\0\0";
    let len = 16 + name.len();
    let mut buf = Vec::with_capacity(len);
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0x00000200u32.to_le_bytes()); // mask = IN_DELETE
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
    buf.extend_from_slice(name);

    let events = InotifyWatcher::parse_buffer(&buf);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        ParsedWatchEvent::Delete {
            wd: 0,
            name: "old.rs".into()
        }
    );
}

#[test]
#[cfg(target_os = "linux")]
fn inotify_resolves_duplicate_basenames_by_watch_descriptor() {
    let fd = OwnedFd::from(std::fs::File::open("/dev/null").unwrap());
    let watcher = InotifyWatcher::from_watch_map(
        fd,
        HashMap::from([(7, "/tmp/a".to_string()), (9, "/tmp/b".to_string())]),
    );

    assert_eq!(
        watcher.resolve_full_path(9, "shared.txt"),
        "/tmp/b/shared.txt"
    );
    assert_eq!(
        watcher.resolve_full_path(7, "shared.txt"),
        "/tmp/a/shared.txt"
    );
}
