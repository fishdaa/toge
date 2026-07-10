use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use toge_core::ipc::{
    DaemonStatus, MAX_IPC_MESSAGE_SIZE, QueryRequest, Request, Response, ResultsResponse,
    StatusResponse,
};

pub fn socket_path() -> PathBuf {
    env::var_os("TOGE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_state_dir().join("toged.sock"))
}

fn default_state_dir() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME").expect("HOME not set");
            PathBuf::from(home).join(".local/state")
        })
        .join("toge")
}

fn daemon_command(sock: &Path) -> Command {
    if let Ok(current) = env::current_exe()
        && let Some(bin_dir) = current.parent()
    {
        let sibling = bin_dir.join("toged");
        if sibling.exists() {
            let mut cmd = Command::new(sibling);
            cmd.arg("--socket").arg(sock);
            return cmd;
        }
    }
    let mut cmd = Command::new("toged");
    cmd.arg("--socket").arg(sock);
    cmd
}

fn daemon_responding(sock: &Path) -> bool {
    status(sock).is_ok()
}

pub fn ensure_daemon_running(sock: &Path) -> io::Result<()> {
    if daemon_responding(sock) {
        return Ok(());
    }
    daemon_command(sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    for _ in 0..100 {
        thread::sleep(Duration::from_millis(50));
        if daemon_responding(sock) {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "daemon did not start",
    ))
}

pub enum ReadyEvent {
    Progress(String),
    Ready,
}

pub fn wait_for_ready(
    sock: &Path,
    timeout: Duration,
    event_tx: &mpsc::Sender<ReadyEvent>,
) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match status(sock) {
            Ok(s) if s.status == DaemonStatus::Ready => return Ok(()),
            Ok(s) => {
                let msg = if s.status_message.is_empty() {
                    format!("{:?}", s.status)
                } else {
                    format!("{:?}: {}", s.status, s.status_message)
                };
                let _ = event_tx.send(ReadyEvent::Progress(msg));
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionAborted
                ) => {}
            Err(e) => return Err(e),
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "daemon did not become ready in time",
    ))
}

fn connect(sock: &Path) -> io::Result<UnixStream> {
    UnixStream::connect(sock)
}

fn send_request(stream: &mut UnixStream, req: &Request) -> io::Result<()> {
    let bytes = req.encode();
    stream.write_all(&(bytes.len() as u64).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn read_response(stream: &mut UnixStream) -> io::Result<Response> {
    let mut len_buf = [0u8; 8];
    stream.read_exact(&mut len_buf)?;
    let len = u64::from_le_bytes(len_buf) as usize;
    if len > MAX_IPC_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Response::decode(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn status(sock: &Path) -> io::Result<StatusResponse> {
    let mut stream = connect(sock)?;
    send_request(&mut stream, &Request::Status)?;
    match read_response(&mut stream)? {
        Response::Status(s) => Ok(s),
        Response::Error(e) => Err(io::Error::other(e)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response type",
        )),
    }
}

pub fn query(
    sock: &Path,
    id: u64,
    raw: &str,
    max_results: usize,
    offset: usize,
) -> io::Result<ResultsResponse> {
    let mut stream = connect(sock)?;
    let req = Request::Query(QueryRequest {
        id,
        raw: raw.to_string(),
        max_results,
        offset,
        format: toge_core::ipc::OutputFormat::Default,
        highlight: false,
    });
    send_request(&mut stream, &req)?;
    match read_response(&mut stream)? {
        Response::Results(r) => Ok(r),
        Response::Error(e) => Err(io::Error::other(e)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response type",
        )),
    }
}

pub fn reindex(sock: &Path) -> io::Result<()> {
    let mut stream = connect(sock)?;
    send_request(&mut stream, &Request::Reindex)?;
    match read_response(&mut stream)? {
        Response::Ok => Ok(()),
        Response::Error(e) => Err(io::Error::other(e)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response type",
        )),
    }
}
