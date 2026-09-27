//! IPC protocol types and serialization.

pub mod session;

pub const MAX_IPC_MESSAGE_SIZE: usize = 256 * 1024 * 1024;
pub const MAX_RESPONSE_PATHS: usize = 1_000_000;
pub const MAX_STATUS_LOG_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Query(QueryRequest),
    StreamQuery(StreamQueryRequest),
    /// Start a result session on this connection; see [`session`].
    OpenSession(session::SessionOpen),
    /// Open with bounded preview rows before the final sorted state.
    OpenSessionPreview(session::SessionOpen),
    Status,
    Flush,
    Reindex,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    pub id: u64,
    pub raw: String,
    pub max_results: usize,
    pub offset: usize,
    pub format: OutputFormat,
    pub highlight: bool,
}

/// Index order avoids a full result-ID buffer; sorted order uses the query's sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamOrder {
    Index,
    Sorted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamQueryRequest {
    pub query: QueryRequest,
    pub order: StreamOrder,
}

pub const STREAM_BATCH_SIZE: usize = 128;
pub const MAX_STREAM_FRAME_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSummary {
    pub id: u64,
    pub total_count: usize,
    pub total_size: u64,
    pub returned_count: usize,
}

/// A stream sends zero or more row batches followed by exactly one completion
/// summary or error. EOF without completion means the stream was interrupted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Rows { id: u64, rows: Vec<ResultRow> },
    Done(StreamSummary),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    Default,
    Csv,
    Tsv,
    Txt,
    Efu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Results(ResultsResponse),
    Status(StatusResponse),
    Ok,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonStatus {
    Starting,
    LoadingConfig,
    LoadingIndex,
    Indexing,
    StartingWatcher,
    Ready,
    Error,
}

impl DaemonStatus {
    pub fn to_u8(&self) -> u8 {
        match self {
            DaemonStatus::Starting => 0,
            DaemonStatus::LoadingConfig => 1,
            DaemonStatus::LoadingIndex => 2,
            DaemonStatus::Indexing => 3,
            DaemonStatus::StartingWatcher => 4,
            DaemonStatus::Ready => 5,
            DaemonStatus::Error => 6,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(DaemonStatus::Starting),
            1 => Some(DaemonStatus::LoadingConfig),
            2 => Some(DaemonStatus::LoadingIndex),
            3 => Some(DaemonStatus::Indexing),
            4 => Some(DaemonStatus::StartingWatcher),
            5 => Some(DaemonStatus::Ready),
            6 => Some(DaemonStatus::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub extension: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_unix: i64,
    pub created_unix: i64,
    pub accessed_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsResponse {
    pub id: u64,
    pub total_count: usize,
    pub total_size: u64,
    pub rows: Vec<ResultRow>,
}

impl ResultsResponse {
    /// Return the full paths of the result rows, matching the legacy payload shape.
    pub fn paths(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.path.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    pub indexed_count: usize,
    pub status: DaemonStatus,
    pub status_message: String,
    pub watcher_healthy: bool,
    pub watched_dir_count: usize,
    pub watch_failure_count: usize,
    pub watch_overflow_count: u64,
    pub watcher_log: Vec<String>,
    pub last_updated_unix: i64,
    pub build_duration_ms: u64,
}

impl OutputFormat {
    fn to_u8(&self) -> u8 {
        match self {
            OutputFormat::Default => 0,
            OutputFormat::Csv => 1,
            OutputFormat::Tsv => 2,
            OutputFormat::Txt => 3,
            OutputFormat::Efu => 4,
        }
    }

    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(OutputFormat::Default),
            1 => Some(OutputFormat::Csv),
            2 => Some(OutputFormat::Tsv),
            3 => Some(OutputFormat::Txt),
            4 => Some(OutputFormat::Efu),
            _ => None,
        }
    }
}

fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_usize(buf: &mut Vec<u8>, v: usize) {
    buf.extend_from_slice(&(v as u64).to_le_bytes());
}

fn push_string(buf: &mut Vec<u8>, s: &str) {
    push_usize(buf, s.len());
    buf.extend_from_slice(s.as_bytes());
}

fn take_u64(buf: &[u8], off: &mut usize) -> Option<u64> {
    if *off + 8 > buf.len() {
        return None;
    }
    let v = u64::from_le_bytes([
        buf[*off],
        buf[*off + 1],
        buf[*off + 2],
        buf[*off + 3],
        buf[*off + 4],
        buf[*off + 5],
        buf[*off + 6],
        buf[*off + 7],
    ]);
    *off += 8;
    Some(v)
}

fn take_usize(buf: &[u8], off: &mut usize) -> Option<usize> {
    take_u64(buf, off).map(|v| v as usize)
}

fn take_string(buf: &[u8], off: &mut usize) -> Option<String> {
    let len = take_usize(buf, off)?;
    if *off + len > buf.len() {
        return None;
    }
    let s = std::str::from_utf8(&buf[*off..*off + len])
        .ok()?
        .to_string();
    *off += len;
    Some(s)
}

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Request::Query(q) => {
                buf.push(1);
                push_u64(&mut buf, q.id);
                push_string(&mut buf, &q.raw);
                push_usize(&mut buf, q.max_results);
                push_usize(&mut buf, q.offset);
                buf.push(q.format.to_u8());
                buf.push(if q.highlight { 1 } else { 0 });
            }
            Request::StreamQuery(stream) => {
                buf = Request::Query(stream.query.clone()).encode();
                buf[0] = 6;
                buf.push(match stream.order {
                    StreamOrder::Index => 0,
                    StreamOrder::Sorted => 1,
                });
            }
            Request::OpenSession(open) | Request::OpenSessionPreview(open) => {
                buf.push(if matches!(self, Request::OpenSessionPreview(_)) {
                    8
                } else {
                    7
                });
                push_string(&mut buf, &open.raw);
                session::push_sort(&mut buf, open.sort);
            }
            Request::Status => buf.push(2),
            Request::Flush => buf.push(3),
            Request::Reindex => buf.push(4),
            Request::Quit => buf.push(5),
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("empty message".into());
        }
        let mut off = 1;
        match bytes[0] {
            1 | 6 => {
                let id = take_u64(bytes, &mut off).ok_or("missing id")?;
                let raw = take_string(bytes, &mut off).ok_or("missing raw")?;
                let max_results = take_usize(bytes, &mut off).ok_or("missing max_results")?;
                let offset = take_usize(bytes, &mut off).ok_or("missing offset")?;
                let format = bytes
                    .get(off)
                    .copied()
                    .and_then(OutputFormat::from_u8)
                    .ok_or("missing format")?;
                off += 1;
                let highlight = bytes.get(off).copied() == Some(1);
                #[allow(unused_assignments)]
                {
                    off += 1;
                }
                let query = QueryRequest {
                    id,
                    raw,
                    max_results,
                    offset,
                    format,
                    highlight,
                };
                if bytes[0] == 6 {
                    let order = match bytes.get(off) {
                        Some(0) => StreamOrder::Index,
                        Some(1) => StreamOrder::Sorted,
                        _ => return Err("missing or invalid stream order".into()),
                    };
                    Ok(Request::StreamQuery(StreamQueryRequest { query, order }))
                } else {
                    Ok(Request::Query(query))
                }
            }
            7 | 8 => {
                let raw = take_string(bytes, &mut off).ok_or("missing raw")?;
                let sort = session::take_sort(bytes, &mut off).ok_or("invalid sort")?;
                if off != bytes.len() {
                    return Err("trailing session bytes".into());
                }
                let open = session::SessionOpen { raw, sort };
                Ok(if bytes[0] == 8 {
                    Request::OpenSessionPreview(open)
                } else {
                    Request::OpenSession(open)
                })
            }
            2 => Ok(Request::Status),
            3 => Ok(Request::Flush),
            4 => Ok(Request::Reindex),
            5 => Ok(Request::Quit),
            _ => Err("unknown request type".into()),
        }
    }
}

fn encode_results(id: u64, total_count: usize, total_size: u64, rows: &[ResultRow]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(1);
    push_u64(&mut buf, id);
    push_usize(&mut buf, total_count);
    push_u64(&mut buf, total_size);
    push_usize(&mut buf, rows.len());
    for row in rows {
        push_string(&mut buf, &row.path);
        push_string(&mut buf, &row.name);
        push_string(&mut buf, &row.parent);
        push_string(&mut buf, &row.extension);
        buf.push(if row.is_dir { 1 } else { 0 });
        push_u64(&mut buf, row.size);
        push_u64(&mut buf, row.modified_unix as u64);
        push_u64(&mut buf, row.created_unix as u64);
        push_u64(&mut buf, row.accessed_unix as u64);
    }
    buf
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Response::Results(r) => {
                buf = encode_results(r.id, r.total_count, r.total_size, &r.rows);
            }
            Response::Status(s) => {
                buf.push(2);
                push_usize(&mut buf, s.indexed_count);
                buf.push(s.status.to_u8());
                push_string(&mut buf, &s.status_message);
                buf.push(if s.watcher_healthy { 1 } else { 0 });
                push_usize(&mut buf, s.watched_dir_count);
                push_usize(&mut buf, s.watch_failure_count);
                push_u64(&mut buf, s.watch_overflow_count);
                push_usize(&mut buf, s.watcher_log.len());
                for entry in &s.watcher_log {
                    push_string(&mut buf, entry);
                }
                push_u64(&mut buf, s.last_updated_unix as u64);
                push_u64(&mut buf, s.build_duration_ms);
            }
            Response::Ok => buf.push(3),
            Response::Error(e) => {
                buf.push(4);
                push_string(&mut buf, e);
            }
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("empty message".into());
        }
        let mut off = 1;
        match bytes[0] {
            1 => {
                let id = take_u64(bytes, &mut off).ok_or("missing id")?;
                let total_count = take_usize(bytes, &mut off).ok_or("missing total_count")?;
                let total_size = take_u64(bytes, &mut off).unwrap_or(0);
                let row_count = take_usize(bytes, &mut off).ok_or("missing row_count")?;
                if row_count > MAX_RESPONSE_PATHS {
                    return Err("too many rows".into());
                }
                let mut rows = Vec::with_capacity(row_count);
                for _ in 0..row_count {
                    let path = take_string(bytes, &mut off).ok_or("missing row path")?;
                    let name = take_string(bytes, &mut off).ok_or("missing row name")?;
                    let parent = take_string(bytes, &mut off).ok_or("missing row parent")?;
                    let extension = take_string(bytes, &mut off).ok_or("missing row extension")?;
                    let is_dir = bytes.get(off).copied() == Some(1);
                    off += 1;
                    let size = take_u64(bytes, &mut off).ok_or("missing row size")?;
                    let modified_unix =
                        take_u64(bytes, &mut off).ok_or("missing row modified")? as i64;
                    let created_unix =
                        take_u64(bytes, &mut off).ok_or("missing row created")? as i64;
                    let accessed_unix =
                        take_u64(bytes, &mut off).ok_or("missing row accessed")? as i64;
                    rows.push(ResultRow {
                        path,
                        name,
                        parent,
                        extension,
                        is_dir,
                        size,
                        modified_unix,
                        created_unix,
                        accessed_unix,
                    });
                }
                Ok(Response::Results(ResultsResponse {
                    id,
                    total_count,
                    total_size,
                    rows,
                }))
            }
            2 => {
                let indexed_count = take_usize(bytes, &mut off).ok_or("missing indexed_count")?;
                let status_u8 = bytes.get(off).copied().ok_or("missing status")?;
                off += 1;
                let status = DaemonStatus::from_u8(status_u8).ok_or("invalid status")?;
                let status_message =
                    take_string(bytes, &mut off).ok_or("missing status_message")?;
                let watcher_healthy = bytes.get(off).copied() == Some(1);
                off += 1;
                let watched_dir_count =
                    take_usize(bytes, &mut off).ok_or("missing watched_dir_count")?;
                let watch_failure_count =
                    take_usize(bytes, &mut off).ok_or("missing watch_failure_count")?;
                let watch_overflow_count =
                    take_u64(bytes, &mut off).ok_or("missing watch_overflow_count")?;
                let remaining = bytes.len().saturating_sub(off);
                let mut watcher_log = Vec::new();
                if remaining != 16 {
                    let watcher_log_count =
                        take_usize(bytes, &mut off).ok_or("missing watcher_log_count")?;
                    if watcher_log_count > MAX_STATUS_LOG_ENTRIES {
                        return Err("too many watcher log entries".into());
                    }
                    watcher_log = Vec::with_capacity(watcher_log_count);
                    for _ in 0..watcher_log_count {
                        watcher_log
                            .push(take_string(bytes, &mut off).ok_or("missing watcher_log entry")?);
                    }
                }
                let last_updated_unix =
                    take_u64(bytes, &mut off).ok_or("missing last_updated")? as i64;
                let build_duration_ms =
                    take_u64(bytes, &mut off).ok_or("missing build_duration")?;
                Ok(Response::Status(StatusResponse {
                    indexed_count,
                    status,
                    status_message,
                    watcher_healthy,
                    watched_dir_count,
                    watch_failure_count,
                    watch_overflow_count,
                    watcher_log,
                    last_updated_unix,
                    build_duration_ms,
                }))
            }
            3 => Ok(Response::Ok),
            4 => {
                let e = take_string(bytes, &mut off).ok_or("missing error")?;
                Ok(Response::Error(e))
            }
            _ => Err("unknown response type".into()),
        }
    }
}

#[cfg(test)]
mod tests;

impl StreamEvent {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Rows { id, rows } => encode_results(*id, 0, 0, rows),
            Self::Error(error) => Response::Error(error.clone()).encode(),
            Self::Done(summary) => {
                let mut buf = vec![5];
                push_u64(&mut buf, summary.id);
                push_usize(&mut buf, summary.total_count);
                push_u64(&mut buf, summary.total_size);
                push_usize(&mut buf, summary.returned_count);
                buf
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_STREAM_FRAME_SIZE {
            return Err("stream frame too large".into());
        }
        if !matches!(bytes.first(), Some(1 | 4 | 5)) {
            return Err("unexpected stream event".into());
        }
        if bytes.first() == Some(&5) {
            let mut off = 1;
            let summary = StreamSummary {
                id: take_u64(bytes, &mut off).ok_or("missing stream id")?,
                total_count: take_usize(bytes, &mut off).ok_or("missing stream total count")?,
                total_size: take_u64(bytes, &mut off).ok_or("missing stream total size")?,
                returned_count: take_usize(bytes, &mut off)
                    .ok_or("missing stream returned count")?,
            };
            if off != bytes.len() {
                return Err("trailing stream summary bytes".into());
            }
            return Ok(Self::Done(summary));
        }
        // Check the row count before allocating the decoded row vector.
        if bytes.first() == Some(&1) {
            let mut off = 25;
            if take_usize(bytes, &mut off).ok_or("missing stream row count")? > STREAM_BATCH_SIZE {
                return Err("stream batch too large".into());
            }
        }
        match Response::decode(bytes)? {
            Response::Results(results) => Ok(Self::Rows {
                id: results.id,
                rows: results.rows,
            }),
            Response::Error(error) => Ok(Self::Error(error)),
            _ => Err("unexpected stream event".into()),
        }
    }
}

/// Consume a daemon stream one batch at a time. Returning an error from the
/// callback stops consumption; drop/close the connection to cancel the server.
/// Totals are available only when the completion summary arrives.
pub fn stream_query<S: std::io::Read + std::io::Write>(
    connection: &mut S,
    request: &StreamQueryRequest,
    mut on_rows: impl FnMut(&[ResultRow]) -> std::io::Result<()>,
) -> std::io::Result<StreamSummary> {
    use std::io::{Error, ErrorKind};
    let request_bytes = Request::StreamQuery(request.clone()).encode();
    connection.write_all(&(request_bytes.len() as u64).to_le_bytes())?;
    connection.write_all(&request_bytes)?;
    connection.flush()?;
    let mut returned = 0usize;
    loop {
        let mut len = [0; 8];
        connection.read_exact(&mut len)?;
        let len = u64::from_le_bytes(len);
        if len > MAX_STREAM_FRAME_SIZE as u64 {
            return Err(Error::new(ErrorKind::InvalidData, "stream frame too large"));
        }
        let mut bytes = vec![0; len as usize];
        connection.read_exact(&mut bytes)?;
        match StreamEvent::decode(&bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))? {
            StreamEvent::Rows { id, rows } if id == request.query.id => {
                returned = returned
                    .checked_add(rows.len())
                    .ok_or_else(|| Error::new(ErrorKind::InvalidData, "stream count overflow"))?;
                if returned > request.query.max_results {
                    return Err(Error::new(ErrorKind::InvalidData, "too many streamed rows"));
                }
                on_rows(&rows)?;
            }
            StreamEvent::Done(summary)
                if summary.id == request.query.id
                    && summary.returned_count == returned
                    && returned
                        == summary
                            .total_count
                            .saturating_sub(request.query.offset)
                            .min(request.query.max_results) =>
            {
                return Ok(summary);
            }
            StreamEvent::Error(error) => return Err(Error::other(error)),
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "inconsistent stream response",
                ));
            }
        }
    }
}
