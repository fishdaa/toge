use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use slint::ComponentHandle;
use std::collections::HashSet;
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

const FILE_CLIPBOARD_TYPE: &str = "x-special/gnome-copied-files";

fn clipboard_payload(path: &Path, cut: bool) -> io::Result<String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(uri, "%{byte:02X}").unwrap();
        }
    }
    Ok(format!("{}\n{uri}", if cut { "cut" } else { "copy" }))
}

fn file_clipboard(path: &Path, cut: bool) -> io::Result<Vec<ClipboardContent>> {
    let payload = clipboard_payload(path, cut)?;
    let uri = payload.split_once('\n').unwrap().1;
    Ok(vec![
        ClipboardContent::Text(path.to_string_lossy().into_owned()),
        ClipboardContent::Other("text/uri-list".into(), format!("{uri}\r\n").into_bytes()),
        ClipboardContent::Other(FILE_CLIPBOARD_TYPE.into(), payload.into_bytes()),
        ClipboardContent::Other(
            "application/x-kde-cutselection".into(),
            if cut { b"1" } else { b"0" }.to_vec(),
        ),
    ])
}

fn write_clipboard(contents: Vec<ClipboardContent>) -> io::Result<()> {
    // Retain the X11 selection owner after the action's worker finishes.
    static CLIPBOARD: Mutex<Option<ClipboardContext>> = Mutex::new(None);
    let mut clipboard = CLIPBOARD
        .lock()
        .map_err(|_| io::Error::other("Clipboard unavailable"))?;
    if clipboard.is_none() {
        *clipboard = Some(ClipboardContext::new().map_err(io::Error::other)?);
    }
    clipboard
        .as_ref()
        .unwrap()
        .set(contents)
        .map_err(io::Error::other)
}

pub fn rename(path: &Path, name: &str) -> io::Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter a filename without slashes.",
        ));
    }
    let parent = path
        .parent()
        .filter(|_| path.file_name().is_some())
        .ok_or_else(|| io::Error::other("This location cannot be renamed."))?;
    let target = parent.join(name);
    if target == path {
        return Ok(target);
    }
    let old = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let new = CString::new(target.as_os_str().as_bytes()).map_err(io::Error::other)?;
    unsafe extern "C" {
        fn renameat2(
            oldfd: i32,
            old: *const std::ffi::c_char,
            newfd: i32,
            new: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    // RENAME_NOREPLACE prevents overwriting a destination, including a dangling
    // symlink, atomically rather than relying on a racy existence check.
    let result = unsafe { renameat2(-100, old.as_ptr(), -100, new.as_ptr(), 1) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(target)
}

fn trash_with(path: &Path, program: &Path) -> io::Result<()> {
    let output = Command::new(program)
        .arg("trash")
        .arg("--")
        .arg(path)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Could not move to Trash: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// Permanently remove a path. Symlinks are removed themselves, never their targets.
pub fn delete_permanently(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn file_action(action: &str, path: &Path) -> io::Result<&'static str> {
    match action {
        "copy" | "cut" => {
            write_clipboard(file_clipboard(path, action == "cut")?)?;
            Ok(if action == "cut" {
                "Cut file — paste in your file manager to move it"
            } else {
                "Copied file — paste in your file manager"
            })
        }
        "copy-path" => {
            write_clipboard(vec![ClipboardContent::Text(
                path.to_string_lossy().into_owned(),
            )])?;
            Ok("Copied path")
        }
        "open" | "parent" => {
            let target = if action == "parent" {
                path.parent().unwrap_or(path)
            } else {
                path
            };
            if Command::new("xdg-open").arg(target).status()?.success() {
                Ok("Opened")
            } else {
                Err(io::Error::other("Could not open this location."))
            }
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unknown file action",
        )),
    }
}

/// Keep the editor attached to its original path when fresh query results arrive.
pub fn place_rename(ui: &crate::AppWindow, position: Option<i32>) {
    if ui.get_rename_path().is_empty() {
        return;
    }
    let index = position.unwrap_or(-1);
    ui.set_rename_row(index);
    if index < 0 {
        ui.invoke_cancel_rename(false);
    }
}

/// Run a trash or permanent delete off the UI thread, then refresh the row.
fn remove(
    ui: &crate::AppWindow,
    path: PathBuf,
    permanent: bool,
    pending_deletes: &Arc<Mutex<HashSet<PathBuf>>>,
) {
    // Ignore repeated key events while this item is being removed.
    if !pending_deletes.lock().unwrap().insert(path.clone()) {
        return;
    }
    ui.invoke_cancel_rename(false);
    ui.set_status(
        if permanent {
            "Deleting…"
        } else {
            "Moving to Trash…"
        }
        .into(),
    );
    let pending = pending_deletes.clone();
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let outcome = if permanent {
            delete_permanently(&path)
                .map_err(|error| io::Error::other(format!("Could not delete: {error}")))
        } else {
            trash_with(&path, Path::new("gio"))
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            pending.lock().unwrap().remove(&path);
            match outcome {
                Ok(()) => {
                    crate::worker::results(&ui).send(crate::model::Command::Reconcile {
                        paths: vec![path.to_string_lossy().into_owned()],
                        select: None,
                    });
                    ui.set_status(
                        if permanent {
                            "Deleted permanently"
                        } else {
                            "Moved to Trash"
                        }
                        .into(),
                    );
                }
                Err(error) => ui.set_status(error.to_string().into()),
            }
        });
    });
}

pub fn connect(ui: &crate::AppWindow) {
    let pending_deletes = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));
    let weak = ui.as_weak();
    let pending = pending_deletes.clone();
    ui.on_delete_confirmed(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        // Delete the path shown in the dialog, even if results refreshed meanwhile.
        let path = ui.get_delete_path();
        ui.invoke_close_delete_confirm();
        if !path.is_empty() {
            remove(&ui, PathBuf::from(path.as_str()), true, &pending);
        }
    });
    let weak = ui.as_weak();
    ui.on_rename_commit(move |name| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_rename_path().is_empty() || ui.get_rename_working() {
            return;
        }
        let path = PathBuf::from(ui.get_rename_path().as_str());
        ui.set_rename_working(true);
        ui.set_status("Renaming…".into());
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let outcome = rename(&path, &name);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_rename_working(false);
                match outcome {
                    Ok(target) => {
                        ui.invoke_cancel_rename(true);
                        // The daemon re-reads both paths, so the row moves to
                        // its new sort position without waiting for the watcher.
                        let target = target.to_string_lossy().into_owned();
                        crate::worker::results(&ui).send(crate::model::Command::Reconcile {
                            paths: vec![path.to_string_lossy().into_owned(), target.clone()],
                            select: Some(target),
                        });
                        ui.set_status("Renamed".into());
                    }
                    Err(error) => {
                        ui.set_status(
                            format!("Could not rename: {error} — Enter to retry, Esc to cancel")
                                .into(),
                        );
                        if ui.get_rename_row() < 0 {
                            ui.invoke_cancel_rename(true);
                        } else {
                            ui.invoke_focus_rename();
                        }
                    }
                }
            });
        });
    });
    let weak = ui.as_weak();
    ui.on_action(move |action| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_rename_working() {
            return;
        }
        let Some(path) = crate::worker::results(&ui).path(ui.get_selected()) else {
            return;
        };
        let path = PathBuf::from(path);
        if action == "rename" {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let selection_end = if path.is_dir() {
                name.len()
            } else {
                path.file_stem().map_or(name.len(), |stem| stem.len())
            };
            ui.set_rename_path(path.to_string_lossy().into_owned().into());
            ui.invoke_begin_rename(
                ui.get_selected(),
                name.to_string().into(),
                selection_end as i32,
            );
            return;
        }
        if action == "delete" {
            remove(&ui, path, false, &pending_deletes);
            return;
        }
        if action == "delete-permanently" {
            if pending_deletes.lock().unwrap().contains(&path) {
                return;
            }
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            ui.invoke_confirm_delete(
                path.to_string_lossy().into_owned().into(),
                name.into_owned().into(),
            );
            return;
        }
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result = file_action(&action, &path);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_status(match result {
                    Ok(message) => message.into(),
                    Err(error) => format!("Action failed: {error}").into(),
                });
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_files_escape_special_characters_and_preserve_cut_intent() {
        assert_eq!(
            clipboard_payload(Path::new("/tmp/a b#%\n.txt"), false).unwrap(),
            "copy\nfile:///tmp/a%20b%23%25%0A.txt"
        );
        assert_eq!(
            clipboard_payload(Path::new("/tmp/é.txt"), true).unwrap(),
            "cut\nfile:///tmp/%C3%A9.txt"
        );
    }
    #[test]
    fn file_clipboard_offers_kde_and_gnome_copy_and_cut_formats() {
        for cut in [false, true] {
            let contents = file_clipboard(Path::new("/tmp/a b.txt"), cut).unwrap();
            let formats: std::collections::HashMap<_, _> = contents
                .into_iter()
                .filter_map(|content| {
                    if let ClipboardContent::Other(mime, bytes) = content {
                        Some((mime, bytes))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(formats["text/uri-list"], b"file:///tmp/a%20b.txt\r\n");
            assert_eq!(
                formats["application/x-kde-cutselection"],
                if cut { b"1" } else { b"0" }
            );
            assert_eq!(
                formats[FILE_CLIPBOARD_TYPE],
                format!(
                    "{}\nfile:///tmp/a%20b.txt",
                    if cut { "cut" } else { "copy" }
                )
                .as_bytes()
            );
        }
    }
    #[test]
    fn rename_rejects_traversal_and_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        let existing = dir.path().join("existing.txt");
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&existing, "keep").unwrap();
        for name in [
            "",
            ".",
            "..",
            "../escape.txt",
            "/tmp/escape.txt",
            "a/b",
            "bad\0name",
            "existing.txt",
        ] {
            assert!(rename(&source, name).is_err(), "{name:?}");
        }
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "source");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");
        let target = rename(&source, "renamed.txt").unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "source");
    }
    #[test]
    fn rename_preserves_symlinks_and_rejects_dangling_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("link");
        std::os::unix::fs::symlink("missing", &source).unwrap();
        std::os::unix::fs::symlink("also-missing", dir.path().join("taken")).unwrap();
        assert!(rename(&source, "taken").is_err());
        let target = rename(&source, "renamed").unwrap();
        assert_eq!(std::fs::read_link(target).unwrap(), Path::new("missing"));
    }
    #[test]
    fn permanent_delete_removes_files_and_folders_but_not_symlink_targets() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "gone").unwrap();
        delete_permanently(&file).unwrap();
        assert!(!file.exists());
        let folder = dir.path().join("folder");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        std::fs::write(folder.join("nested/a.txt"), "a").unwrap();
        delete_permanently(&folder).unwrap();
        assert!(!folder.exists());
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), "keep").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        delete_permanently(&link).unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("keep.txt")).unwrap(),
            "keep"
        );
        assert!(delete_permanently(&dir.path().join("missing")).is_err());
    }
    #[test]
    fn trash_failure_is_reported_without_deleting_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keep.txt");
        std::fs::write(&file, "keep").unwrap();
        let gio = dir.path().join("gio");
        std::fs::write(&gio, "#!/bin/sh\necho 'Trash unavailable' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&gio, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            trash_with(&file, &gio)
                .unwrap_err()
                .to_string()
                .contains("Trash unavailable")
        );
        assert!(file.exists());
    }
}
