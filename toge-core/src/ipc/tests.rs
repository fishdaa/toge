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
        rows: vec![
            ResultRow {
                path: "/a.txt".into(),
                name: "a.txt".into(),
                parent: "/".into(),
                extension: "txt".into(),
                is_dir: false,
                size: 12,
                modified_unix: 1700000000,
                created_unix: 1700000000,
                accessed_unix: 1700000000,
            },
            ResultRow {
                path: "/b.txt".into(),
                name: "b.txt".into(),
                parent: "/".into(),
                extension: "txt".into(),
                is_dir: false,
                size: 34,
                modified_unix: 1700000001,
                created_unix: 1700000001,
                accessed_unix: 1700000001,
            },
        ],
    });
    let bytes = resp.encode();
    let decoded = Response::decode(&bytes).unwrap();
    assert_eq!(resp, decoded);
}

#[test]
fn test_response_status_roundtrip() {
    let resp = Response::Status(StatusResponse {
        indexed_count: 1234,
        status: DaemonStatus::Ready,
        status_message: "Indexed 1234 entries in 567ms".to_string(),
        watcher_healthy: true,
        watched_dir_count: 12,
        watch_failure_count: 1,
        watch_overflow_count: 2,
        watcher_log: vec![
            "12:00:00 create /downloads/movie.mkv".to_string(),
            "12:00:01 modify /downloads/movie.mkv".to_string(),
        ],
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

#[test]
fn test_request_decode_rejects_invalid_format_byte() {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&3u64.to_le_bytes());
    bytes.extend_from_slice(b"foo");
    bytes.extend_from_slice(&10u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.push(99);
    bytes.push(0);

    let err = Request::decode(&bytes).unwrap_err();
    assert_eq!(err, "missing format");
}

#[test]
fn test_response_decode_rejects_excessive_row_count() {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&((MAX_RESPONSE_PATHS + 1) as u64).to_le_bytes());

    let err = Response::decode(&bytes).unwrap_err();
    assert!(err.contains("too many rows"));
}

#[test]
fn test_response_decode_rejects_truncated_row_payload() {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&10u64.to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&5u64.to_le_bytes());
    bytes.extend_from_slice(b"abc");

    let err = Response::decode(&bytes).unwrap_err();
    assert_eq!(err, "missing row path");
}

#[test]
fn test_response_status_decode_supports_legacy_payload_without_watcher_log() {
    let message = "Indexed 1234 entries in 567ms";
    let mut bytes = vec![2];
    bytes.extend_from_slice(&1234u64.to_le_bytes()); // indexed_count
    bytes.push(DaemonStatus::Ready.to_u8()); // status
    bytes.extend_from_slice(&(message.len() as u64).to_le_bytes()); // status_message len
    bytes.extend_from_slice(message.as_bytes());
    bytes.push(1); // watcher_healthy
    bytes.extend_from_slice(&12u64.to_le_bytes()); // watched_dir_count
    bytes.extend_from_slice(&1u64.to_le_bytes()); // watch_failure_count
    bytes.extend_from_slice(&2u64.to_le_bytes()); // watch_overflow_count
    bytes.extend_from_slice(&1700000000u64.to_le_bytes()); // last_updated_unix
    bytes.extend_from_slice(&567u64.to_le_bytes()); // build_duration_ms

    let decoded = Response::decode(&bytes).unwrap();
    assert_eq!(
        decoded,
        Response::Status(StatusResponse {
            indexed_count: 1234,
            status: DaemonStatus::Ready,
            status_message: "Indexed 1234 entries in 567ms".to_string(),
            watcher_healthy: true,
            watched_dir_count: 12,
            watch_failure_count: 1,
            watch_overflow_count: 2,
            watcher_log: vec![],
            last_updated_unix: 1700000000,
            build_duration_ms: 567,
        })
    );
}
