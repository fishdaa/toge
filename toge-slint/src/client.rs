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
    StreamOrder, StreamQueryRequest, stream_query,
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

// Consume bounded daemon frames. The GUI sorts loaded rows locally, so use
// index order to avoid a daemon-wide matching-ID buffer.
pub fn query_stream(
    sock: &Path,
    id: u64,
    raw: &str,
    offset: usize,
    register: impl FnOnce(&UnixStream) -> io::Result<()>,
    mut publish: impl FnMut(ResultsResponse, bool, bool) -> io::Result<()>,
) -> io::Result<()> {
    let mut connection = connect(sock, QUERY_TIMEOUT)?;
    register(&connection)?;
    let request = StreamQueryRequest {
        query: QueryRequest {
            id,
            raw: raw.to_string(),
            max_results: usize::MAX,
            offset,
            format: toge_core::ipc::OutputFormat::Default,
            highlight: false,
        },
        order: StreamOrder::Index,
    };
    let mut first = true;
    let summary = stream_query(&mut connection, &request, |rows| {
        publish(
            ResultsResponse {
                id,
                total_count: 0,
                total_size: 0,
                rows: rows.to_vec(),
            },
            first,
            false,
        )?;
        first = false;
        Ok(())
    })
    .map_err(|error| {
        if error.to_string() == "unknown request type" {
            io::Error::other("Daemon does not support streaming. Restart or rebuild toged.")
        } else {
            response_error(error)
        }
    })?;
    publish(
        ResultsResponse {
            id,
            total_count: summary.total_count,
            total_size: summary.total_size,
            rows: vec![],
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
    query_stream(
        sock,
        id,
        raw,
        offset,
        |_| Ok(()),
        |batch, _, _| {
            result.total_count = batch.total_count;
            result.total_size = batch.total_size;
            result.rows.extend(batch.rows);
            Ok(())
        },
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use toge_core::ipc::{ResultRow, StreamEvent, StreamSummary};

    pub(super) fn result_rows(count: usize) -> Vec<ResultRow> {
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
                    Request::StreamQuery(request) => {
                        assert_eq!(request.order, StreamOrder::Index);
                        let q = request.query;
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
                let response = StreamEvent::Done(StreamSummary {
                    id: response_id,
                    total_count: 0,
                    total_size: 0,
                    returned_count: 0,
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

#[cfg(test)]
mod stream_tests {
    use super::tests::result_rows;
    use super::*;
    use std::os::unix::net::UnixListener;
    use toge_core::ipc::{ResultRow, StreamEvent, StreamSummary};

    fn send(socket: &mut UnixStream, event: StreamEvent) -> io::Result<()> {
        let bytes = event.encode();
        socket.write_all(&(bytes.len() as u64).to_le_bytes())?;
        socket.write_all(&bytes)
    }
    fn request(socket: &mut UnixStream) -> StreamQueryRequest {
        let mut length = [0; 8];
        socket.read_exact(&mut length).unwrap();
        let mut bytes = vec![0; u64::from_le_bytes(length) as usize];
        socket.read_exact(&mut bytes).unwrap();
        let Request::StreamQuery(request) = Request::decode(&bytes).unwrap() else {
            panic!("expected daemon stream request")
        };
        assert_eq!(request.order, StreamOrder::Index);
        request
    }

    #[test]
    fn publishes_rows_before_completion_and_totals_only_after_done() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let rows = result_rows(300);
        let expected = rows.clone();
        let (published, wait) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let request = request(&mut socket);
            assert_eq!(request.query.raw, "foo");
            assert_eq!(request.query.offset, 5);
            send(
                &mut socket,
                StreamEvent::Rows {
                    id: 7,
                    rows: rows[..128].to_vec(),
                },
            )
            .unwrap();
            // The first rows must reach the UI before later rows or totals exist.
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            for batch in rows[128..].chunks(128) {
                send(
                    &mut socket,
                    StreamEvent::Rows {
                        id: 7,
                        rows: batch.to_vec(),
                    },
                )
                .unwrap();
            }
            send(
                &mut socket,
                StreamEvent::Done(StreamSummary {
                    id: 7,
                    total_count: 305,
                    total_size: 42,
                    returned_count: 300,
                }),
            )
            .unwrap();
        });
        let mut collected = Vec::<ResultRow>::new();
        let mut flags = vec![];
        query_stream(
            &path,
            7,
            "foo",
            5,
            |_| Ok(()),
            |batch, first, done| {
                if first {
                    published.send(()).unwrap();
                }
                assert_eq!(batch.total_count, if done { 305 } else { 0 });
                assert_eq!(batch.total_size, if done { 42 } else { 0 });
                flags.push((batch.rows.len(), first, done));
                collected.extend(batch.rows);
                Ok(())
            },
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(collected, expected);
        assert_eq!(
            flags,
            [
                (128, true, false),
                (128, false, false),
                (44, false, false),
                (0, false, true)
            ]
        );
    }

    #[test]
    fn streams_all_rows_beyond_the_old_display_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let request = request(&mut socket);
            let rows = result_rows(25_003);
            for batch in rows.chunks(128) {
                send(
                    &mut socket,
                    StreamEvent::Rows {
                        id: request.query.id,
                        rows: batch.to_vec(),
                    },
                )
                .unwrap();
            }
            send(
                &mut socket,
                StreamEvent::Done(StreamSummary {
                    id: request.query.id,
                    total_count: rows.len(),
                    total_size: 42,
                    returned_count: rows.len(),
                }),
            )
            .unwrap();
        });
        let result = query(&path, 7, "all", 0).unwrap();
        server.join().unwrap();
        assert_eq!(result.rows, result_rows(25_003));
        assert_eq!(result.total_count, 25_003);
    }

    #[test]
    fn incomplete_errors_empty_and_cancelled_streams_do_not_publish_false_completion() {
        for scenario in ["empty", "truncated", "error", "legacy", "cancel"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("daemon.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                request(&mut socket);
                match scenario {
                    "empty" => send(
                        &mut socket,
                        StreamEvent::Done(StreamSummary {
                            id: 7,
                            total_count: 0,
                            total_size: 0,
                            returned_count: 0,
                        }),
                    )
                    .unwrap(),
                    "error" | "legacy" => send(
                        &mut socket,
                        StreamEvent::Error(
                            if scenario == "legacy" {
                                "unknown request type"
                            } else {
                                "bad query"
                            }
                            .into(),
                        ),
                    )
                    .unwrap(),
                    _ => send(
                        &mut socket,
                        StreamEvent::Rows {
                            id: 7,
                            rows: result_rows(128),
                        },
                    )
                    .unwrap(),
                }
            });
            let mut done = false;
            let result = query_stream(
                &path,
                7,
                "test",
                0,
                |_| Ok(()),
                |batch, first, complete| {
                    done = complete;
                    if scenario == "empty" {
                        assert!(first && complete && batch.rows.is_empty());
                    }
                    if scenario == "cancel" {
                        return Err(io::Error::other("superseded"));
                    }
                    Ok(())
                },
            );
            server.join().unwrap();
            assert_eq!(result.is_ok(), scenario == "empty");
            assert_eq!(done, scenario == "empty");
            if scenario == "legacy" {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("Restart or rebuild")
                );
            }
        }
    }
}
