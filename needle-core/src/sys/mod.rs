//! Platform abstraction layer for filesystem watching.

use std::io;
use std::path::Path;

/// A filesystem event produced by a watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    Create { path: String, is_dir: bool },
    Delete { path: String },
    Modify { path: String },
    Move { from: String, to: String },
    Overflow { path: String },
}

/// Abstract filesystem watcher.
pub trait FsWatcher: Send {
    fn watch(&mut self, path: &Path) -> io::Result<()>;
    fn unwatch(&mut self, path: &Path) -> io::Result<()>;
    fn poll_events(&mut self) -> io::Result<Vec<WatchEvent>>;
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::InotifyWatcher;

#[cfg(test)]
mod tests;
