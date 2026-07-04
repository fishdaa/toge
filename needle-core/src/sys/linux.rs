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

    fn resolve_paths(&self, events: Vec<WatchEvent>) -> Vec<WatchEvent> {
        events
            .into_iter()
            .map(|ev| match &ev {
                WatchEvent::Create { path: name, is_dir } => {
                    let full = self.resolve_full_path(name);
                    WatchEvent::Create {
                        path: full,
                        is_dir: *is_dir,
                    }
                }
                WatchEvent::Delete { path: name } => WatchEvent::Delete {
                    path: self.resolve_full_path(name),
                },
                WatchEvent::Modify { path: name } => WatchEvent::Modify {
                    path: self.resolve_full_path(name),
                },
                WatchEvent::Move { from, to } => WatchEvent::Move {
                    from: self.resolve_full_path(from),
                    to: self.resolve_full_path(to),
                },
                other => other.clone(),
            })
            .collect()
    }

    fn resolve_full_path(&self, name: &str) -> String {
        if name.is_empty() {
            return String::new();
        }
        if name.starts_with('/') {
            return name.to_string();
        }
        if name.contains("overflow") {
            return name.to_string();
        }
        for dir in self.watches.values() {
            let full = format!("{}/{}", dir, name);
            if std::path::Path::new(&full).exists() {
                return full;
            }
        }
        if let Some(first_dir) = self.watches.values().next() {
            return format!("{}/{}", first_dir, name);
        }
        name.to_string()
    }

    /// Parse a raw inotify event buffer into structured events.
    /// Exposed for unit testing the parsing logic without touching the kernel.
    pub fn parse_buffer(buf: &[u8]) -> Vec<WatchEvent> {
        let mut events = Vec::new();
        let mut offset = 0;
        let mut move_from: Option<(u32, String)> = None;

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
                events.push(WatchEvent::Overflow {
                    path: String::new(),
                });
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
                events.push(WatchEvent::Create { path: name, is_dir });
            } else if mask & IN_DELETE != 0 {
                events.push(WatchEvent::Delete { path: name });
            } else if mask & IN_MODIFY != 0 {
                events.push(WatchEvent::Modify { path: name });
            } else if mask & IN_MOVED_FROM != 0 {
                move_from = Some((cookie, name));
            } else if mask & IN_MOVED_TO != 0 {
                if let Some((from_cookie, from_path)) = move_from.take() {
                    if from_cookie == cookie && !from_path.is_empty() {
                        events.push(WatchEvent::Move {
                            from: from_path,
                            to: name,
                        });
                    }
                }
            }

            // wd -1 indicates an error/overflow from the kernel.
            let _ = wd;
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
