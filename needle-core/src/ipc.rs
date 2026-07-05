//! IPC protocol types and serialization.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Query(QueryRequest),
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
pub struct ResultsResponse {
    pub id: u64,
    pub total_count: usize,
    pub total_size: u64,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    pub indexed_count: usize,
    pub is_ready: bool,
    pub watcher_healthy: bool,
    pub watched_dir_count: usize,
    pub watch_failure_count: usize,
    pub watch_overflow_count: u64,
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
            1 => {
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
                Ok(Request::Query(QueryRequest {
                    id,
                    raw,
                    max_results,
                    offset,
                    format,
                    highlight,
                }))
            }
            2 => Ok(Request::Status),
            3 => Ok(Request::Flush),
            4 => Ok(Request::Reindex),
            5 => Ok(Request::Quit),
            _ => Err("unknown request type".into()),
        }
    }
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Response::Results(r) => {
                buf.push(1);
                push_u64(&mut buf, r.id);
                push_usize(&mut buf, r.total_count);
                push_u64(&mut buf, r.total_size);
                push_usize(&mut buf, r.paths.len());
                for p in &r.paths {
                    push_string(&mut buf, p);
                }
            }
            Response::Status(s) => {
                buf.push(2);
                push_usize(&mut buf, s.indexed_count);
                buf.push(if s.is_ready { 1 } else { 0 });
                buf.push(if s.watcher_healthy { 1 } else { 0 });
                push_usize(&mut buf, s.watched_dir_count);
                push_usize(&mut buf, s.watch_failure_count);
                push_u64(&mut buf, s.watch_overflow_count);
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
                let path_count = take_usize(bytes, &mut off).ok_or("missing path_count")?;
                let mut paths = Vec::with_capacity(path_count);
                for _ in 0..path_count {
                    paths.push(take_string(bytes, &mut off).ok_or("missing path")?);
                }
                Ok(Response::Results(ResultsResponse {
                    id,
                    total_count,
                    total_size,
                    paths,
                }))
            }
            2 => {
                let indexed_count = take_usize(bytes, &mut off).ok_or("missing indexed_count")?;
                let is_ready = bytes.get(off).copied() == Some(1);
                off += 1;
                let watcher_healthy = bytes.get(off).copied() == Some(1);
                off += 1;
                let watched_dir_count =
                    take_usize(bytes, &mut off).ok_or("missing watched_dir_count")?;
                let watch_failure_count =
                    take_usize(bytes, &mut off).ok_or("missing watch_failure_count")?;
                let watch_overflow_count =
                    take_u64(bytes, &mut off).ok_or("missing watch_overflow_count")?;
                let last_updated_unix =
                    take_u64(bytes, &mut off).ok_or("missing last_updated")? as i64;
                let build_duration_ms =
                    take_u64(bytes, &mut off).ok_or("missing build_duration")?;
                Ok(Response::Status(StatusResponse {
                    indexed_count,
                    is_ready,
                    watcher_healthy,
                    watched_dir_count,
                    watch_failure_count,
                    watch_overflow_count,
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
