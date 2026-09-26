use slint::ComponentHandle;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const CAPABILITIES: &str = "cap_dac_read_search,cap_sys_admin=ep";

fn system_tool(name: &str) -> io::Result<PathBuf> {
    ["/usr/bin", "/usr/sbin", "/bin", "/sbin"]
        .into_iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Install {name} to enable live updates."),
            )
        })
}

fn has_capabilities(binary: &Path, getcap: &Path) -> io::Result<bool> {
    let output = Command::new(getcap).arg(binary).output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "Could not check background service access.",
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim()
        == format!("{} {CAPABILITIES}", binary.display()))
}

fn grant(binary: &Path, pkexec: &Path, setcap: &Path, getcap: &Path) -> io::Result<()> {
    let output = Command::new(pkexec)
        .arg("--disable-internal-agent")
        .arg(setcap)
        .arg(CAPABILITIES)
        .arg(binary)
        .output()?;
    match output.status.code() {
        Some(0) if has_capabilities(binary, getcap)? => Ok(()),
        Some(0) => Err(io::Error::other(
            "Access could not be verified. Live updates remain disabled.",
        )),
        Some(126) => Err(io::Error::other(
            "Approval was cancelled. You can try again or cancel setup.",
        )),
        _ => Err(io::Error::other(
            "Access was not approved. Check that a system authentication agent is running, then try again.",
        )),
    }
}

pub fn request(binary: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let binary = binary.canonicalize()?;
    let getcap = system_tool("getcap")?;
    if has_capabilities(&binary, &getcap)? {
        return Ok(());
    }
    let ui = crate::AccessWindow::new()?;
    let accepted = Arc::new(AtomicBool::new(false));
    ui.on_cancel(|| {
        let _ = slint::quit_event_loop();
    });
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        if weak.upgrade().is_some_and(|ui| ui.get_granting()) {
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        }
    });
    let weak = ui.as_weak();
    let approved = accepted.clone();
    ui.on_approve(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_granting() {
            return;
        }
        ui.set_granting(true);
        ui.set_message("".into());
        let weak = ui.as_weak();
        let binary = binary.clone();
        let getcap = getcap.clone();
        let approved = approved.clone();
        std::thread::spawn(move || {
            let result = (|| {
                grant(
                    &binary,
                    &system_tool("pkexec")?,
                    &system_tool("setcap")?,
                    &getcap,
                )
            })();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_granting(false);
                match result {
                    Ok(()) => {
                        approved.store(true, Ordering::Relaxed);
                        let _ = slint::quit_event_loop();
                    }
                    Err(error) => ui.set_message(error.to_string().into()),
                }
            });
        });
    });
    ui.run()?;
    if accepted.load(Ordering::Relaxed) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Live update setup was cancelled.",
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tool(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn approval_uses_fixed_arguments_and_verifies_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("daemon with spaces");
        let pkexec = tool(
            dir.path(),
            "pkexec",
            "printf '%s\\n' \"$@\" > \"$0.args\"; exit 0",
        );
        let setcap = dir.path().join("setcap");
        let getcap = tool(
            dir.path(),
            "getcap",
            "printf '%s cap_dac_read_search,cap_sys_admin=ep\\n' \"$1\"",
        );
        grant(&binary, &pkexec, &setcap, &getcap).unwrap();
        let args = std::fs::read_to_string(pkexec.with_extension("args")).unwrap();
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            vec![
                "--disable-internal-agent",
                setcap.to_str().unwrap(),
                CAPABILITIES,
                binary.to_str().unwrap(),
            ]
        );
    }

    #[test]
    fn cancelled_or_denied_authentication_never_counts_as_approval() {
        for code in [126, 127] {
            let dir = tempfile::tempdir().unwrap();
            let pkexec = tool(dir.path(), "pkexec", &format!("exit {code}"));
            let error = grant(
                Path::new("daemon"),
                &pkexec,
                Path::new("setcap"),
                Path::new("missing-getcap"),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains(if code == 126 {
                    "cancelled"
                } else {
                    "not approved"
                }),
                "exit {code}: {error}"
            );
        }
    }

    #[test]
    fn successful_command_without_capabilities_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let pkexec = tool(dir.path(), "pkexec", "exit 0");
        let getcap = tool(dir.path(), "getcap", "exit 0");
        assert!(
            grant(Path::new("daemon"), &pkexec, Path::new("setcap"), &getcap)
                .unwrap_err()
                .to_string()
                .contains("could not be verified")
        );
    }
}
