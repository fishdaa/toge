use std::path::Path;
use std::process::{Command, Stdio};

pub fn open_path(path: &str) {
    let _ = Command::new("xdg-open")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn reveal_in_folder(path: &str) {
    let parent = Path::new(path)
        .parent()
        .map(|p| p.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    let _ = Command::new("xdg-open")
        .arg(parent)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn copy_to_clipboard(text: &str) {
    let _ = try_copy(text, "wl-copy", &[])
        .or_else(|_| try_copy(text, "xclip", &["-selection", "clipboard"]))
        .or_else(|_| try_copy(text, "xsel", &["--clipboard", "--input"]));
}

fn try_copy(text: &str, program: &str, args: &[&str]) -> std::io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(text.as_bytes())?;
    }
    let _ = child.wait()?;
    Ok(())
}
