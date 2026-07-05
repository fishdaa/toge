use super::{FsWatcher, WatchEvent};
use std::collections::HashMap;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const IN_CREATE: u32 = 0x0000_0100;
const IN_DELETE: u32 = 0x0000_0200;
const IN_MODIFY: u32 = 0x0000_0002;
const IN_MOVED_FROM: u32 = 0x0000_0040;
const IN_MOVED_TO: u32 = 0x0000_0080;
const IN_ISDIR: u32 = 0x4000_0000;
const IN_Q_OVERFLOW: u32 = 0x0000_4000;
const IN_ALL_EVENTS: u32 = IN_CREATE | IN_DELETE | IN_MODIFY | IN_MOVED_FROM | IN_MOVED_TO;

const IN_NONBLOCK: i32 = 0x0000_0800;
const IN_CLOEXEC: i32 = 0x0000_8000;

extern "C" {
    fn inotify_init1(flags: i32) -> i32;
    fn inotify_add_watch(fd: i32, pathname: *const u8, mask: u32) -> i32;
    fn inotify_rm_watch(fd: i32, wd: i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
}

/// Linux inotify-based filesystem watcher.
pub struct InotifyWatcher {
    fd: OwnedFd,
    watches: HashMap<i32, String>, // wd -> dir path
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParsedWatchEvent {
    Create {
        wd: i32,
        name: String,
        is_dir: bool,
    },
    Delete {
        wd: i32,
        name: String,
    },
    Modify {
        wd: i32,
        name: String,
    },
    Move {
        from_wd: i32,
        from_name: String,
        to_wd: i32,
        to_name: String,
    },
    Overflow,
}

impl InotifyWatcher {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            watches: HashMap::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_watch_map(fd: OwnedFd, watches: HashMap<i32, String>) -> Self {
        Self { fd, watches }
    }

    fn resolve_paths(&self, events: Vec<ParsedWatchEvent>) -> Vec<WatchEvent> {
        events
            .into_iter()
            .map(|ev| match &ev {
                ParsedWatchEvent::Create { wd, name, is_dir } => {
                    let full = self.resolve_full_path(*wd, name);
                    WatchEvent::Create {
                        path: full,
                        is_dir: *is_dir,
                    }
                }
                ParsedWatchEvent::Delete { wd, name } => WatchEvent::Delete {
                    path: self.resolve_full_path(*wd, name),
                },
                ParsedWatchEvent::Modify { wd, name } => WatchEvent::Modify {
                    path: self.resolve_full_path(*wd, name),
                },
                ParsedWatchEvent::Move {
                    from_wd,
                    from_name,
                    to_wd,
                    to_name,
                } => WatchEvent::Move {
                    from: self.resolve_full_path(*from_wd, from_name),
                    to: self.resolve_full_path(*to_wd, to_name),
                },
                ParsedWatchEvent::Overflow => WatchEvent::Overflow {
                    path: String::new(),
                },
            })
            .collect()
    }

    pub(crate) fn resolve_full_path(&self, wd: i32, name: &str) -> String {
        if name.is_empty() {
            return String::new();
        }
        if name.starts_with('/') {
            return name.to_string();
        }
        if let Some(dir) = self.watches.get(&wd) {
            return format!("{}/{}", dir, name);
        }
        name.to_string()
    }

    /// Parse a raw inotify event buffer into structured events.
    /// Exposed for unit testing the parsing logic without touching the kernel.
    pub(crate) fn parse_buffer(buf: &[u8]) -> Vec<ParsedWatchEvent> {
        let mut events = Vec::new();
        let mut offset = 0;
        let mut move_from: Option<(u32, i32, String)> = None;

        while offset + 16 <= buf.len() {
            let wd = i32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
            let mask = u32::from_le_bytes([
                buf[offset + 4],
                buf[offset + 5],
                buf[offset + 6],
                buf[offset + 7],
            ]);
            let cookie = u32::from_le_bytes([
                buf[offset + 8],
                buf[offset + 9],
                buf[offset + 10],
                buf[offset + 11],
            ]);
            let len = u32::from_le_bytes([
                buf[offset + 12],
                buf[offset + 13],
                buf[offset + 14],
                buf[offset + 15],
            ]) as usize;
            offset += 16;

            if mask & IN_Q_OVERFLOW != 0 {
                events.push(ParsedWatchEvent::Overflow);
                continue;
            }

            let name = if len > 0 && offset + len <= buf.len() {
                let bytes = &buf[offset..offset + len];
                let bytes = bytes.split(|&b| b == 0).next().unwrap_or(bytes);
                String::from_utf8_lossy(bytes).into_owned()
            } else {
                String::new()
            };
            offset += len;

            let is_dir = mask & IN_ISDIR != 0;

            if mask & IN_CREATE != 0 {
                events.push(ParsedWatchEvent::Create { wd, name, is_dir });
            } else if mask & IN_DELETE != 0 {
                events.push(ParsedWatchEvent::Delete { wd, name });
            } else if mask & IN_MODIFY != 0 {
                events.push(ParsedWatchEvent::Modify { wd, name });
            } else if mask & IN_MOVED_FROM != 0 {
                move_from = Some((cookie, wd, name));
            } else if mask & IN_MOVED_TO != 0 {
                if let Some((from_cookie, from_wd, from_name)) = move_from.take() {
                    if from_cookie == cookie && !from_name.is_empty() {
                        events.push(ParsedWatchEvent::Move {
                            from_wd,
                            from_name,
                            to_wd: wd,
                            to_name: name,
                        });
                    }
                }
            }
        }

        events
    }
}

impl FsWatcher for InotifyWatcher {
    fn watch(&mut self, path: &Path) -> io::Result<()> {
        let path_c = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains null byte"))?;
        let wd = unsafe {
            inotify_add_watch(
                self.fd.as_raw_fd(),
                path_c.as_ptr() as *const u8,
                IN_ALL_EVENTS,
            )
        };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        self.watches.insert(wd, path.to_string_lossy().to_string());
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> io::Result<()> {
        let path_str = path.to_string_lossy().to_string();
        let wds: Vec<i32> = self
            .watches
            .iter()
            .filter(|(_, p)| **p == path_str)
            .map(|(wd, _)| *wd)
            .collect();
        for wd in wds {
            unsafe { inotify_rm_watch(self.fd.as_raw_fd(), wd) };
            self.watches.remove(&wd);
        }
        Ok(())
    }

    fn poll_events(&mut self) -> io::Result<Vec<WatchEvent>> {
        let mut buf = [0u8; 4096];
        let n = unsafe { read(self.fd.as_raw_fd(), buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Ok(Vec::new());
            }
            return Err(err);
        }
        let n = n as usize;
        let raw_events = Self::parse_buffer(&buf[..n]);
        Ok(self.resolve_paths(raw_events))
    }
}
