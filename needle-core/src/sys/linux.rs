use super::{FsWatcher, WatchEvent};
use std::io;
use std::path::Path;

/// Linux inotify-based filesystem watcher.
pub struct InotifyWatcher;

impl InotifyWatcher {
    pub fn new() -> io::Result<Self> {
        todo!()
    }
}

impl InotifyWatcher {
    /// Parse a raw inotify event buffer into structured events.
    /// Exposed for unit testing the parsing logic without touching the kernel.
    pub fn parse_buffer(buf: &[u8]) -> Vec<WatchEvent> {
        let _ = buf;
        todo!()
    }
}

impl FsWatcher for InotifyWatcher {
    fn watch(&mut self, _path: &Path) -> io::Result<()> {
        todo!()
    }

    fn unwatch(&mut self, _path: &Path) -> io::Result<()> {
        todo!()
    }

    fn poll_events(&mut self) -> io::Result<Vec<WatchEvent>> {
        todo!()
    }
}
