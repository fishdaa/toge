//! Linux fanotify-based filesystem watcher using filesystem-level marks.
//!
//! Uses `FAN_MARK_FILESYSTEM` to watch entire filesystems with a single kernel
//! mark, reducing the watch count from one-per-directory (inotify) to
//! one-per-filesystem.
//!
//! On btrfs, subvolumes have independent fsids, so `FAN_MARK_FILESYSTEM` on a
//! subvolume mount fails with `EXDEV`. To work around this, the watcher
//! auto-mounts the btrfs toplevel (subvolid=5) at a private mount point and
//! marks that instead. Events from all subvolumes are caught, and file handles
//! are resolved via the btrfs root mount fd, with path translation back to the
//! real subvolume mount points.
//!
//! Requires `CAP_SYS_ADMIN` (for `FAN_MARK_FILESYSTEM` and mounting) and
//! `CAP_DAC_READ_SEARCH` (for `open_by_handle_at`).
//!
//! Grant both capabilities with:
//!   sudo setcap cap_sys_admin,cap_dac_read_search+ep /path/to/toged

use super::{FsWatcher, WatchEvent};
use std::env;
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

// ===== FFI =====

unsafe extern "C" {
    fn fanotify_init(flags: u32, event_f_flags: u32) -> i32;
    fn fanotify_mark(fan_fd: i32, flags: u32, mask: u64, dirfd: i32, pathname: *const i8) -> i32;
    fn open_by_handle_at(mountdirfd: i32, handle: *mut FileHandle, flags: i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn mount(
        source: *const i8,
        target: *const i8,
        fstype: *const i8,
        flags: u64,
        data: *const std::ffi::c_void,
    ) -> i32;
    fn umount(target: *const i8) -> i32;
    fn mkdir(path: *const i8, mode: u32) -> i32;
}

// ===== Constants =====

const FAN_CLOEXEC: u32 = 0x0000_0001;
const FAN_NONBLOCK: u32 = 0x0000_0002;
const FAN_CLASS_NOTIF: u32 = 0x0000_0000;
const FAN_REPORT_DIR_FID: u32 = 0x0000_0400;
const FAN_REPORT_NAME: u32 = 0x0000_0800;
const FAN_REPORT_DFID_NAME: u32 = FAN_REPORT_DIR_FID | FAN_REPORT_NAME;

const FAN_MARK_ADD: u32 = 0x0000_0001;
const FAN_MARK_FILESYSTEM: u32 = 0x0000_0100;

const FAN_MODIFY: u64 = 0x0000_0002;
const FAN_MOVED_FROM: u64 = 0x0000_0040;
const FAN_MOVED_TO: u64 = 0x0000_0080;
const FAN_CREATE: u64 = 0x0000_0100;
const FAN_DELETE: u64 = 0x0000_0200;
const FAN_Q_OVERFLOW: u64 = 0x0000_4000;
const FAN_ONDIR: u64 = 0x4000_0000;

const FAN_NOFD: i32 = -1;
const AT_FDCWD: i32 = -100;

const FAN_EVENT_INFO_TYPE_DFID_NAME: u8 = 2;
const FAN_EVENT_INFO_TYPE_DFID: u8 = 3;

const O_RDONLY: i32 = 0;
const O_CLOEXEC: i32 = 0x0008_0000;

const FANOTIFY_METADATA_VERSION: u8 = 3;
const METADATA_LEN: usize = 24;
const FID_HDR_LEN: usize = 4;
const FSID_LEN: usize = 8;
const FILE_HANDLE_HDR_LEN: usize = 8;

const MAX_HANDLE_SZ: usize = 128;

const WATCH_MASK: u64 =
    FAN_CREATE | FAN_DELETE | FAN_MODIFY | FAN_MOVED_FROM | FAN_MOVED_TO | FAN_ONDIR;

// mount(2) flags
const MS_NOSUID: u64 = 0x0000_0002;
const MS_NODEV: u64 = 0x0000_0004;
const MS_RDONLY: u64 = 0x0000_0001;

// ===== Structs =====

#[repr(C)]
struct FileHandle {
    handle_bytes: u32,
    handle_type: i32,
}

#[repr(C)]
struct FileHandleBuf {
    handle_bytes: u32,
    handle_type: i32,
    f_handle: [u8; MAX_HANDLE_SZ],
}

/// A filesystem that has been marked with fanotify.
struct MarkedFs {
    /// fd to a directory on this filesystem, for open_by_handle_at
    handle_fd: OwnedFd,
    /// For btrfs: prefix in the btrfs root mount → real mount point
    /// e.g. /mnt/btrfs-root/home → /home
    path_translations: Vec<(PathBuf, PathBuf)>,
}

// ===== Watcher =====

pub struct FanotifyWatcher {
    fd: OwnedFd,
    marked: Vec<MarkedFs>,
    marked_count: usize,
    /// btrfs root mount points we created (to unmount on drop)
    btrfs_mounts: Vec<PathBuf>,
    buf: Vec<u8>,
}

impl FanotifyWatcher {
    pub fn new() -> io::Result<Self> {
        let flags = FAN_CLOEXEC | FAN_NONBLOCK | FAN_CLASS_NOTIF | FAN_REPORT_DFID_NAME;
        let raw = unsafe { fanotify_init(flags, (O_RDONLY | O_CLOEXEC) as u32) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(raw) },
            marked: Vec::new(),
            marked_count: 0,
            btrfs_mounts: Vec::new(),
            buf: vec![0u8; 65536],
        })
    }

    pub fn fs_count(&self) -> usize {
        self.marked_count
    }

    fn mount_point_of(path: &Path) -> io::Result<PathBuf> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        let mut best: Option<PathBuf> = None;
        for line in mountinfo.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 5 {
                continue;
            }
            let mp = PathBuf::from(fields[4]);
            if path.starts_with(&mp) {
                let is_better = best
                    .as_ref()
                    .is_none_or(|b| mp.as_os_str().len() > b.as_os_str().len());
                if is_better {
                    best = Some(mp);
                }
            }
        }
        best.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no mount point found"))
    }

    /// Parse mountinfo to find the device and btrfs subvol path for a btrfs mount point.
    fn btrfs_mount_info(mount_point: &Path) -> Option<(String, String)> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo").ok()?;
        for line in mountinfo.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 {
                continue;
            }
            let mp = PathBuf::from(fields[4]);
            if mp != mount_point {
                continue;
            }
            // Format: mount_id parent dev root mountpoint mount_opts [optional...] - fstype source super_opts
            // Find the "-" separator
            let sep_idx = fields.iter().position(|f| *f == "-")?;
            let fstype = fields.get(sep_idx + 1)?;
            if *fstype != "btrfs" {
                continue;
            }
            let device = fields.get(sep_idx + 2)?;
            let super_opts = fields.get(sep_idx + 3).unwrap_or(&"");
            let subvol = super_opts
                .split(',')
                .find(|o| o.starts_with("subvol="))
                .and_then(|o| o.strip_prefix("subvol="))?;
            return Some((device.to_string(), subvol.to_string()));
        }
        None
    }

    /// Find all btrfs subvolume mount points and their subvol paths on the same device.
    fn btrfs_subvolumes(device: &str) -> Vec<(PathBuf, String)> {
        let mountinfo = match fs::read_to_string("/proc/self/mountinfo") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for line in mountinfo.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 {
                continue;
            }
            let sep_idx = match fields.iter().position(|f| *f == "-") {
                Some(i) => i,
                None => continue,
            };
            let fstype = match fields.get(sep_idx + 1) {
                Some(f) => *f,
                None => continue,
            };
            if fstype != "btrfs" {
                continue;
            }
            let dev = match fields.get(sep_idx + 2) {
                Some(d) => *d,
                None => continue,
            };
            if dev != device {
                continue;
            }
            let mp = PathBuf::from(fields[4]);
            let super_opts = fields.get(sep_idx + 3).unwrap_or(&"");
            if let Some(subvol) = super_opts
                .split(',')
                .find(|o| o.starts_with("subvol="))
                .and_then(|o| o.strip_prefix("subvol="))
            {
                out.push((mp, subvol.to_string()));
            }
        }
        out
    }

    /// Mount the btrfs toplevel (subvolid=5) at a private mount point.
    fn mount_btrfs_root(device: &str) -> io::Result<PathBuf> {
        let pid = std::process::id();
        let base = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let mount_dir = base.join(format!("toge-btrfs-root-{}", pid));
        let mount_dir_str = mount_dir.to_string_lossy().into_owned();

        let cdir = CString::new(mount_dir_str.as_str()).unwrap();
        let rc = unsafe { mkdir(cdir.as_ptr(), 0o700) };
        if rc < 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
            return Err(io::Error::last_os_error());
        }

        let csrc = CString::new(device).unwrap();
        let ctgt = CString::new(mount_dir_str.as_str()).unwrap();
        let cfst = CString::new("btrfs").unwrap();
        let cdata = CString::new("subvolid=5").unwrap();

        let rc = unsafe {
            mount(
                csrc.as_ptr(),
                ctgt.as_ptr(),
                cfst.as_ptr(),
                MS_NOSUID | MS_NODEV | MS_RDONLY,
                cdata.as_ptr() as *const std::ffi::c_void,
            )
        };
        if rc < 0 {
            let err = io::Error::last_os_error();
            let _ = unsafe { umount(ctgt.as_ptr()) };
            let _ = fs::remove_dir(&mount_dir);
            return Err(err);
        }

        Ok(mount_dir)
    }

    fn try_mark_filesystem(&self, path: &Path) -> io::Result<()> {
        let cstr = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let ret = unsafe {
            fanotify_mark(
                self.fd.as_raw_fd(),
                FAN_MARK_ADD | FAN_MARK_FILESYSTEM,
                WATCH_MASK,
                AT_FDCWD,
                cstr.as_ptr(),
            )
        };
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn resolve_handle(
        &self,
        handle_bytes: u32,
        handle_type: i32,
        handle_data: &[u8],
    ) -> Option<PathBuf> {
        let mut fh = FileHandleBuf {
            handle_bytes,
            handle_type,
            f_handle: [0u8; MAX_HANDLE_SZ],
        };
        let copy = handle_data.len().min(MAX_HANDLE_SZ);
        fh.f_handle[..copy].copy_from_slice(&handle_data[..copy]);

        for marked in &self.marked {
            let fd = unsafe {
                open_by_handle_at(
                    marked.handle_fd.as_raw_fd(),
                    &mut fh as *mut _ as *mut FileHandle,
                    O_RDONLY | O_CLOEXEC,
                )
            };
            if fd >= 0 {
                let path = fs::read_link(format!("/proc/self/fd/{}", fd)).ok();
                let _ = unsafe { OwnedFd::from_raw_fd(fd) };
                if let Some(p) = path {
                    // Translate btrfs root paths to real mount point paths
                    let translated = self.translate_path(&p, marked);
                    return Some(translated);
                }
            }
        }
        None
    }

    fn translate_path(&self, resolved: &Path, marked: &MarkedFs) -> PathBuf {
        for (btrfs_prefix, real_mount) in &marked.path_translations {
            if let Ok(rest) = resolved.strip_prefix(btrfs_prefix) {
                if rest.as_os_str().is_empty() {
                    return real_mount.clone();
                }
                return real_mount.join(rest);
            }
        }
        resolved.to_path_buf()
    }

    fn read_u16(&self, off: usize) -> u16 {
        u16::from_ne_bytes([self.buf[off], self.buf[off + 1]])
    }
    fn read_u32(&self, off: usize) -> u32 {
        u32::from_ne_bytes([
            self.buf[off],
            self.buf[off + 1],
            self.buf[off + 2],
            self.buf[off + 3],
        ])
    }
    fn read_i32(&self, off: usize) -> i32 {
        i32::from_ne_bytes([
            self.buf[off],
            self.buf[off + 1],
            self.buf[off + 2],
            self.buf[off + 3],
        ])
    }
    fn read_u64(&self, off: usize) -> u64 {
        u64::from_ne_bytes([
            self.buf[off],
            self.buf[off + 1],
            self.buf[off + 2],
            self.buf[off + 3],
            self.buf[off + 4],
            self.buf[off + 5],
            self.buf[off + 6],
            self.buf[off + 7],
        ])
    }

    fn extract_dir_and_name(
        &self,
        event_start: usize,
        event_len: u32,
    ) -> Option<(PathBuf, String)> {
        let event_end = event_start + event_len as usize;
        let mut off = event_start + METADATA_LEN;

        while off + FID_HDR_LEN <= event_end {
            let info_type = self.buf[off];
            let info_len = self.read_u16(off + 2) as usize;
            if info_len == 0 || off + info_len > event_end {
                break;
            }

            if info_type == FAN_EVENT_INFO_TYPE_DFID_NAME || info_type == FAN_EVENT_INFO_TYPE_DFID {
                let fh_off = off + FID_HDR_LEN + FSID_LEN;
                if fh_off + FILE_HANDLE_HDR_LEN > event_end {
                    break;
                }
                let handle_bytes = self.read_u32(fh_off);
                let handle_type = self.read_i32(fh_off + 4);
                let data_start = fh_off + FILE_HANDLE_HDR_LEN;
                if data_start + handle_bytes as usize > event_end {
                    break;
                }
                let handle_data = &self.buf[data_start..data_start + handle_bytes as usize];

                let parent = self.resolve_handle(handle_bytes, handle_type, handle_data);

                let name = if info_type == FAN_EVENT_INFO_TYPE_DFID_NAME {
                    let name_start = data_start + handle_bytes as usize;
                    if name_start < off + info_len {
                        let name_end = (off + info_len).min(event_end);
                        let raw = &self.buf[name_start..name_end];
                        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                        Some(String::from_utf8_lossy(&raw[..end]).to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(parent) = parent {
                    return Some((parent, name.unwrap_or_default()));
                }
            }

            off += info_len;
        }
        None
    }
}

impl Drop for FanotifyWatcher {
    fn drop(&mut self) {
        for mount_dir in &self.btrfs_mounts {
            let cpath = CString::new(mount_dir.as_os_str().as_encoded_bytes()).unwrap_or_default();
            unsafe { umount(cpath.as_ptr()) };
            let _ = fs::remove_dir(mount_dir);
        }
    }
}

impl FsWatcher for FanotifyWatcher {
    fn watch(&mut self, path: &Path) -> io::Result<()> {
        let mount = Self::mount_point_of(path)?;

        // Check if we already marked this filesystem
        for marked in &self.marked {
            if marked.path_translations.iter().any(|(_, mp)| mp == &mount) {
                return Ok(());
            }
        }

        // Try FAN_MARK_FILESYSTEM directly on the mount point
        match self.try_mark_filesystem(&mount) {
            Ok(()) => {
                let handle_fd = OwnedFd::from(fs::File::open(&mount)?);
                self.marked.push(MarkedFs {
                    handle_fd,
                    path_translations: vec![(mount.clone(), mount.clone())],
                });
                self.marked_count += 1;
                return Ok(());
            }
            Err(e) if e.raw_os_error() == Some(18) => {}
            Err(e) => {
                return Err(e);
            }
        }

        // btrfs: find device, mount root subvolid=5, mark that
        let (device, _subvol) = Self::btrfs_mount_info(&mount)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "btrfs mount info not found"))?;

        // Check if we already mounted this btrfs root
        let btrfs_root = {
            let mut existing: Option<PathBuf> = None;
            for marked in &self.marked {
                if let Some((first_prefix, _)) = marked.path_translations.first()
                    && first_prefix
                        .to_str()
                        .is_some_and(|s| s.contains("toge-btrfs-root"))
                {
                    existing = Some(first_prefix.clone());
                    break;
                }
            }
            if let Some(root) = existing {
                root
            } else {
                let root = Self::mount_btrfs_root(&device)?;
                self.btrfs_mounts.push(root.clone());
                root
            }
        };

        // Mark the btrfs root filesystem
        self.try_mark_filesystem(&btrfs_root)?;

        // Build path translations for all subvolumes on this device
        let subvolumes = Self::btrfs_subvolumes(&device);
        let translations: Vec<(PathBuf, PathBuf)> = subvolumes
            .iter()
            .map(|(mp, subvol)| (btrfs_root.join(subvol.trim_start_matches('/')), mp.clone()))
            .collect();

        let handle_fd = OwnedFd::from(fs::File::open(&btrfs_root)?);
        self.marked.push(MarkedFs {
            handle_fd,
            path_translations: translations,
        });
        self.marked_count += 1;
        Ok(())
    }

    fn unwatch(&mut self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn poll_events(&mut self) -> io::Result<Vec<WatchEvent>> {
        let mut events = Vec::new();
        let n = unsafe { read(self.fd.as_raw_fd(), self.buf.as_mut_ptr(), self.buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Ok(events);
            }
            return Err(err);
        }

        let n = n as usize;
        let mut offset = 0;

        while offset + METADATA_LEN <= n {
            let event_len = self.read_u32(offset) as usize;
            let vers = self.buf[offset + 4];
            let _metadata_len = self.read_u16(offset + 6);
            let mask = self.read_u64(offset + 8);
            let fd = self.read_i32(offset + 16);

            if event_len < METADATA_LEN || offset + event_len > n {
                break;
            }
            if vers != FANOTIFY_METADATA_VERSION {
                offset += event_len;
                continue;
            }

            if mask & FAN_Q_OVERFLOW != 0 {
                if fd != FAN_NOFD {
                    let _ = unsafe { OwnedFd::from_raw_fd(fd) };
                }
                events.push(WatchEvent::Overflow {
                    path: String::new(),
                });
                offset += event_len;
                continue;
            }

            let is_dir = mask & FAN_ONDIR != 0;

            // For FAN_MODIFY events, the fd points to the modified file
            let path_opt = if fd != FAN_NOFD {
                let p = fs::read_link(format!("/proc/self/fd/{}", fd)).ok();
                let _ = unsafe { OwnedFd::from_raw_fd(fd) };
                p
            } else {
                None
            };

            // For dir events (CREATE, DELETE, MOVE), use DFID_NAME info records
            let path = if let Some(p) = path_opt {
                p
            } else {
                match self.extract_dir_and_name(offset, event_len as u32) {
                    Some((parent, name)) => {
                        if name.is_empty() {
                            parent
                        } else {
                            parent.join(&name)
                        }
                    }
                    None => {
                        offset += event_len;
                        continue;
                    }
                }
            };

            let path_str = path.to_string_lossy().to_string();

            if mask & FAN_CREATE != 0 || mask & FAN_MOVED_TO != 0 {
                events.push(WatchEvent::Create {
                    path: path_str,
                    is_dir,
                });
            } else if mask & FAN_DELETE != 0 || mask & FAN_MOVED_FROM != 0 {
                events.push(WatchEvent::Delete { path: path_str });
            } else if mask & FAN_MODIFY != 0 {
                events.push(WatchEvent::Modify { path: path_str });
            }

            offset += event_len;
        }

        Ok(events)
    }
}
