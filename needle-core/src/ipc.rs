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
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    pub indexed_count: usize,
    pub is_ready: bool,
    pub last_updated_unix: i64,
    pub build_duration_ms: u64,
}

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        let _ = self;
        todo!()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let _ = bytes;
        todo!()
    }
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let _ = self;
        todo!()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let _ = bytes;
        todo!()
    }
}

#[cfg(test)]
mod tests;
