//! toged — background indexing daemon.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use toge_core::config::Config;
use toge_core::index::Index;
use toge_core::ipc::{
    DaemonStatus, MAX_IPC_MESSAGE_SIZE, MAX_STREAM_FRAME_SIZE, QueryRequest, Request, Response,
    ResultRow, ResultsResponse, STREAM_BATCH_SIZE, StatusResponse, StreamEvent, StreamOrder,
    StreamQueryRequest, StreamSummary,
};
use toge_core::matcher::{QueryMatcher, candidate_ids, match_query};
use toge_core::query::Query;
use toge_core::sort::{OrderCache, SortKey};
use toge_core::sys::FsWatcher;
use toge_core::sys::{FanotifyWatcher, WatchEvent};
use toge_core::walker::{
    Excludes, excluded_under_roots, has_hidden_ancestor_dir, is_hidden_dir_path, reconcile_live,
    walk,
};

mod session;

struct DaemonState {
    index: Index,
    status: DaemonStatus,
    status_message: String,
    build_duration_ms: u64,
    last_updated_unix: i64,
    watcher: WatcherStatus,
    watcher_log: Vec<String>,
    /// Whole-index name/path orders shared by all queries.
    orders: OrderCache,
    /// Bumped by [`replace_index`], so a background reconcile can tell that the
    /// index it was updating is gone.
    index_generation: u64,
}

#[derive(Clone, Debug, Default)]
struct WatcherStatus {
    is_healthy: bool,
    watched_dir_count: usize,
    watch_failure_count: usize,
    watch_overflow_count: u64,
}

const WATCHER_LOG_LIMIT: usize = 50;
const WATCHER_REMEDIATION: &str = "Live updates unavailable: fanotify setup failed. Reinstall the DEB/RPM package or run `sudo setcap cap_sys_admin,cap_dac_read_search+ep /usr/bin/toged`, then restart Toge.";

fn append_watcher_log(st: &mut DaemonState, message: impl Into<String>) {
    let timestamp = current_unix_time();
    st.last_updated_unix = timestamp;
    st.watcher_log
        .push(format!("[{}] {}", timestamp, message.into()));
    if st.watcher_log.len() > WATCHER_LOG_LIMIT {
        let excess = st.watcher_log.len() - WATCHER_LOG_LIMIT;
        st.watcher_log.drain(0..excess);
    }
}

fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

fn set_owner_only(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected peer credential size",
        ));
    }
    Ok(cred.uid)
}

fn authorize_peer(stream: &UnixStream) -> io::Result<()> {
    let peer = peer_uid(stream)?;
    let owner = unsafe { libc::geteuid() };
    if peer != owner {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unauthorized peer uid",
        ));
    }
    Ok(())
}

fn usage() {
    println!("toged [options]");
    println!("Options:");
    println!("  --socket <path>     Unix domain socket path");
    println!("  --config <path>     Config file path");
    println!("  --state-dir <path>  State directory (for index.bin)");
    println!("  --clean             Delete old index before starting");
    println!("  -h, --help          Show this help");
    println!("  -v, --version       Show version");
}

fn version() {
    println!("toged 0.1.1");
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

fn default_config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME").expect("HOME not set");
            PathBuf::from(home).join(".config")
        })
        .join("toge")
}

fn discover_roots(config: &Config) -> Vec<PathBuf> {
    if !config.roots.is_empty() {
        return config.roots.clone();
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .into_iter()
        .collect()
}

fn index_excludes(config: &Config) -> Excludes {
    Excludes {
        skip_hidden: config.exclude_hidden,
        skip_system_paths: true,
        patterns: config.exclude_patterns.clone(),
        folders: config.exclude_folders.clone(),
        paths: Vec::new(),
        include_only: config.include_only.clone(),
    }
}

fn fetches_metadata(config: &Config) -> bool {
    config.index_size
        || config.index_date_modified
        || config.index_date_created
        || config.index_date_accessed
}

/// Walk the configured roots into a fresh index.
fn build_index(config: &Config, state: &Arc<Mutex<DaemonState>>) -> (Index, u64) {
    let start = Instant::now();
    let excludes = index_excludes(config);
    let fetch_metadata = fetches_metadata(config);
    let roots = discover_roots(config);

    let mut index = Index::new();
    let total_roots = roots.len();
    for (i, root) in roots.iter().enumerate() {
        {
            let mut st = state.lock().unwrap();
            st.status = DaemonStatus::Indexing;
            st.status_message = format!("Indexing {}/{}: {}", i + 1, total_roots, root.display());
        }
        walk(root, &mut index, &excludes, fetch_metadata);
    }

    index.compact();
    let duration_ms = start.elapsed().as_millis() as u64;
    (index, duration_ms)
}

/// Swap in a newly built index, invalidating IDs handed out by the old one.
fn replace_index(st: &mut DaemonState, mut index: Index) {
    index.succeed(&st.index);
    st.index = index;
    st.index_generation += 1;
}

/// Load the startup index and hand over to the watcher.
///
/// A cached index is served immediately and then reconciled with the disk in the
/// background, since changes made while the daemon was stopped can only be found
/// by walking. Without a usable cache, a fresh index is built first.
fn start_index(
    state_dir: &Path,
    config: &Config,
    state: &Arc<Mutex<DaemonState>>,
    watcher_failure: Option<&str>,
) {
    let start = Instant::now();
    let Ok(cached) = Index::load(&state_dir.join("index.bin")) else {
        let (index, duration) = build_index(config, state);
        let _ = save_index(&index, state_dir);
        let mut st = state.lock().unwrap();
        replace_index(&mut st, index);
        st.build_duration_ms = duration;
        st.last_updated_unix = current_unix_time();
        hand_off_to_watcher(&mut st, watcher_failure);
        return;
    };

    let generation = {
        let mut st = state.lock().unwrap();
        replace_index(&mut st, cached);
        st.build_duration_ms = start.elapsed().as_millis() as u64;
        st.last_updated_unix = current_unix_time();
        hand_off_to_watcher(&mut st, watcher_failure);
        st.index_generation
    };

    // Walk only once the watcher is installed: every change after that point is
    // seen by the watcher, and every change before it by the walk.
    loop {
        {
            let mut st = state.lock().unwrap();
            if st.status != DaemonStatus::StartingWatcher {
                if st.watcher.is_healthy {
                    st.status_message = format!("Reconciling {} cached entries", st.index.count());
                }
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    if !reconcile_served_index(config, state, generation) {
        return;
    }
    let mut st = state.lock().unwrap();
    let _ = save_index(&st.index, state_dir);
    st.build_duration_ms = start.elapsed().as_millis() as u64;
    st.last_updated_unix = current_unix_time();
    if st.status == DaemonStatus::Ready && st.watcher.is_healthy {
        st.status_message = format!(
            "Indexed {} entries in {}ms",
            st.index.count(),
            st.build_duration_ms
        );
    }
}

/// Let the watcher thread take over once the index is served. Without a
/// watcher thread nothing would leave `StartingWatcher`, so go straight to Ready.
fn hand_off_to_watcher(st: &mut DaemonState, watcher_failure: Option<&str>) {
    if let Some(detail) = watcher_failure {
        mark_watcher_unavailable_locked(st, detail);
    } else {
        st.status = DaemonStatus::StartingWatcher;
        st.status_message = "Setting up file watcher".to_string();
    }
}

/// Reconcile the served index with the disk while it stays searchable.
/// Returns false, leaving the rest to the new owner, if the index was
/// replaced (generation `generation` ended) in the meantime.
fn reconcile_served_index(
    config: &Config,
    state: &Arc<Mutex<DaemonState>>,
    generation: u64,
) -> bool {
    reconcile_live(
        &discover_roots(config),
        &index_excludes(config),
        fetches_metadata(config),
        |step| {
            let mut st = state.lock().unwrap();
            if st.index_generation != generation {
                return false;
            }
            step(&mut st.index);
            true
        },
    );
    let mut st = state.lock().unwrap();
    if st.index_generation != generation {
        return false;
    }
    st.index.compact();
    true
}

fn save_index(index: &Index, state_dir: &Path) -> io::Result<()> {
    ensure_private_dir(state_dir)?;
    let path = state_dir.join("index.bin");
    index.save(&path)?;
    Ok(())
}

fn current_unix_time() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn is_ignored_path(path: &str, state_dir: &Path, config_dir: &Path, is_dir: bool) -> bool {
    let path = Path::new(path);
    canonical_starts_with(path, state_dir)
        || canonical_starts_with(path, config_dir)
        || has_hidden_ancestor_dir(path)
        || (is_dir
            && path
                .file_name()
                .map(|name| {
                    let bytes = name.as_encoded_bytes();
                    bytes.len() > 1 && bytes.starts_with(b".")
                })
                .unwrap_or(false))
}

fn is_within_roots(path: &str, roots: &[PathBuf]) -> bool {
    let path = Path::new(path);
    roots.iter().any(|root| path_matches_root(path, root))
}

fn path_matches_root(path: &Path, root: &Path) -> bool {
    match (fs::canonicalize(path), fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        (_, Ok(root)) => path.starts_with(&root),
        _ => path.starts_with(root),
    }
}

fn metadata_snapshot(path: &str) -> (u64, i64, i64, i64) {
    let now = current_unix_time();
    let Ok(metadata) = fs::metadata(path) else {
        return (0, now, now, now);
    };

    let read_time = |value: io::Result<SystemTime>| {
        value
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(now)
    };

    (
        metadata.len(),
        read_time(metadata.modified()),
        read_time(metadata.created()),
        read_time(metadata.accessed()),
    )
}

fn index_created_path(st: &mut DaemonState, path: &str, is_dir: bool, config: &Config) {
    index_created_path_with(st, path, is_dir, metadata_snapshot(path), config);
}

/// [`index_created_path`] with metadata the caller already read, so the
/// `stat` can happen before the index lock is taken.
fn index_created_path_with(
    st: &mut DaemonState,
    path: &str,
    is_dir: bool,
    metadata: (u64, i64, i64, i64),
    config: &Config,
) {
    let (size, modified, created, accessed) = metadata;
    st.index
        .insert_with_metadata(path, is_dir, size, modified, created, accessed);

    if is_dir {
        let excludes = Excludes {
            skip_hidden: config.exclude_hidden,
            skip_system_paths: true,
            patterns: config.exclude_patterns.clone(),
            folders: config.exclude_folders.clone(),
            paths: Vec::new(),
            include_only: config.include_only.clone(),
        };
        walk(
            Path::new(path),
            &mut st.index,
            &excludes,
            config.index_size
                || config.index_date_modified
                || config.index_date_created
                || config.index_date_accessed,
        );
    }
}

fn remove_deleted_path(index: &mut Index, path: &str) {
    // Only an indexed directory needs the scan over every entry: a file has no
    // descendants, and a directory is always indexed before its contents, so
    // an unindexed path has none either. (Entries left behind by lost watcher
    // events are removed by the reconcile that an overflow triggers.)
    match index.id_by_path(path) {
        Some(id) if index.entries[id as usize].path == path => {
            if !index.entries[id as usize].is_dir {
                index.remove(path);
                return;
            }
        }
        _ => return,
    }
    let deleted = Path::new(path);
    // Indexed paths are normalized, so every descendant starts with the
    // normalized path as a string; that cheap check skips parsing components
    // of every entry while the index lock is held.
    let prefix: PathBuf = deleted.components().collect();
    let prefix = prefix.to_string_lossy();
    let paths: Vec<String> = index
        .entries
        .iter()
        .filter(|entry| {
            entry.path.starts_with(prefix.as_ref()) && Path::new(&entry.path).starts_with(deleted)
        })
        .map(|entry| entry.path.clone())
        .collect();
    for path in paths {
        index.remove(&path);
    }
}

fn mark_watcher_unavailable(state: &Arc<Mutex<DaemonState>>, detail: &str) {
    mark_watcher_unavailable_locked(&mut state.lock().unwrap(), detail);
}

fn mark_watcher_unavailable_locked(st: &mut DaemonState, detail: &str) {
    st.watcher.is_healthy = false;
    st.watcher.watch_failure_count = st.watcher.watch_failure_count.max(1);
    st.status = DaemonStatus::Ready;
    st.status_message = WATCHER_REMEDIATION.to_string();
    append_watcher_log(
        st,
        format!("fanotify setup failed: {detail}; {WATCHER_REMEDIATION}"),
    );
}

fn status_response(st: &DaemonState) -> StatusResponse {
    StatusResponse {
        indexed_count: st.index.count(),
        status: st.status.clone(),
        status_message: st.status_message.clone(),
        watcher_healthy: st.watcher.is_healthy,
        watched_dir_count: st.watcher.watched_dir_count,
        watch_failure_count: st.watcher.watch_failure_count,
        watch_overflow_count: st.watcher.watch_overflow_count,
        watcher_log: st.watcher_log.clone(),
        last_updated_unix: st.last_updated_unix,
        build_duration_ms: st.build_duration_ms,
    }
}

fn install_watches(watcher: &mut FanotifyWatcher, dirs: &[PathBuf]) -> WatcherStatus {
    let mut watcher_status = WatcherStatus {
        watched_dir_count: 0,
        watch_failure_count: 0,
        ..WatcherStatus::default()
    };

    for dir in dirs {
        match watcher.watch(dir) {
            Ok(()) => {}
            Err(error) => {
                watcher_status.watch_failure_count += 1;
                eprintln!(
                    "Failed to install fanotify filesystem watch for {}: {}",
                    dir.display(),
                    error
                );
            }
        }
    }

    watcher_status.watched_dir_count = watcher.fs_count();
    watcher_status.is_healthy = watcher_status.watch_failure_count == 0;
    watcher_status
}

fn handle_request(
    req: Request,
    state_dir: &Path,
    config: &Config,
    state: &Arc<Mutex<DaemonState>>,
) -> Response {
    match req {
        Request::Flush => {
            let st = state.lock().unwrap();
            match save_index(&st.index, state_dir) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(e.to_string()),
            }
        }
        Request::Reindex => {
            let _ = fs::remove_file(state_dir.join("index.bin"));
            let mut st = state.lock().unwrap();
            st.status = DaemonStatus::Indexing;
            st.status_message = "Reindexing".to_string();
            drop(st);
            let (new_index, duration) = build_index(config, state);
            if let Err(e) = save_index(&new_index, state_dir) {
                return Response::Error(e.to_string());
            }
            let mut st = state.lock().unwrap();
            replace_index(&mut st, new_index);
            st.build_duration_ms = duration;
            st.last_updated_unix = current_unix_time();
            st.status = DaemonStatus::Ready;
            st.status_message = format!("Indexed {} entries", st.index.count());
            Response::Ok
        }
        Request::Status => {
            let st = state.lock().unwrap();
            Response::Status(status_response(&st))
        }
        Request::Query(q) => {
            let mut st = state.lock().unwrap();
            if st.status != DaemonStatus::Ready {
                return Response::Error("daemon not ready".into());
            }
            let st = &mut *st;
            handle_query(&mut st.index, &mut st.orders, &q, config.index_size)
        }
        Request::StreamQuery(_) | Request::OpenSession(_) | Request::OpenSessionPreview(_) => {
            Response::Error("stream request requires a streaming connection".into())
        }
        Request::Quit => unreachable!(),
    }
}

fn handle_query(
    index: &mut Index,
    orders: &mut OrderCache,
    q: &QueryRequest,
    index_size: bool,
) -> Response {
    let query = match Query::parse(&q.raw) {
        Ok(query) => query,
        Err(e) => return Response::Error(e.to_string()),
    };

    let ids = prepare_query_ids(index, orders, &query, index_size);

    let total = ids.len();
    let total_size: u64 = ids.iter().map(|id| index.entries[*id as usize].size).sum();

    let offset = q.offset.min(total);
    let end = offset.saturating_add(q.max_results).min(total);
    let page = &ids[offset..end];

    let rows = page
        .iter()
        .map(|id| result_row(&index.entries[*id as usize], &query, q.highlight))
        .collect();

    Response::Results(ResultsResponse {
        id: q.id,
        total_count: total,
        total_size,
        rows,
    })
}

fn prepare_query_ids(
    index: &mut Index,
    orders: &mut OrderCache,
    query: &Query,
    index_size: bool,
) -> Vec<u32> {
    let (sort_key, ascending) = sort_params(query.sort);

    // Indexing, reconciliation and watcher events maintain cached metadata.
    // Hydrate missing date fields instead of stat-ing every match on every
    // query/rebuild while holding the shared index lock.
    let needs_all_metadata = query.date_modified.is_some()
        || query.date_created.is_some()
        || query.date_accessed.is_some();
    if needs_all_metadata {
        for id in 0..index.count() as u32 {
            if missing_query_dates(&index.entries[id as usize], query) {
                index.update_metadata_by_id(id);
            }
        }
    }

    let mut ids = match_query(index, query);

    if matches!(
        sort_key,
        SortKey::Modified | SortKey::Created | SortKey::Accessed
    ) && !needs_all_metadata
    {
        for &id in &ids {
            if missing_sort_date(&index.entries[id as usize], sort_key) {
                index.update_metadata_by_id(id);
            }
        }
    } else if index_size {
        for id in &ids {
            let entry = &index.entries[*id as usize];
            if entry.is_dir || entry.size != 0 {
                continue;
            }
            index.update_metadata_by_id(*id);
        }
    }

    orders.sort(index, &mut ids, sort_key, ascending);

    ids
}

fn missing_sort_date(entry: &toge_core::index::Entry, key: SortKey) -> bool {
    match key {
        SortKey::Modified => entry.modified == 0,
        SortKey::Created => entry.created == 0,
        SortKey::Accessed => entry.accessed == 0,
        _ => false,
    }
}

fn missing_query_dates(entry: &toge_core::index::Entry, query: &Query) -> bool {
    (query.date_modified.is_some() && entry.modified == 0)
        || (query.date_created.is_some() && entry.created == 0)
        || (query.date_accessed.is_some() && entry.accessed == 0)
}

fn result_row(entry: &toge_core::index::Entry, query: &Query, highlight: bool) -> ResultRow {
    let display_path = if highlight && !query.terms.is_empty() {
        highlight_path(&entry.path, query)
    } else {
        entry.path.clone()
    };
    let name = entry.name().to_string();
    let parent_end = entry.name_off as usize;
    let parent = if parent_end > 0 {
        entry.path[..parent_end.saturating_sub(1)].to_string()
    } else {
        String::new()
    };
    ResultRow {
        path: display_path,
        name,
        parent,
        extension: entry.extension().to_string(),
        is_dir: entry.is_dir,
        size: entry.size,
        modified_unix: entry.modified,
        created_unix: entry.created,
        accessed_unix: entry.accessed,
    }
}

// A consistent stream holds the index lock, avoiding an O(N) snapshot copy.
// Bound the duration so disconnected or stalled consumers cannot pin it indefinitely.
fn write_stream_event(
    stream: &mut UnixStream,
    event: &StreamEvent,
    deadline: Instant,
) -> io::Result<()> {
    let bytes = event.encode();
    if bytes.len() > MAX_STREAM_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stream frame too large",
        ));
    }
    let length = (bytes.len() as u64).to_le_bytes();
    for mut pending in [length.as_slice(), bytes.as_slice()] {
        while !pending.is_empty() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|duration| !duration.is_zero())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "stream deadline exceeded")
                })?;
            stream.set_write_timeout(Some(remaining.min(Duration::from_secs(5))))?;
            match stream.write(pending) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "stream closed")),
                Ok(written) => pending = &pending[written..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

fn stream_results(
    stream: &mut UnixStream,
    request: &StreamQueryRequest,
    index: &mut Index,
    orders: &mut OrderCache,
    index_size: bool,
) -> io::Result<()> {
    let query = match Query::parse(&request.query.raw) {
        Ok(query) => query,
        Err(error) => {
            return write_stream_event(
                stream,
                &StreamEvent::Error(error.to_string()),
                Instant::now() + Duration::from_secs(5),
            );
        }
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    let matcher = QueryMatcher::new(query.clone());
    let needs_dates = query.date_modified.is_some()
        || query.date_created.is_some()
        || query.date_accessed.is_some();
    // Sorted streams retain IDs, but still serialize only one batch at a time.
    let sorted = if request.order == StreamOrder::Sorted {
        Some(prepare_query_ids(index, orders, &query, index_size))
    } else {
        None
    };
    // Index-order streams visit only trigram/extension candidates when the
    // query has a selective seed; posting lists are sorted, so order holds.
    // Otherwise every entry is scanned without an ID buffer.
    let ids = sorted.or_else(|| candidate_ids(index, &query));
    let matched = request.order == StreamOrder::Sorted;
    let count = ids.as_ref().map_or(index.count(), Vec::len);
    let mut summary = StreamSummary {
        id: request.query.id,
        total_count: 0,
        total_size: 0,
        returned_count: 0,
    };
    let mut rows = Vec::with_capacity(STREAM_BATCH_SIZE);
    for position in 0..count {
        if position % 4096 == 0 && Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "stream deadline exceeded",
            ));
        }
        let id = ids.as_ref().map_or(position as u32, |ids| ids[position]);
        if !matched && needs_dates && missing_query_dates(&index.entries[id as usize], &query) {
            index.update_metadata_by_id(id);
        }
        if !matched && !matcher.matches(&index.entries[id as usize]) {
            continue;
        }
        if !matched && index_size {
            let entry = &index.entries[id as usize];
            if !entry.is_dir && entry.size == 0 {
                index.update_metadata_by_id(id);
            }
        }
        let entry = &index.entries[id as usize];
        let ordinal = summary.total_count;
        summary.total_count += 1;
        summary.total_size = summary.total_size.saturating_add(entry.size);
        if ordinal < request.query.offset || summary.returned_count >= request.query.max_results {
            continue;
        }
        rows.push(result_row(entry, &query, request.query.highlight));
        summary.returned_count += 1;
        if rows.len() == STREAM_BATCH_SIZE {
            write_stream_event(
                stream,
                &StreamEvent::Rows {
                    id: summary.id,
                    rows: std::mem::take(&mut rows),
                },
                deadline,
            )?;
            rows = Vec::with_capacity(STREAM_BATCH_SIZE);
        }
    }
    if !rows.is_empty() {
        write_stream_event(
            stream,
            &StreamEvent::Rows {
                id: summary.id,
                rows,
            },
            deadline,
        )?;
    }
    write_stream_event(stream, &StreamEvent::Done(summary), deadline)
}

fn handle_stream_request(
    stream: &mut UnixStream,
    request: &StreamQueryRequest,
    config: &Config,
    state: &Arc<Mutex<DaemonState>>,
) -> io::Result<()> {
    let mut st = state.lock().unwrap();
    if st.status != DaemonStatus::Ready {
        return write_stream_event(
            stream,
            &StreamEvent::Error("daemon not ready".into()),
            Instant::now() + Duration::from_secs(5),
        );
    }
    let st = &mut *st;
    stream_results(
        stream,
        request,
        &mut st.index,
        &mut st.orders,
        config.index_size,
    )
}

fn highlight_path(path: &str, query: &Query) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let parent_end = path.len().saturating_sub(name.len());
    let parent = &path[..parent_end];

    let mut ranges = Vec::new();
    for needle in query.terms.iter().flat_map(term_needles) {
        if needle.is_empty() {
            continue;
        }
        let needle_lower = needle.to_lowercase();
        let name_lower = name.to_lowercase();
        for (pos, _) in name_lower.match_indices(&needle_lower) {
            ranges.push((pos, pos + needle.len()));
        }
    }

    let highlighted = apply_highlight_ranges(name, &mut ranges);
    if highlighted != name {
        format!("{}{}", parent, highlighted)
    } else {
        path.to_string()
    }
}

fn apply_highlight_ranges(text: &str, ranges: &mut [(usize, usize)]) -> String {
    if ranges.is_empty() {
        return text.to_string();
    }

    ranges.sort_unstable_by_key(|(start, end)| (*start, *end));
    let mut merged = Vec::with_capacity(ranges.len());
    for &(start, end) in ranges.iter() {
        if let Some((_, last_end)) = merged.last_mut()
            && start <= *last_end
        {
            *last_end = (*last_end).max(end);
            continue;
        }
        merged.push((start, end));
    }

    let mut result = String::new();
    let mut last = 0;
    for (start, end) in merged {
        if start > text.len() || end > text.len() || start >= end {
            continue;
        }
        result.push_str(&text[last..start]);
        result.push('*');
        result.push_str(&text[start..end]);
        result.push('*');
        last = end;
    }
    result.push_str(&text[last..]);
    result
}

fn term_needles(term: &toge_core::query::TextTerm) -> Vec<String> {
    match term {
        toge_core::query::TextTerm::Substring(s) => vec![s.clone()],
        toge_core::query::TextTerm::Wildcard(p) => {
            let clean: String = p.chars().filter(|c| *c != '*' && *c != '?').collect();
            if clean.is_empty() {
                Vec::new()
            } else {
                vec![clean]
            }
        }
        toge_core::query::TextTerm::Regex(p) => {
            let clean: String = p.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.is_empty() {
                Vec::new()
            } else {
                vec![clean]
            }
        }
        toge_core::query::TextTerm::Not(_) => Vec::new(),
        toge_core::query::TextTerm::Or(items) => items.iter().flat_map(term_needles).collect(),
    }
}

fn sort_params(sort: toge_core::query::Sort) -> (SortKey, bool) {
    match sort {
        toge_core::query::Sort::NameAsc => (SortKey::Name, true),
        toge_core::query::Sort::NameDesc => (SortKey::Name, false),
        toge_core::query::Sort::PathAsc => (SortKey::Path, true),
        toge_core::query::Sort::PathDesc => (SortKey::Path, false),
        toge_core::query::Sort::SizeAsc => (SortKey::Size, true),
        toge_core::query::Sort::SizeDesc => (SortKey::Size, false),
        toge_core::query::Sort::ModifiedAsc => (SortKey::Modified, true),
        toge_core::query::Sort::ModifiedDesc => (SortKey::Modified, false),
        toge_core::query::Sort::CreatedAsc => (SortKey::Created, true),
        toge_core::query::Sort::CreatedDesc => (SortKey::Created, false),
        toge_core::query::Sort::AccessedAsc => (SortKey::Accessed, true),
        toge_core::query::Sort::AccessedDesc => (SortKey::Accessed, false),
        toge_core::query::Sort::ExtensionAsc => (SortKey::Extension, true),
        toge_core::query::Sort::ExtensionDesc => (SortKey::Extension, false),
    }
}

fn read_request(stream: &mut UnixStream) -> io::Result<Option<Request>> {
    let mut len_buf = [0u8; 8];
    match stream.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u64::from_le_bytes(len_buf) as usize;
    if len > MAX_IPC_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Request::decode(&buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        .map(Some)
}

fn write_response(stream: &mut UnixStream, resp: &Response) -> io::Result<()> {
    let bytes = resp.encode();
    stream.write_all(&(bytes.len() as u64).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn serve(
    state_dir: PathBuf,
    config_dir: PathBuf,
    config: Config,
    state: Arc<Mutex<DaemonState>>,
    socket_path: PathBuf,
) -> io::Result<()> {
    ensure_private_dir(state_dir.parent().unwrap_or(&state_dir))?;
    if let Some(parent) = socket_path.parent() {
        ensure_private_dir(parent)?;
    }
    let _ = fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;
    set_owner_only(&socket_path)?;

    let mut workers = Vec::new();
    for mut s in listener.incoming().flatten() {
        if let Err(e) = authorize_peer(&s) {
            let _ = write_response(&mut s, &Response::Error(e.to_string()));
            continue;
        }
        let req = match read_request(&mut s) {
            Ok(Some(req)) => req,
            Ok(None) => continue,
            Err(e) => {
                let _ = write_response(&mut s, &Response::Error(e.to_string()));
                continue;
            }
        };

        if matches!(req, Request::Quit) {
            let _ = write_response(&mut s, &Response::Ok);
            break;
        }

        // Requests are read serially above (cheap: local clients write their
        // whole message immediately), but the potentially slow part — running
        // the query against the index — runs on its own thread so one slow
        // request (e.g. a broad substring scan) can't stall every other
        // connection behind it, as a single-threaded accept loop would.
        let state_dir = state_dir.clone();
        let config_dir = config_dir.clone();
        let config = config.clone();
        let state = state.clone();
        // Sessions stay open until the client leaves, so Quit shuts their
        // sockets down to unblock them; other requests finish on their own.
        let session_socket = matches!(
            req,
            Request::OpenSession(_) | Request::OpenSessionPreview(_)
        )
        .then(|| s.try_clone().ok())
        .flatten();
        let handle = thread::spawn(move || match req {
            Request::StreamQuery(request) => {
                // Transport failures close the connection. EOF without Done
                // lets clients distinguish an interrupted stream from success.
                let _ = handle_stream_request(&mut s, &request, &config, &state);
            }
            ref request @ (Request::OpenSession(ref open)
            | Request::OpenSessionPreview(ref open)) => {
                let preview = matches!(request, Request::OpenSessionPreview(_));
                let env = session::SessionEnv::new(&config, &state_dir, &config_dir);
                let _ = session::serve_session_with_preview(&mut s, open, &env, &state, preview);
            }
            req => {
                let resp = handle_request(req, &state_dir, &config, &state);
                let _ = write_response(&mut s, &resp);
            }
        });
        workers.push((handle, session_socket));
        workers.retain(|(handle, _)| !handle.is_finished());
    }
    // Refuse new clients before waiting on the in-flight ones.
    drop(listener);
    let _ = fs::remove_file(&socket_path);
    for (_, socket) in &workers {
        if let Some(socket) = socket {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }
    for (handle, _) in workers {
        let _ = handle.join();
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut socket_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut state_dir: Option<PathBuf> = None;
    let mut clean = false;

    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                usage();
                process::exit(0);
            }
            "-v" | "--version" => {
                version();
                process::exit(0);
            }
            "--socket" => {
                socket_path = Some(PathBuf::from(iter.next().expect("missing socket path")));
            }
            "--config" => {
                config_path = Some(PathBuf::from(iter.next().expect("missing config path")));
            }
            "--state-dir" => {
                state_dir = Some(PathBuf::from(iter.next().expect("missing state dir")));
            }
            "--clean" => clean = true,
            _ => {}
        }
    }

    let state_dir = state_dir.unwrap_or_else(default_state_dir);
    let config_dir = default_config_dir();
    ensure_private_dir(&state_dir).unwrap();
    ensure_private_dir(&config_dir).unwrap();

    if clean {
        let _ = fs::remove_file(state_dir.join("index.bin"));
    }

    let state = Arc::new(Mutex::new(DaemonState {
        index: Index::new(),
        status: DaemonStatus::Starting,
        status_message: "Initializing daemon".to_string(),
        build_duration_ms: 0,
        last_updated_unix: 0,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: OrderCache::default(),
        index_generation: 0,
    }));

    {
        let mut st = state.lock().unwrap();
        st.status = DaemonStatus::LoadingConfig;
        st.status_message = "Loading configuration".to_string();
    }

    let config = config_path
        .as_deref()
        .map(Config::load)
        .unwrap_or_else(|| Config::load(&config_dir.join("config.toml")))
        .unwrap_or_else(|_| Config::default_config());

    {
        let mut st = state.lock().unwrap();
        st.status = DaemonStatus::LoadingIndex;
        st.status_message = "Checking for cached index".to_string();
    }

    let socket = socket_path.unwrap_or_else(|| state_dir.join("toged.sock"));

    let watcher_state = Arc::clone(&state);
    let watcher_state_dir = state_dir.clone();
    let watcher_config_dir = config_dir.clone();
    let watcher_config = config.clone();
    let watcher_failure = start_watcher(
        watcher_state,
        watcher_state_dir,
        watcher_config_dir,
        watcher_config,
    );

    let index_state_dir = state_dir.clone();
    let index_config = config.clone();
    let index_state = Arc::clone(&state);
    let spawn_result = thread::Builder::new().spawn({
        let watcher_failure = watcher_failure.clone();
        move || {
            start_index(
                &index_state_dir,
                &index_config,
                &index_state,
                watcher_failure.as_deref(),
            )
        }
    });

    if let Err(err) = spawn_result {
        eprintln!("background indexing unavailable: {}", err);
        start_index(&state_dir, &config, &state, watcher_failure.as_deref());
    }

    serve(state_dir, config_dir, config, state, socket).unwrap();
}

/// Watcher events applied per index-lock acquisition.
const WATCH_APPLY_BATCH: usize = 256;

/// What the watcher indexes: the configured roots and exclusions, minus the
/// daemon's own files. Directories are resolved once up front: fanotify reports
/// resolved paths, and resolving every event's path would cost a syscall per
/// component while the kernel queue fills.
struct WatchScope {
    /// Roots as configured and as resolved.
    roots: Vec<PathBuf>,
    excludes: Excludes,
    /// The state and config directories, as given and as resolved.
    own_dirs: Vec<PathBuf>,
}

impl WatchScope {
    fn new(roots: &[PathBuf], excludes: Excludes, state_dir: &Path, config_dir: &Path) -> Self {
        let with_resolved = |dirs: &[&Path]| {
            let mut all = Vec::new();
            for dir in dirs {
                all.push(dir.to_path_buf());
                if let Ok(resolved) = fs::canonicalize(dir)
                    && resolved != *dir
                {
                    all.push(resolved);
                }
            }
            all
        };
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        Self {
            roots: with_resolved(&roots),
            excludes,
            own_dirs: with_resolved(&[state_dir, config_dir]),
        }
    }

    fn covers(&self, path: &str, is_dir: bool) -> bool {
        let path = Path::new(path);
        self.roots.iter().any(|root| path.starts_with(root))
            && !self.own_dirs.iter().any(|dir| path.starts_with(dir))
            && !is_hidden_dir_path(path, is_dir)
            && !excluded_under_roots(path, &self.roots, &self.excludes)
    }
}

/// Overflow reconciles run off the watcher thread, which must keep draining
/// the kernel queue; an overflow during a run schedules one more run.
#[derive(Default)]
struct Resync {
    running: bool,
    again: bool,
}

fn request_resync(
    resync: &Arc<Mutex<Resync>>,
    state: &Arc<Mutex<DaemonState>>,
    config: &Config,
    state_dir: &Path,
) {
    {
        let mut pending = resync.lock().unwrap();
        if pending.running {
            pending.again = true;
            return;
        }
        pending.running = true;
    }
    let (resync, state, config, state_dir) = (
        Arc::clone(resync),
        Arc::clone(state),
        config.clone(),
        state_dir.to_path_buf(),
    );
    thread::spawn(move || {
        loop {
            resync_after_overflow(&state, &config, &state_dir);
            let mut pending = resync.lock().unwrap();
            if !pending.again {
                pending.running = false;
                break;
            }
            pending.again = false;
        }
        let mut st = state.lock().unwrap();
        st.watcher.is_healthy = st.watcher.watch_failure_count == 0;
    });
}

/// Lost events can only be recovered by walking. The current index keeps
/// serving queries meanwhile, as at startup.
fn resync_after_overflow(state: &Arc<Mutex<DaemonState>>, config: &Config, state_dir: &Path) {
    let generation = {
        let mut st = state.lock().unwrap();
        st.status_message = "Reconciling after watcher overflow".to_string();
        st.index_generation
    };
    let start = Instant::now();
    if reconcile_served_index(config, state, generation) {
        let mut st = state.lock().unwrap();
        let _ = save_index(&st.index, state_dir);
        st.build_duration_ms = start.elapsed().as_millis() as u64;
        st.last_updated_unix = current_unix_time();
        st.status_message = format!("Reindexed {} entries", st.index.count());
        append_watcher_log(&mut st, "reindex completed after watcher overflow");
    }
}

/// A watcher event checked against the [`WatchScope`], with its disk reads
/// already done, so applying it under the index lock stays cheap.
#[derive(Debug)]
enum IndexChange {
    Create {
        path: String,
        is_dir: bool,
        metadata: (u64, i64, i64, i64),
    },
    Delete {
        path: String,
    },
    Modify {
        path: String,
        metadata: (u64, i64, i64, i64),
    },
    Move {
        from: String,
        to: String,
    },
    Overflow,
}

/// Resolve raw events without holding the index lock. Events outside the
/// scope (e.g. build output in an excluded folder) are dropped here.
fn resolve_events(
    events: Vec<WatchEvent>,
    scope: &WatchScope,
    watcher: &mut impl FsWatcher,
) -> Vec<IndexChange> {
    let mut changes = Vec::new();
    for event in events {
        match event {
            WatchEvent::Create { path, is_dir } => {
                if scope.covers(&path, is_dir) {
                    let metadata = metadata_snapshot(&path);
                    changes.push(IndexChange::Create {
                        path,
                        is_dir,
                        metadata,
                    });
                }
            }
            WatchEvent::Delete { path } => {
                if scope.covers(&path, false) {
                    let _ = watcher.unwatch(Path::new(&path));
                    changes.push(IndexChange::Delete { path });
                }
            }
            WatchEvent::Modify { path } => {
                if scope.covers(&path, false) {
                    let metadata = metadata_snapshot(&path);
                    changes.push(IndexChange::Modify { path, metadata });
                }
            }
            WatchEvent::Move { from, to } => {
                let from_covered = scope.covers(&from, false);
                let to_is_dir = Path::new(&to).is_dir();
                let to_covered = scope.covers(&to, to_is_dir);
                if from_covered {
                    let _ = watcher.unwatch(Path::new(&from));
                }
                match (from_covered, to_covered) {
                    (false, false) => {}
                    (true, false) => changes.push(IndexChange::Delete { path: from }),
                    (false, true) => {
                        let metadata = metadata_snapshot(&to);
                        changes.push(IndexChange::Create {
                            path: to,
                            is_dir: to_is_dir,
                            metadata,
                        });
                    }
                    (true, true) => changes.push(IndexChange::Move { from, to }),
                }
            }
            WatchEvent::Overflow { .. } => changes.push(IndexChange::Overflow),
        }
    }
    changes
}

/// Apply one resolved change. Returns true when events were lost and the
/// index must be reconciled with the disk.
fn apply_change(st: &mut DaemonState, change: &IndexChange, config: &Config) -> bool {
    match change {
        IndexChange::Create {
            path,
            is_dir,
            metadata,
        } => {
            append_watcher_log(
                st,
                format!("create {}{}", path, if *is_dir { " (dir)" } else { "" }),
            );
            index_created_path_with(st, path, *is_dir, *metadata, config);
        }
        IndexChange::Delete { path } => {
            append_watcher_log(st, format!("delete {}", path));
            remove_deleted_path(&mut st.index, path);
        }
        IndexChange::Modify { path, metadata } => {
            append_watcher_log(st, format!("modify {}", path));
            // Refresh only entries already indexed, with the same type.
            if let Some(id) = st.index.id_by_path(path) {
                let entry = &st.index.entries[id as usize];
                if entry.path == *path {
                    let is_dir = entry.is_dir;
                    let (size, modified, created, accessed) = *metadata;
                    st.index
                        .insert_with_metadata(path, is_dir, size, modified, created, accessed);
                }
            }
        }
        IndexChange::Move { from, to } => {
            append_watcher_log(st, format!("move {} -> {}", from, to));
            remove_deleted_path(&mut st.index, from);
            index_created_path(st, to, Path::new(to).is_dir(), config);
        }
        IndexChange::Overflow => {
            eprintln!("fanotify queue overflow — some events may have been lost");
            st.watcher.watch_overflow_count += 1;
            st.watcher.is_healthy = false;
            append_watcher_log(
                st,
                "overflow: fanotify queue overflow — some events may have been lost",
            );
            return true;
        }
    }
    false
}

fn start_watcher(
    state: Arc<Mutex<DaemonState>>,
    state_dir: PathBuf,
    config_dir: PathBuf,
    config: Config,
) -> Option<String> {
    let spawn_result = thread::Builder::new()
        .name("fanotify-watcher".into())
        .spawn(move || {
            loop {
                {
                    let st = state.lock().unwrap();
                    if st.status == DaemonStatus::StartingWatcher {
                        break;
                    }
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }

            let mut watcher = match FanotifyWatcher::new() {
                Ok(watcher) => watcher,
                Err(e) => {
                    eprintln!("Failed to create fanotify watcher: {}", e);
                    mark_watcher_unavailable(&state, &format!("initialization error: {e}"));
                    return;
                }
            };

            let dirs = discover_roots(&config);
            let scope = WatchScope::new(&dirs, index_excludes(&config), &state_dir, &config_dir);
            let resync = Arc::new(Mutex::new(Resync::default()));
            let watcher_status = install_watches(&mut watcher, &dirs);
            {
                let mut st = state.lock().unwrap();
                st.watcher = watcher_status;
                if st.watcher.is_healthy {
                    st.status = DaemonStatus::Ready;
                    st.status_message = format!(
                        "Indexed {} entries in {}ms",
                        st.index.count(),
                        st.build_duration_ms
                    );
                    append_watcher_log(&mut st, "using fanotify filesystem watcher");
                } else {
                    st.status = DaemonStatus::Ready;
                    st.status_message = WATCHER_REMEDIATION.to_string();
                    append_watcher_log(&mut st, WATCHER_REMEDIATION);
                }
            }

            loop {
                let events = match watcher.poll_events() {
                    Ok(ev) => ev,
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            thread::sleep(std::time::Duration::from_millis(100));
                            continue;
                        }
                        eprintln!("fanotify poll error: {}", e);
                        thread::sleep(std::time::Duration::from_secs(1));
                        continue;
                    }
                };

                if events.is_empty() {
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }

                let changes = resolve_events(events, &scope, &mut watcher);
                let mut overflowed = false;
                // Release the lock between chunks so queries are not stuck
                // behind a burst of filesystem activity.
                for chunk in changes.chunks(WATCH_APPLY_BATCH) {
                    let mut st = state.lock().unwrap();
                    for change in chunk {
                        overflowed |= apply_change(&mut st, change, &config);
                    }
                }
                if overflowed {
                    request_resync(&resync, &state, &config, &state_dir);
                }
            }
        });

    // Reported to start_index, which marks the daemon Ready once the index is
    // served; marking it here would be undone by the StartingWatcher hand-off.
    spawn_result.err().map(|error| {
        eprintln!("Failed to spawn fanotify watcher thread: {error}");
        format!("thread spawn error: {error}")
    })
}

#[cfg(test)]
fn is_own_path(path: &str, state_dir: &Path, config_dir: &Path) -> bool {
    is_ignored_path(path, state_dir, config_dir, false)
}

fn canonical_starts_with(path: &Path, root: &Path) -> bool {
    match (fs::canonicalize(path), fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
