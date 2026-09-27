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
}

impl Request {
    pub fn from_arg(arg: Option<&str>) -> Option<Self> {
        match arg {
            None => Some(Self::Show),
            Some("--new-window") => Some(Self::NewWindow),
            Some("--toggle") => Some(Self::Toggle),
            Some(_) => None,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::NewWindow => "new-window",
            Self::Toggle => "toggle",
        }
    }
    fn parse(word: &str) -> Option<Self> {
        [Self::Show, Self::NewWindow, Self::Toggle]
            .into_iter()
            .find(|request| request.word() == word)
    }
}

/// One GUI per daemon socket, so isolated development profiles stay separate.
pub fn socket_path() -> PathBuf {
    crate::client::socket_path().with_file_name("toge-slint.sock")
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
    fn arguments_map_to_requests() {
        assert_eq!(Request::from_arg(None), Some(Request::Show));
        assert_eq!(
            Request::from_arg(Some("--new-window")),
            Some(Request::NewWindow)
        );
        assert_eq!(Request::from_arg(Some("--toggle")), Some(Request::Toggle));
        assert_eq!(Request::from_arg(Some("--bogus")), None);
    }

    #[test]
    fn later_launch_hands_requests_to_the_running_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toge-slint.sock");
        assert!(!send(&path, Request::Toggle).unwrap());
        let (tx, rx) = channel();
        serve(bind(&path).unwrap(), move |request| {
            tx.send(request).unwrap()
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
            tx.send(request).unwrap()
        });
        assert!(send(&path, Request::NewWindow).unwrap());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Request::NewWindow
        );
    }
}
