use super::*;

#[test]
fn test_request_query_roundtrip() {
    let req = Request::Query(QueryRequest {
        id: 7,
        raw: "foo bar".into(),
        max_results: 100,
        offset: 0,
        format: OutputFormat::Default,
        highlight: false,
    });
    let bytes = req.encode();
    let decoded = Request::decode(&bytes).unwrap();
    assert_eq!(req, decoded);
}

#[test]
fn test_request_status_roundtrip() {
    let req = Request::Status;
    let bytes = req.encode();
    let decoded = Request::decode(&bytes).unwrap();
    assert_eq!(req, decoded);
}

#[test]
fn test_response_results_roundtrip() {
    let resp = Response::Results(ResultsResponse {
        id: 1,
        total_count: 42,
        total_size: 0,
        paths: vec!["/a.txt".into(), "/b.txt".into()],
    });
    let bytes = resp.encode();
    let decoded = Response::decode(&bytes).unwrap();
    assert_eq!(resp, decoded);
}

#[test]
fn test_response_status_roundtrip() {
    let resp = Response::Status(StatusResponse {
        indexed_count: 1234,
        is_ready: true,
        watcher_healthy: true,
        watched_dir_count: 12,
        watch_failure_count: 1,
        watch_overflow_count: 2,
        last_updated_unix: 1700000000,
        build_duration_ms: 567,
    });
    let bytes = resp.encode();
    let decoded = Response::decode(&bytes).unwrap();
    assert_eq!(resp, decoded);
}

#[test]
fn test_decode_garbage_returns_error() {
    assert!(Response::decode(b"not a valid message").is_err());
}
