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
use std::time::{Instant, SystemTime};
use toge_core::config::Config;
use toge_core::index::Index;
use toge_core::ipc::{
    DaemonStatus, MAX_IPC_MESSAGE_SIZE, QueryRequest, Request, Response, ResultRow,
    ResultsResponse, StatusResponse,
};
use toge_core::matcher::match_query;
use toge_core::query::Query;
use toge_core::sort::{SortKey, sort_ids};
use toge_core::sys::FsWatcher;
use toge_core::sys::{FanotifyWatcher, WatchEvent};
use toge_core::walker::{Excludes, has_hidden_ancestor_dir, walk};

struct DaemonState {
    index: Index,
    status: DaemonStatus,
    status_message: String,
    build_duration_ms: u64,
    last_updated_unix: i64,
    watcher: WatcherStatus,
    watcher_log: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct WatcherStatus {
    is_healthy: bool,
    watched_dir_count: usize,
    watch_failure_count: usize,
    watch_overflow_count: u64,
}

const WATCHER_LOG_LIMIT: usize = 50;

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

fn build_index(state_dir: &Path, config: &Config, state: &Arc<Mutex<DaemonState>>) -> (Index, u64) {
    let index_path = state_dir.join("index.bin");
    if let Ok(idx) = Index::load(&index_path) {
        {
            let mut st = state.lock().unwrap();
            st.status = DaemonStatus::LoadingIndex;
            st.status_message = format!("Loaded {} entries from cache", idx.count());
        }
        return (idx, 0);
    }

    let start = Instant::now();
    let mut index = Index::new();
    let excludes = Excludes {
        skip_hidden: config.exclude_hidden,
        skip_system_paths: true,
        patterns: config.exclude_patterns.clone(),
        folders: config.exclude_folders.clone(),
        paths: Vec::new(),
        include_only: config.include_only.clone(),
    };

    let roots = discover_roots(config);
    let fetch_metadata = config.index_size
        || config.index_date_modified
        || config.index_date_created
        || config.index_date_accessed;
    let total_roots = roots.len();
    for (i, root) in roots.iter().enumerate() {
        {
            let mut st = state.lock().unwrap();
            st.status = DaemonStatus::Indexing;
            st.status_message = format!("Indexing {}/{}: {}", i + 1, total_roots, root.display());
        }
        walk(root, &mut index, &excludes, fetch_metadata);
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    (index, duration_ms)
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
            Err(e)
                if e.kind() == io::ErrorKind::PermissionDenied
                    || e.kind() == io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(28) =>
            {
                watcher_status.watch_failure_count += 1;
            }
            Err(_) => {
                watcher_status.watch_failure_count += 1;
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
            let (new_index, duration) = build_index(state_dir, config, state);
            if let Err(e) = save_index(&new_index, state_dir) {
                return Response::Error(e.to_string());
            }
            let mut st = state.lock().unwrap();
            st.index = new_index;
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
            handle_query(&mut st.index, &q, config.index_size)
        }
        Request::Quit => unreachable!(),
    }
}

fn handle_query(index: &mut Index, q: &QueryRequest, index_size: bool) -> Response {
    let query = match Query::parse(&q.raw) {
        Ok(query) => query,
        Err(e) => return Response::Error(e.to_string()),
    };

    let mut ids = match_query(index, &query);
    let (sort_key, ascending) = sort_params(query.sort);
    sort_ids(index, &mut ids, sort_key, ascending);

    if index_size {
        for id in &ids {
            let entry = &index.entries[*id as usize];
            if entry.is_dir || entry.size != 0 {
                continue;
            }
            let path = entry.path.clone();
            index.update_metadata(&path);
        }
    }

    let total = ids.len();
    let total_size: u64 = ids.iter().map(|id| index.entries[*id as usize].size).sum();

    let offset = q.offset.min(total);
    let end = (offset + q.max_results).min(total);
    let page = &ids[offset..end];

    let rows: Vec<ResultRow> = page
        .iter()
        .map(|id| {
            let entry = &index.entries[*id as usize];
            let path = entry.path.clone();
            let display_path = if q.highlight && !query.terms.is_empty() {
                highlight_path(&path, &query)
            } else {
                path.clone()
            };
            let name = entry.name().to_string();
            let parent_end = entry.name_off as usize;
            let parent = if parent_end > 0 {
                path[..parent_end.saturating_sub(1)].to_string()
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
        })
        .collect();

    Response::Results(ResultsResponse {
        id: q.id,
        total_count: total,
        total_size,
        rows,
    })
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

        let resp = handle_request(req, &state_dir, &config, &state);

        let _ = write_response(&mut s, &resp);
    }

    let _ = fs::remove_file(&socket_path);
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

    let index_state_dir = state_dir.clone();
    let index_config = config.clone();
    let index_state = Arc::clone(&state);
    let spawn_result = thread::Builder::new().spawn(move || {
        let (index, duration) = build_index(&index_state_dir, &index_config, &index_state);
        let _ = save_index(&index, &index_state_dir);
        let mut st = index_state.lock().unwrap();
        st.index = index;
        st.build_duration_ms = duration;
        st.last_updated_unix = current_unix_time();
        st.status = DaemonStatus::StartingWatcher;
        st.status_message = "Setting up file watcher".to_string();
        st.status = DaemonStatus::Ready;
        st.status_message = format!("Indexed {} entries in {}ms", st.index.count(), duration);
    });

    if let Err(err) = spawn_result {
        eprintln!("background indexing unavailable: {}", err);
        let mut st = state.lock().unwrap();
        st.status = DaemonStatus::Indexing;
        st.status_message = "Scanning filesystem (foreground)".to_string();
        drop(st);
        let (index, duration) = build_index(&state_dir, &config, &state);
        let _ = save_index(&index, &state_dir);
        let mut st = state.lock().unwrap();
        st.index = index;
        st.build_duration_ms = duration;
        st.last_updated_unix = current_unix_time();
        st.status = DaemonStatus::StartingWatcher;
        st.status_message = "Setting up file watcher".to_string();
        st.status = DaemonStatus::Ready;
        st.status_message = format!("Indexed {} entries in {}ms", st.index.count(), duration);
    }

    let watcher_state = Arc::clone(&state);
    let watcher_state_dir = state_dir.clone();
    let watcher_config_dir = config_dir.clone();
    let watcher_config = config.clone();
    start_watcher(
        watcher_state,
        watcher_state_dir,
        watcher_config_dir,
        watcher_config,
    );

    serve(state_dir, config, state, socket).unwrap();
}

fn start_watcher(
    state: Arc<Mutex<DaemonState>>,
    state_dir: PathBuf,
    config_dir: PathBuf,
    config: Config,
) {
    thread::Builder::new()
        .name("fanotify-watcher".into())
        .spawn(move || {
            loop {
                {
                    let st = state.lock().unwrap();
                    if st.status == DaemonStatus::Ready {
                        break;
                    }
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }

            let mut watcher = match FanotifyWatcher::new() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create fanotify watcher: {}", e);
                    return;
                }
            };

            let dirs = discover_roots(&config);

            let watcher_status = install_watches(&mut watcher, &dirs);
            {
                let mut st = state.lock().unwrap();
                st.watcher = watcher_status;
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

                let mut needs_reindex = false;
                {
                    let mut st = state.lock().unwrap();
                    for event in events {
                        match &event {
                            WatchEvent::Create { path, is_dir } => {
                                if !is_within_roots(path, &dirs) {
                                    continue;
                                }
                                if is_ignored_path(path, &state_dir, &config_dir, *is_dir) {
                                    continue;
                                }
                                append_watcher_log(
                                    &mut st,
                                    format!("create {}{}", path, if *is_dir { " (dir)" } else { "" }),
                                );
                                let (size, modified, created, accessed) = metadata_snapshot(path);
                                st.index.insert_with_metadata(
                                    path,
                                    *is_dir,
                                    size,
                                    modified,
                                    created,
                                    accessed,
                                );
                            }
                            WatchEvent::Delete { path } => {
                                if !is_within_roots(path, &dirs) {
                                    continue;
                                }
                                if is_ignored_path(path, &state_dir, &config_dir, false) {
                                    continue;
                                }
                                append_watcher_log(&mut st, format!("delete {}", path));
                                st.index.remove(path);
                            }
                            WatchEvent::Modify { path } => {
                                if !is_within_roots(path, &dirs) {
                                    continue;
                                }
                                if is_ignored_path(path, &state_dir, &config_dir, false) {
                                    continue;
                                }
                                append_watcher_log(&mut st, format!("modify {}", path));
                                st.index.update_metadata(path);
                            }
                            WatchEvent::Move { from, to } => {
                                let from_in_roots = is_within_roots(from, &dirs);
                                let to_in_roots = is_within_roots(to, &dirs);
                                if !from_in_roots && !to_in_roots {
                                    continue;
                                }
                                let from_ignored =
                                    is_ignored_path(from, &state_dir, &config_dir, false);
                                let to_ignored =
                                    is_ignored_path(
                                        to,
                                        &state_dir,
                                        &config_dir,
                                        std::path::Path::new(to).is_dir(),
                                    );
                                append_watcher_log(&mut st, format!("move {} -> {}", from, to));
                                if from_in_roots && !from_ignored {
                                    st.index.remove(from);
                                }
                                if to_in_roots && !to_ignored {
                                    let is_dir = std::path::Path::new(to).is_dir();
                                    let (size, modified, created, accessed) = metadata_snapshot(to);
                                    st.index.insert_with_metadata(
                                        to,
                                        is_dir,
                                        size,
                                        modified,
                                        created,
                                        accessed,
                                    );
                                }
                            }
                            WatchEvent::Overflow { .. } => {
                                eprintln!(
                                    "fanotify queue overflow — some events may have been lost"
                                );
                                st.watcher.watch_overflow_count += 1;
                                st.watcher.is_healthy = false;
                                append_watcher_log(
                                    &mut st,
                                    "overflow: fanotify queue overflow — some events may have been lost",
                                );
                                needs_reindex = true;
                            }
                        }
                    }
                    st.watcher.is_healthy = st.watcher.watch_failure_count == 0 && !needs_reindex;
                }

                if needs_reindex {
                    let _ = fs::remove_file(state_dir.join("index.bin"));
                    let (new_index, duration) = build_index(&state_dir, &config, &state);
                    let _ = save_index(&new_index, &state_dir);
                    let dirs = discover_roots(&config);
                    let watcher_status = install_watches(&mut watcher, &dirs);
                    {
                        let mut st = state.lock().unwrap();
                        st.index = new_index;
                        st.build_duration_ms = duration;
                        st.status = DaemonStatus::Ready;
                        st.status_message = format!("Reindexed {} entries", st.index.count());
                        st.watcher = watcher_status;
                        append_watcher_log(&mut st, "reindex completed after watcher overflow");
                    }
                }
            }
        })
        .ok();
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
