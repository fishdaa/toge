// Adapted from the Tauri shell IPC client; shares the existing wire protocol.
use std::env;
use std::io::{self, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use toge_core::ipc::{
    MAX_IPC_MESSAGE_SIZE, QueryRequest, Request, Response, ResultRow, ResultsResponse,
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
    read_response_frame(stream).map_err(response_error)
}

fn response_error(error: io::Error) -> io::Error {
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

// Decode the existing Results frame incrementally, so older daemons work too.
// A batch is published before reading the rest of the frame; no full-frame buffer
// or duplicate 10k-row result vector is needed.
pub fn query_stream(
    sock: &Path,
    id: u64,
    raw: &str,
    offset: usize,
    mut publish: impl FnMut(ResultsResponse, bool, bool) -> io::Result<()>,
) -> io::Result<()> {
    let mut stream = connect(sock, QUERY_TIMEOUT)?;
    send_request(
        &mut stream,
        &Request::Query(QueryRequest {
            id,
            raw: raw.to_string(),
            max_results: usize::MAX,
            offset,
            format: toge_core::ipc::OutputFormat::Default,
            highlight: false,
        }),
    )?;
    read_results_stream(&mut BufReader::new(stream), id, &mut publish).map_err(response_error)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_string(reader: &mut std::io::Take<impl Read>) -> io::Result<String> {
    let len = read_u64(reader)?;
    if len > reader.limit() {
        return Err(invalid("string exceeds response frame"));
    }
    let mut bytes = vec![0; len as usize];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("invalid UTF-8"))
}

fn read_results_stream(
    reader: &mut impl Read,
    id: u64,
    publish: &mut impl FnMut(ResultsResponse, bool, bool) -> io::Result<()>,
) -> io::Result<()> {
    let len = read_u64(reader)?;
    if len > MAX_IPC_MESSAGE_SIZE as u64 {
        return Err(invalid("response too large"));
    }
    let mut frame = reader.take(len);
    let mut tag = [0];
    frame.read_exact(&mut tag)?;
    if tag[0] == 4 {
        return Err(io::Error::other(read_string(&mut frame)?));
    }
    if tag[0] != 1 || read_u64(&mut frame)? != id {
        return Err(invalid("unexpected response type or query id"));
    }
    let total_count =
        usize::try_from(read_u64(&mut frame)?).map_err(|_| invalid("total count overflow"))?;
    let total_size = read_u64(&mut frame)?;
    let count = read_u64(&mut frame)?;
    // Each row needs at least four string lengths, a flag, and four u64s.
    // Validate against the frame itself rather than imposing a display cap.
    const MIN_ROW_BYTES: u64 = 4 * 8 + 1 + 4 * 8;
    if count > frame.limit() / MIN_ROW_BYTES {
        return Err(invalid("row count exceeds response frame"));
    }
    const BATCH_SIZE: usize = 128;
    let mut rows = Vec::with_capacity(BATCH_SIZE);
    let mut first = true;
    for index in 0..count {
        let path = read_string(&mut frame)?;
        let name = read_string(&mut frame)?;
        let parent = read_string(&mut frame)?;
        let extension = read_string(&mut frame)?;
        let mut is_dir = [0];
        frame.read_exact(&mut is_dir)?;
        rows.push(ResultRow {
            path,
            name,
            parent,
            extension,
            is_dir: is_dir[0] == 1,
            size: read_u64(&mut frame)?,
            modified_unix: read_u64(&mut frame)? as i64,
            created_unix: read_u64(&mut frame)? as i64,
            accessed_unix: read_u64(&mut frame)? as i64,
        });
        if rows.len() == BATCH_SIZE && index + 1 < count {
            publish(
                ResultsResponse {
                    id,
                    total_count,
                    total_size,
                    rows,
                },
                first,
                false,
            )?;
            first = false;
            rows = Vec::with_capacity(BATCH_SIZE);
        }
    }
    if frame.limit() != 0 {
        return Err(invalid("unexpected trailing result bytes"));
    }
    publish(
        ResultsResponse {
            id,
            total_count,
            total_size,
            rows,
        },
        first,
        true,
    )
}

#[cfg(test)]
fn query(sock: &Path, id: u64, raw: &str, offset: usize) -> io::Result<ResultsResponse> {
    let mut result = ResultsResponse {
        id,
        total_count: 0,
        total_size: 0,
        rows: vec![],
    };
    query_stream(sock, id, raw, offset, |batch, _, _| {
        result.total_count = batch.total_count;
        result.total_size = batch.total_size;
        result.rows.extend(batch.rows);
        Ok(())
    })?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn result_rows(count: usize) -> Vec<ResultRow> {
        (0..count)
            .map(|i| ResultRow {
                path: format!("/test/{i}"),
                name: i.to_string(),
                parent: "/test".into(),
                extension: String::new(),
                is_dir: false,
                size: i as u64,
                modified_unix: -1,
                created_unix: 2,
                accessed_unix: 3,
            })
            .collect()
    }

    #[test]
    fn publishes_first_batch_before_peer_sends_remaining_rows() {
        let rows = result_rows(300);
        let full = Response::Results(ResultsResponse {
            id: 7,
            total_count: 500,
            total_size: 42,
            rows: rows.clone(),
        })
        .encode();
        let prefix = Response::Results(ResultsResponse {
            id: 7,
            total_count: 500,
            total_size: 42,
            rows: rows[..128].to_vec(),
        })
        .encode()
        .len();
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            writer
                .write_all(&(full.len() as u64).to_le_bytes())
                .unwrap();
            writer.write_all(&full[..prefix]).unwrap();
            // Deadlocks/times out if the client waits for the full frame.
            received.recv_timeout(Duration::from_secs(2)).unwrap();
            writer.write_all(&full[prefix..]).unwrap();
        });
        let mut collected = vec![];
        let mut flags = vec![];
        read_results_stream(&mut reader, 7, &mut |batch, first, done| {
            assert_eq!(batch.total_count, 500);
            assert_eq!(batch.total_size, 42);
            if first {
                sent.send(()).unwrap();
            }
            flags.push((batch.rows.len(), first, done));
            collected.extend(batch.rows);
            Ok(())
        })
        .unwrap();
        server.join().unwrap();
        assert_eq!(
            flags,
            [(128, true, false), (128, false, false), (44, false, true)]
        );
        assert_eq!(collected, rows);
    }

    #[test]
    fn streams_every_row_beyond_the_old_display_limit() {
        let expected = result_rows(25_003);
        let payload = Response::Results(ResultsResponse {
            id: 7,
            total_count: expected.len(),
            total_size: 42,
            rows: expected.clone(),
        })
        .encode();
        let mut wire = (payload.len() as u64).to_le_bytes().to_vec();
        wire.extend(payload);
        let mut rows = Vec::new();
        let mut completed = false;
        read_results_stream(&mut &wire[..], 7, &mut |batch, first, done| {
            assert_eq!(batch.total_count, 25_003);
            assert_eq!(first, rows.is_empty());
            assert!(batch.rows.len() <= 128);
            assert!(!completed);
            completed = done;
            rows.extend(batch.rows);
            Ok(())
        })
        .unwrap();
        assert!(completed);
        assert_eq!(rows, expected);
    }

    #[test]
    fn streaming_empty_errors_truncation_and_cancellation() {
        let empty = Response::Results(ResultsResponse {
            id: 7,
            total_count: 0,
            total_size: 0,
            rows: vec![],
        })
        .encode();
        let mut wire = (empty.len() as u64).to_le_bytes().to_vec();
        wire.extend(empty);
        let mut called = false;
        read_results_stream(&mut &wire[..], 7, &mut |batch, first, done| {
            called = true;
            assert!(first && done && batch.rows.is_empty());
            Ok(())
        })
        .unwrap();
        assert!(called);
        assert!(read_results_stream(&mut &wire[..], 8, &mut |_, _, _| panic!()).is_err());
        let full = Response::Results(ResultsResponse {
            id: 7,
            total_count: 300,
            total_size: 0,
            rows: result_rows(300),
        })
        .encode();
        let mut wire = (full.len() as u64).to_le_bytes().to_vec();
        wire.extend(&full);
        let error = read_results_stream(&mut &wire[..wire.len() - 1], 7, &mut |_, _, done| {
            assert!(!done);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        let mut reader = &wire[..];
        let error = read_results_stream(&mut reader, 7, &mut |_, _, _| {
            Err(io::Error::other("superseded"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "superseded");
        assert!(!reader.is_empty());
        wire[8 + 25..8 + 33].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(read_results_stream(&mut &wire[..], 7, &mut |_, _, _| panic!()).is_err());
        let error = Response::Error("bad query".into()).encode();
        let mut wire = (error.len() as u64).to_le_bytes().to_vec();
        wire.extend(error);
        assert_eq!(
            read_results_stream(&mut &wire[..], 7, &mut |_, _, _| panic!())
                .unwrap_err()
                .to_string(),
            "bad query"
        );
    }

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
                        assert_eq!(q.max_results, usize::MAX);
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
