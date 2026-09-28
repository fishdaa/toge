//! Result sessions: the daemon keeps a query's matching IDs and clients fetch
//! only the row ranges they display.
//!
//! A client opens a session with [`Request::OpenSession`] on a dedicated
//! connection. The daemon answers with a [`SessionResponse::State`] and then
//! serves [`SessionRequest`]s on that connection, one response per request,
//! until the client disconnects, which discards the session.
//! [`Request::OpenSessionPreview`] additionally emits bounded generation-zero
//! row previews in index order during matching, before the sorted state.

use super::{Request, push_string, push_u64, push_usize, take_string, take_u64, take_usize};
use crate::sort::SortKey;
use std::io::{self, Read, Write};

/// Largest row range a single fetch may request.
pub const MAX_SESSION_FETCH: usize = 1024;
/// Preview storage stays bounded while the daemon finishes matching and sorting.
pub const SESSION_PREVIEW_ROWS: usize = 256;
/// Largest number of paths a single reconcile may name.
pub const MAX_SESSION_RECONCILE: usize = 256;
pub const MAX_SESSION_FRAME_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOpen {
    pub raw: String,
    /// Overrides the query's own sort; `None` keeps the query's sort.
    pub sort: Option<(SortKey, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    /// Rows `[offset, offset + len)` of the current result order.
    Fetch { offset: usize, len: usize },
    /// Reorder the retained results.
    Resort { sort: Option<(SortKey, bool)> },
    /// Position of `path` in the current result order.
    Locate { path: String },
    /// Report the current state, rebuilding the results if the index changed.
    Sync,
    /// Re-read these paths from disk (e.g. after the client renamed or trashed
    /// them) and rebuild the results.
    Reconcile { paths: Vec<String> },
}

/// Every response carries the session state. A changed `generation` means the
/// result order was rebuilt and previously fetched rows are stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionState {
    pub generation: u64,
    pub total_count: usize,
    pub total_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResponse {
    State(SessionState),
    Rows {
        state: SessionState,
        offset: usize,
        rows: Vec<SessionRow>,
    },
    Located {
        state: SessionState,
        position: Option<usize>,
    },
    Error(String),
}

impl SessionResponse {
    pub fn state(&self) -> Option<SessionState> {
        match self {
            Self::State(state) | Self::Rows { state, .. } | Self::Located { state, .. } => {
                Some(*state)
            }
            Self::Error(_) => None,
        }
    }
}

fn sort_key_to_u8(key: SortKey) -> u8 {
    match key {
        SortKey::Name => 0,
        SortKey::Path => 1,
        SortKey::Size => 2,
        SortKey::Modified => 3,
        SortKey::Created => 4,
        SortKey::Accessed => 5,
        SortKey::Extension => 6,
    }
}

fn sort_key_from_u8(value: u8) -> Option<SortKey> {
    Some(match value {
        0 => SortKey::Name,
        1 => SortKey::Path,
        2 => SortKey::Size,
        3 => SortKey::Modified,
        4 => SortKey::Created,
        5 => SortKey::Accessed,
        6 => SortKey::Extension,
        _ => return None,
    })
}

pub(super) fn push_sort(buf: &mut Vec<u8>, sort: Option<(SortKey, bool)>) {
    match sort {
        None => buf.push(0xff),
        Some((key, ascending)) => {
            buf.push(sort_key_to_u8(key));
            buf.push(u8::from(ascending));
        }
    }
}

// The outer `Option` is "malformed wire data" (propagated with `.ok_or(...)?`
// at the call sites); the inner `Option` is the actual "no sort requested"
// value carried over the wire. A dedicated wrapper type would only rename
// this distinction, so it's kept as-is for this internal wire-format helper.
#[allow(
    clippy::option_option,
    reason = "outer Option is parse failure, inner Option is the wire value; see comment above"
)]
pub(super) fn take_sort(buf: &[u8], off: &mut usize) -> Option<Option<(SortKey, bool)>> {
    let tag = *buf.get(*off)?;
    *off += 1;
    if tag == 0xff {
        return Some(None);
    }
    let key = sort_key_from_u8(tag)?;
    let ascending = match *buf.get(*off)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    *off += 1;
    Some(Some((key, ascending)))
}

fn finish<T>(value: T, off: usize, bytes: &[u8]) -> Result<T, String> {
    if off == bytes.len() {
        Ok(value)
    } else {
        Err("trailing session bytes".into())
    }
}

impl SessionRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::Fetch { offset, len } => {
                buf.push(1);
                push_usize(&mut buf, *offset);
                push_usize(&mut buf, *len);
            }
            Self::Resort { sort } => {
                buf.push(2);
                push_sort(&mut buf, *sort);
            }
            Self::Locate { path } => {
                buf.push(3);
                push_string(&mut buf, path);
            }
            Self::Sync => buf.push(4),
            Self::Reconcile { paths } => {
                buf.push(5);
                push_usize(&mut buf, paths.len());
                for path in paths {
                    push_string(&mut buf, path);
                }
            }
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut off = 1;
        let request = match bytes.first() {
            Some(1) => Self::Fetch {
                offset: take_usize(bytes, &mut off).ok_or("missing fetch offset")?,
                len: take_usize(bytes, &mut off).ok_or("missing fetch length")?,
            },
            Some(2) => Self::Resort {
                sort: take_sort(bytes, &mut off).ok_or("invalid sort")?,
            },
            Some(3) => Self::Locate {
                path: take_string(bytes, &mut off).ok_or("missing locate path")?,
            },
            Some(4) => Self::Sync,
            Some(5) => {
                let count = take_usize(bytes, &mut off).ok_or("missing path count")?;
                if count > MAX_SESSION_RECONCILE {
                    return Err("too many reconcile paths".into());
                }
                let mut paths = Vec::with_capacity(count);
                for _ in 0..count {
                    paths.push(take_string(bytes, &mut off).ok_or("missing reconcile path")?);
                }
                Self::Reconcile { paths }
            }
            _ => return Err("unknown session request".into()),
        };
        finish(request, off, bytes)
    }
}

fn push_state(buf: &mut Vec<u8>, state: &SessionState) {
    push_u64(buf, state.generation);
    push_usize(buf, state.total_count);
    push_u64(buf, state.total_size);
}

fn take_state(bytes: &[u8], off: &mut usize) -> Option<SessionState> {
    Some(SessionState {
        generation: take_u64(bytes, off)?,
        total_count: take_usize(bytes, off)?,
        total_size: take_u64(bytes, off)?,
    })
}

impl SessionResponse {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::State(state) => {
                buf.push(1);
                push_state(&mut buf, state);
            }
            Self::Rows {
                state,
                offset,
                rows,
            } => {
                buf.push(2);
                push_state(&mut buf, state);
                push_usize(&mut buf, *offset);
                push_usize(&mut buf, rows.len());
                for row in rows {
                    push_string(&mut buf, &row.path);
                    buf.push(u8::from(row.is_dir));
                    push_u64(&mut buf, row.size);
                    push_u64(&mut buf, row.modified_unix.cast_unsigned());
                }
            }
            Self::Located { state, position } => {
                buf.push(3);
                push_state(&mut buf, state);
                // usize::MAX cannot be a position: fetches cap offsets below it.
                push_usize(&mut buf, position.unwrap_or(usize::MAX));
            }
            Self::Error(error) => {
                buf.push(4);
                push_string(&mut buf, error);
            }
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut off = 1;
        let response = match bytes.first() {
            Some(1) => Self::State(take_state(bytes, &mut off).ok_or("missing state")?),
            Some(2) => {
                let state = take_state(bytes, &mut off).ok_or("missing state")?;
                let offset = take_usize(bytes, &mut off).ok_or("missing offset")?;
                let count = take_usize(bytes, &mut off).ok_or("missing row count")?;
                if count > MAX_SESSION_FETCH {
                    return Err("too many session rows".into());
                }
                let mut rows = Vec::with_capacity(count);
                for _ in 0..count {
                    let path = take_string(bytes, &mut off).ok_or("missing row path")?;
                    let is_dir = match bytes.get(off) {
                        Some(0) => false,
                        Some(1) => true,
                        _ => return Err("invalid row kind".into()),
                    };
                    off += 1;
                    rows.push(SessionRow {
                        path,
                        is_dir,
                        size: take_u64(bytes, &mut off).ok_or("missing row size")?,
                        modified_unix: take_u64(bytes, &mut off)
                            .ok_or("missing row modified")?
                            .cast_signed(),
                    });
                }
                Self::Rows {
                    state,
                    offset,
                    rows,
                }
            }
            Some(3) => {
                let state = take_state(bytes, &mut off).ok_or("missing state")?;
                let position = take_usize(bytes, &mut off).ok_or("missing position")?;
                Self::Located {
                    state,
                    position: (position != usize::MAX).then_some(position),
                }
            }
            Some(4) => Self::Error(take_string(bytes, &mut off).ok_or("missing error")?),
            _ => return Err("unknown session response".into()),
        };
        finish(response, off, bytes)
    }
}

pub fn write_frame<W: Write>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(&(bytes.len() as u64).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

/// Read one length-prefixed frame. Clean EOF before a frame returns `None`.
pub fn read_frame<R: Read>(reader: &mut R, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0; 8];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let len = u64::from_le_bytes(len);
    // Reject frames that don't fit in a usize as well as ones over `max`,
    // rather than truncating the length on 32-bit targets.
    let len = usize::try_from(len)
        .ok()
        .filter(|&len| len <= max)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "session frame too large"))?;
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

/// Client side of a result session over any byte stream.
pub struct SessionClient<S> {
    stream: S,
    state: SessionState,
}

impl<S: Read + Write> SessionClient<S> {
    /// Send the open request and wait for the initial state.
    pub fn open(mut stream: S, open: SessionOpen) -> io::Result<Self> {
        Self::open_request(&mut stream, &Request::OpenSession(open))?;
        Self::finish_open(stream, |_| Ok(()))
    }

    /// Preview rows are in index order, generation zero, and bounded to one page.
    /// The final state replaces the preview with the requested sorted session.
    pub fn open_with_preview(
        mut stream: S,
        open: SessionOpen,
        preview: impl FnMut(Vec<SessionRow>) -> io::Result<()>,
    ) -> io::Result<Self> {
        Self::open_request(&mut stream, &Request::OpenSessionPreview(open))?;
        Self::finish_open(stream, preview)
    }

    fn open_request(stream: &mut S, request: &Request) -> io::Result<()> {
        write_frame(stream, &request.encode())
    }

    fn finish_open(
        stream: S,
        mut preview: impl FnMut(Vec<SessionRow>) -> io::Result<()>,
    ) -> io::Result<Self> {
        let mut client = Self {
            stream,
            state: SessionState {
                generation: 0,
                total_count: 0,
                total_size: 0,
            },
        };
        loop {
            match client.read()? {
                SessionResponse::State(state) => {
                    client.state = state;
                    return Ok(client);
                }
                SessionResponse::Rows {
                    state,
                    offset: 0,
                    rows,
                } if state.generation == 0
                    && state.total_count == rows.len()
                    && rows.len() <= SESSION_PREVIEW_ROWS =>
                {
                    preview(rows)?;
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected session response",
                    ));
                }
            }
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn request(&mut self, request: &SessionRequest) -> io::Result<SessionResponse> {
        write_frame(&mut self.stream, &request.encode())?;
        let response = self.read()?;
        let consistent = match (request, &response) {
            (
                SessionRequest::Fetch { offset, len },
                SessionResponse::Rows {
                    offset: o, rows, ..
                },
            ) => o == offset && rows.len() <= *len,
            (SessionRequest::Locate { .. }, SessionResponse::Located { .. })
            | (
                SessionRequest::Resort { .. }
                | SessionRequest::Sync
                | SessionRequest::Reconcile { .. },
                SessionResponse::State(_),
            ) => true,
            _ => false,
        };
        if !consistent {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "inconsistent session response",
            ));
        }
        if let Some(state) = response.state() {
            self.state = state;
        }
        Ok(response)
    }

    fn read(&mut self) -> io::Result<SessionResponse> {
        let bytes = read_frame(&mut self.stream, MAX_SESSION_FRAME_SIZE)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "daemon closed the session")
        })?;
        match SessionResponse::decode(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        {
            SessionResponse::Error(error) => Err(io::Error::other(error)),
            response => Ok(response),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(generation: u64) -> SessionState {
        SessionState {
            generation,
            total_count: 3,
            total_size: 99,
        }
    }

    #[test]
    fn session_messages_round_trip() {
        for request in [
            SessionRequest::Fetch {
                offset: 256,
                len: 256,
            },
            SessionRequest::Resort { sort: None },
            SessionRequest::Resort {
                sort: Some((SortKey::Modified, false)),
            },
            SessionRequest::Locate {
                path: "/tmp/日本.txt".into(),
            },
            SessionRequest::Sync,
            SessionRequest::Reconcile {
                paths: vec!["/a".into(), "/b".into()],
            },
        ] {
            assert_eq!(SessionRequest::decode(&request.encode()).unwrap(), request);
        }
        for response in [
            SessionResponse::State(state(1)),
            SessionResponse::Rows {
                state: state(2),
                offset: 7,
                rows: vec![SessionRow {
                    path: "/x/é".into(),
                    is_dir: true,
                    size: 5,
                    modified_unix: -1,
                }],
            },
            SessionResponse::Located {
                state: state(3),
                position: Some(0),
            },
            SessionResponse::Located {
                state: state(3),
                position: None,
            },
            SessionResponse::Error("nope".into()),
        ] {
            assert_eq!(
                SessionResponse::decode(&response.encode()).unwrap(),
                response
            );
        }
        let open = Request::OpenSession(SessionOpen {
            raw: ".mkv".into(),
            sort: Some((SortKey::Size, true)),
        });
        assert_eq!(Request::decode(&open.encode()).unwrap(), open);
    }

    #[test]
    fn session_decoding_rejects_malformed_frames() {
        let mut fetch = SessionRequest::Fetch { offset: 0, len: 1 }.encode();
        fetch.push(0);
        assert!(SessionRequest::decode(&fetch).is_err());
        assert!(SessionRequest::decode(&[2, 9, 1]).is_err());
        assert!(SessionRequest::decode(&[]).is_err());
        let mut many = vec![5];
        push_usize(&mut many, MAX_SESSION_RECONCILE + 1);
        assert!(SessionRequest::decode(&many).is_err());
        let mut rows = vec![2];
        push_state(&mut rows, &state(1));
        push_usize(&mut rows, 0);
        push_usize(&mut rows, MAX_SESSION_FETCH + 1);
        assert!(SessionResponse::decode(&rows).is_err());
        let mut buf = io::Cursor::new(((MAX_SESSION_FRAME_SIZE + 1) as u64).to_le_bytes().to_vec());
        assert!(read_frame(&mut buf, MAX_SESSION_FRAME_SIZE).is_err());
        assert!(
            read_frame(&mut io::Cursor::new(Vec::new()), 8)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn preview_reaches_callback_before_daemon_can_send_completion() {
        use std::os::unix::net::UnixStream;
        let (client, mut daemon) = UnixStream::pair().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let bytes = read_frame(&mut daemon, MAX_SESSION_FRAME_SIZE)
                .unwrap()
                .unwrap();
            let request = Request::decode(&bytes).unwrap();
            assert!(matches!(request, Request::OpenSessionPreview(_)));
            assert_eq!(Request::decode(&request.encode()).unwrap(), request);
            write_frame(
                &mut daemon,
                &SessionResponse::Rows {
                    state: SessionState {
                        generation: 0,
                        total_count: 1,
                        total_size: 0,
                    },
                    offset: 0,
                    rows: vec![SessionRow {
                        path: "/early.txt".into(),
                        is_dir: false,
                        size: 1,
                        modified_unix: 0,
                    }],
                }
                .encode(),
            )
            .unwrap();
            // Completion cannot be sent until the consumer has handled the preview.
            rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
            write_frame(&mut daemon, &SessionResponse::State(state(1)).encode()).unwrap();
        });
        let session = SessionClient::open_with_preview(
            client,
            SessionOpen {
                raw: String::new(),
                sort: None,
            },
            |rows| {
                assert_eq!(rows[0].path, "/early.txt");
                tx.send(()).unwrap();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(session.state(), state(1));
        server.join().unwrap();
    }

    #[test]
    fn client_rejects_mismatched_responses_and_surfaces_errors() {
        use std::os::unix::net::UnixStream;
        let (client, mut daemon) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            let open = read_frame(&mut daemon, 1 << 20).unwrap().unwrap();
            assert!(matches!(
                Request::decode(&open).unwrap(),
                Request::OpenSession(_)
            ));
            write_frame(&mut daemon, &SessionResponse::State(state(1)).encode()).unwrap();
            read_frame(&mut daemon, 1 << 20).unwrap().unwrap();
            write_frame(
                &mut daemon,
                &SessionResponse::Rows {
                    state: state(1),
                    offset: 5,
                    rows: vec![],
                }
                .encode(),
            )
            .unwrap();
            read_frame(&mut daemon, 1 << 20).unwrap().unwrap();
            write_frame(&mut daemon, &SessionResponse::Error("gone".into()).encode()).unwrap();
        });
        let mut session = SessionClient::open(
            client,
            SessionOpen {
                raw: String::new(),
                sort: None,
            },
        )
        .unwrap();
        assert_eq!(session.state(), state(1));
        assert!(
            session
                .request(&SessionRequest::Fetch { offset: 0, len: 4 })
                .is_err()
        );
        assert_eq!(
            session
                .request(&SessionRequest::Sync)
                .unwrap_err()
                .to_string(),
            "gone"
        );
        server.join().unwrap();
    }
}
