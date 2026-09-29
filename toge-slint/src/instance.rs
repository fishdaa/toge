//! Single-instance handoff: a later launch asks the running GUI to show,
//! open or toggle a window instead of starting a second process.
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Show the current window, opening one if none exists.
    Show,
    NewWindow,
    /// Hide the current window if it is visible, otherwise show it.
    Toggle,
    /// Hide the current window if it is visible.
    Hide,
}

impl Request {
    pub fn from_arg(arg: Option<&str>) -> Option<Self> {
        match arg {
            None => Some(Self::Show),
            Some("--new-window") => Some(Self::NewWindow),
            Some("--toggle") => Some(Self::Toggle),
            Some("--hide") => Some(Self::Hide),
            Some(_) => None,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::NewWindow => "new-window",
            Self::Toggle => "toggle",
            Self::Hide => "hide",
        }
    }
    fn parse(word: &str) -> Option<Self> {
        [Self::Show, Self::NewWindow, Self::Toggle, Self::Hide]
            .into_iter()
            .find(|request| request.word() == word)
    }
}

/// One GUI per daemon socket, so isolated development profiles stay separate.
pub fn socket_path() -> PathBuf {
    instance_path(&crate::client::socket_path())
}

fn instance_path(daemon: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    // The parent provides a namespace; a filename hash keeps Unix socket names short.
    let mut id = 0xcbf2_9ce4_8422_2325u64;
    for byte in daemon.file_name().unwrap_or_default().as_bytes() {
        id = (id ^ u64::from(*byte)).wrapping_mul(0x000_0100_0000_01b3);
    }
    daemon.with_file_name(format!("toge-slint-{id:016x}.sock"))
}

/// Hand `request` to a running instance. `Ok(false)` means none is running.
pub fn send(path: &Path, request: Request) -> io::Result<bool> {
    let mut stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    writeln!(stream, "{}", request.word())?;
    // Wait for the acknowledgement so the caller exits only once it was queued.
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    if reply.trim() == "ok" {
        Ok(true)
    } else {
        Err(io::Error::other(
            "running Toge instance rejected the request",
        ))
    }
}

/// Bind the instance socket, replacing one left behind by a crashed instance.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        use std::os::unix::fs::DirBuilderExt;
        // Do not change permissions on existing custom directories such as /tmp.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    match UnixListener::bind(path) {
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            if send(path, Request::Show)? {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another Toge instance is running",
                ));
            }
            std::fs::remove_file(path)?;
            UnixListener::bind(path)
        }
        result => result,
    }
}

/// Serve requests on a background thread, calling `handle` for each one.
pub fn serve(listener: UnixListener, handle: impl Fn(Request) + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            match Request::parse(line.trim()) {
                Some(request) => {
                    handle(request);
                    let _ = stream.write_all(b"ok\n");
                }
                None => {
                    let _ = stream.write_all(b"error\n");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn daemon_sockets_in_one_directory_have_independent_instances() {
        let dir = tempfile::tempdir().unwrap();
        let first = instance_path(&dir.path().join("one.sock"));
        let second = instance_path(&dir.path().join("two.sock"));
        assert_ne!(first, second);
        let (tx, rx) = channel();
        serve(bind(&first).unwrap(), move |request| {
            tx.send(request).unwrap();
        });
        let (tx, other) = channel();
        serve(bind(&second).unwrap(), move |request| {
            tx.send(request).unwrap();
        });
        assert!(send(&first, Request::Toggle).unwrap());
        assert!(send(&second, Request::NewWindow).unwrap());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Request::Toggle
        );
        assert_eq!(
            other.recv_timeout(Duration::from_secs(2)).unwrap(),
            Request::NewWindow
        );
    }

    #[test]
    fn first_launch_creates_private_state_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let original_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode();
        let parent = dir.path().join("fresh/toge");
        let path = parent.join("gui.sock");
        let (tx, rx) = channel();
        serve(bind(&path).unwrap(), move |request| {
            tx.send(request).unwrap();
        });
        assert_eq!(
            std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(send(&path, Request::Toggle).unwrap());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Request::Toggle
        );
        assert_eq!(
            std::fs::metadata(dir.path()).unwrap().permissions().mode(),
            original_mode
        );
    }

    #[test]
    fn arguments_map_to_requests() {
        assert_eq!(Request::from_arg(None), Some(Request::Show));
        assert_eq!(
            Request::from_arg(Some("--new-window")),
            Some(Request::NewWindow)
        );
        assert_eq!(Request::from_arg(Some("--toggle")), Some(Request::Toggle));
        assert_eq!(Request::from_arg(Some("--hide")), Some(Request::Hide));
        assert_eq!(Request::from_arg(Some("--bogus")), None);
    }

    #[test]
    fn later_launch_hands_requests_to_the_running_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toge-slint.sock");
        assert!(!send(&path, Request::Toggle).unwrap());
        let (tx, rx) = channel();
        serve(bind(&path).unwrap(), move |request| {
            tx.send(request).unwrap();
        });
        for request in [Request::Toggle, Request::NewWindow, Request::Show] {
            assert!(send(&path, request).unwrap());
            assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), request);
        }
        // A second instance does not steal the live socket.
        assert_eq!(bind(&path).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Request::Show
        );
    }

    #[test]
    fn stale_socket_from_a_crashed_instance_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toge-slint.sock");
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        assert!(!send(&path, Request::Show).unwrap());
        let (tx, rx) = channel();
        serve(bind(&path).unwrap(), move |request| {
            tx.send(request).unwrap();
        });
        assert!(send(&path, Request::NewWindow).unwrap());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Request::NewWindow
        );
    }
}
