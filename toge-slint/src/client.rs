// Adapted from the Tauri shell IPC client; shares the existing wire protocol.
use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use toge_core::ipc::{
    MAX_IPC_MESSAGE_SIZE, QueryRequest, Request, Response, ResultsResponse, StatusResponse,
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

pub fn ensure_daemon_running(sock: &Path) -> io::Result<()> {
    match status(sock) {
        // A connected daemon may be busy with another query holding its index lock.
        // A status timeout is not evidence that it needs to be launched again.
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::TimedOut => return Ok(()),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) => {}
        Err(e) => return Err(e),
    }
    let mut child = daemon_command(sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
        match status(sock) {
            Ok(_) => return Ok(()),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "daemon did not start",
    ))
}

const STATUS_TIMEOUT: Duration = Duration::from_secs(2);
// Keep broad searches within the IPC frame and row limits.
const MAX_DISPLAY_RESULTS: usize = 10_000;
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

fn connect(sock: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let stream = UnixStream::connect(sock)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

fn send_request(stream: &mut UnixStream, req: &Request) -> io::Result<()> {
    let bytes = req.encode();
    stream.write_all(&(bytes.len() as u64).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn read_response(stream: &mut UnixStream) -> io::Result<Response> {
    read_response_frame(stream).map_err(|error| {
        if matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ) {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "Daemon response timed out. Retry shortly.",
            )
        } else {
            error
        }
    })
}

fn read_response_frame(stream: &mut UnixStream) -> io::Result<Response> {
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
    let mut stream = connect(sock, STATUS_TIMEOUT)?;
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

pub fn query(sock: &Path, id: u64, raw: &str, offset: usize) -> io::Result<ResultsResponse> {
    let mut stream = connect(sock, QUERY_TIMEOUT)?;
    let req = Request::Query(QueryRequest {
        id,
        raw: raw.to_string(),
        max_results: MAX_DISPLAY_RESULTS,
        offset,
        format: toge_core::ipc::OutputFormat::Default,
        highlight: false,
    });
    send_request(&mut stream, &req)?;
    match read_response(&mut stream)? {
        Response::Results(r) if r.id == id => Ok(r),
        Response::Error(e) => Err(io::Error::other(e)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response type",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn rejects_oversized_and_truncated_frames() {
        for size in [MAX_IPC_MESSAGE_SIZE as u64 + 1, 4] {
            let (mut reader, mut writer) = UnixStream::pair().unwrap();
            writer.write_all(&size.to_le_bytes()).unwrap();
            drop(writer);
            let e = read_response(&mut reader).unwrap_err();
            assert_eq!(
                e.kind(),
                if size > MAX_IPC_MESSAGE_SIZE as u64 {
                    io::ErrorKind::InvalidData
                } else {
                    io::ErrorKind::UnexpectedEof
                }
            );
        }
    }

    #[test]
    fn rejects_invalid_payload_and_times_out_on_stalled_peer() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(&1u64.to_le_bytes()).unwrap();
        writer.write_all(&[255]).unwrap();
        assert_eq!(
            read_response(&mut reader).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        reader
            .set_read_timeout(Some(Duration::from_millis(30)))
            .unwrap();
        let error = read_response(&mut reader).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("Daemon response timed out"));
    }

    #[test]
    fn busy_daemon_status_does_not_trigger_a_new_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 9];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(request[8], 2); // Status request
            thread::sleep(STATUS_TIMEOUT + Duration::from_millis(100));
        });
        assert!(ensure_daemon_running(&path).is_ok());
        server.join().unwrap();
    }

    #[test]
    fn query_checks_response_id_and_preserves_request() {
        for response_id in [7, 8] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("daemon.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut len = [0; 8];
                stream.read_exact(&mut len).unwrap();
                let mut body = vec![0; u64::from_le_bytes(len) as usize];
                stream.read_exact(&mut body).unwrap();
                match Request::decode(&body).unwrap() {
                    Request::Query(q) => {
                        assert_eq!(q.id, 7);
                        assert_eq!(q.raw, "ext:pdf");
                        assert_eq!(q.max_results, 10_000);
                    }
                    _ => panic!("expected query"),
                }
                // A valid query may take longer than the status timeout.
                if response_id == 7 {
                    thread::sleep(STATUS_TIMEOUT + Duration::from_millis(100));
                }
                let response = Response::Results(ResultsResponse {
                    id: response_id,
                    total_count: 0,
                    total_size: 0,
                    rows: vec![],
                })
                .encode();
                stream
                    .write_all(&(response.len() as u64).to_le_bytes())
                    .unwrap();
                stream.write_all(&response).unwrap();
            });
            let result = query(&path, 7, "ext:pdf", 0);
            assert_eq!(result.is_ok(), response_id == 7);
            server.join().unwrap();
        }
    }
}
