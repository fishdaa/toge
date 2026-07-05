//! needled — background indexing daemon.

use needle_core::config::Config;
use needle_core::index::Index;
use needle_core::ipc::{QueryRequest, Request, Response, ResultsResponse, StatusResponse};
use needle_core::matcher::match_query;
use needle_core::query::Query;
use needle_core::sort::{sort_ids, SortKey};
use needle_core::sys::FsWatcher;
use needle_core::sys::{InotifyWatcher, WatchEvent};
use needle_core::walker::{walk, Excludes};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, SystemTime};

struct DaemonState {
    index: Index,
    is_ready: bool,
    build_duration_ms: u64,
    watcher: WatcherStatus,
}

#[derive(Clone, Debug, Default)]
struct WatcherStatus {
    is_healthy: bool,
    watched_dir_count: usize,
    watch_failure_count: usize,
    watch_overflow_count: u64,
}

fn usage() {
    println!("needled [options]");
    println!("Options:");
    println!("  --socket <path>     Unix domain socket path");
    println!("  --config <path>     Config file path");
    println!("  --state-dir <path>  State directory (for index.bin)");
    println!("  --clean             Delete old index before starting");
    println!("  -h, --help          Show this help");
    println!("  -v, --version       Show version");
}

fn version() {
    println!("needled 0.1.1");
}

fn default_state_dir() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME").expect("HOME not set");
            PathBuf::from(home).join(".local/state")
        })
        .join("needle")
}

fn default_config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME").expect("HOME not set");
            PathBuf::from(home).join(".config")
        })
        .join("needle")
}

fn discover_roots(config: &Config) -> Vec<PathBuf> {
    if !config.roots.is_empty() {
        return config.roots.clone();
    }
    let mut roots = Vec::new();
    if let Ok(content) = fs::read_to_string("/proc/self/mountinfo") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 9 {
                continue;
            }
            let mount_point = parts[4];
            let fs_type = parts[parts.len() - 3];
            if config.exclude_fstypes.iter().any(|t| t == fs_type) {
                continue;
            }
            if matches!(fs_type, "ext4" | "btrfs" | "xfs" | "zfs" | "ext3" | "ext2") {
                roots.push(PathBuf::from(mount_point));
            }
        }
    }
    if roots.is_empty() {
        roots.push(PathBuf::from("/"));
    }
    roots
}

fn build_index(state_dir: &Path, config: &Config) -> (Index, u64) {
    let index_path = state_dir.join("index.bin");
    if let Ok(idx) = Index::load(&index_path) {
        eprintln!(
            "Loaded {} entries from {}",
            idx.count(),
            index_path.display()
        );
        return (idx, 0);
    }

    let start = Instant::now();
    let mut index = Index::new();
    let excludes = Excludes {
        skip_hidden: config.exclude_hidden,
        skip_system_paths: true,
        patterns: config.exclude_patterns.clone(),
        folders: config.exclude_folders.clone(),
        include_only: config.include_only.clone(),
    };

    let roots = discover_roots(config);
    let fetch_metadata = config.index_size
        || config.index_date_modified
        || config.index_date_created
        || config.index_date_accessed;
    eprintln!("Indexing roots: {:?}", roots);
    for root in roots {
        let count_before = index.count();
        walk(&root, &mut index, &excludes, fetch_metadata);
        eprintln!(
            "Indexed {} entries from {}",
            index.count() - count_before,
            root.display()
        );
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    eprintln!("Indexed {} entries in {} ms", index.count(), duration_ms);
    (index, duration_ms)
}

fn save_index(index: &Index, state_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(state_dir)?;
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

fn status_response(st: &DaemonState) -> StatusResponse {
    StatusResponse {
        indexed_count: st.index.count(),
        is_ready: st.is_ready,
        watcher_healthy: st.watcher.is_healthy,
        watched_dir_count: st.watcher.watched_dir_count,
        watch_failure_count: st.watcher.watch_failure_count,
        watch_overflow_count: st.watcher.watch_overflow_count,
        last_updated_unix: current_unix_time(),
        build_duration_ms: st.build_duration_ms,
    }
}

fn watched_dirs(index: &Index) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = index
        .entries
        .iter()
        .map(|e| {
            let parent_end = e.name_off as usize;
            if parent_end > 0 {
                PathBuf::from(&e.path[..parent_end.saturating_sub(1)])
            } else {
                PathBuf::from("/")
            }
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn install_watches(
    watcher: &mut InotifyWatcher,
    dirs: &[PathBuf],
    watcher_status: &mut WatcherStatus,
) {
    eprintln!("Starting inotify watcher on {} directories...", dirs.len());

    watcher_status.watched_dir_count = 0;
    watcher_status.watch_failure_count = 0;

    for dir in dirs {
        match watcher.watch(dir) {
            Ok(()) => watcher_status.watched_dir_count += 1,
            Err(e)
                if e.kind() == io::ErrorKind::PermissionDenied
                    || e.kind() == io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(28) =>
            {
                watcher_status.watch_failure_count += 1;
            }
            Err(e) => {
                watcher_status.watch_failure_count += 1;
                eprintln!("Failed to watch {}: {}", dir.display(), e);
            }
        }
    }

    watcher_status.is_healthy = watcher_status.watch_failure_count == 0;
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
            let (new_index, duration) = build_index(state_dir, config);
            if let Err(e) = save_index(&new_index, state_dir) {
                return Response::Error(e.to_string());
            }
            let mut st = state.lock().unwrap();
            st.index = new_index;
            st.build_duration_ms = duration;
            st.is_ready = true;
            Response::Ok
        }
        Request::Status => {
            let st = state.lock().unwrap();
            Response::Status(status_response(&st))
        }
        Request::Query(q) => {
            let st = state.lock().unwrap();
            if !st.is_ready {
                return Response::Error("daemon not ready".into());
            }
            handle_query(&st.index, &q)
        }
        Request::Quit => unreachable!(),
    }
}

fn handle_query(index: &Index, q: &QueryRequest) -> Response {
    let query = match Query::parse(&q.raw) {
        Ok(query) => query,
        Err(e) => return Response::Error(e.to_string()),
    };

    let mut ids = match_query(index, &query);
    let (sort_key, ascending) = sort_params(query.sort);
    sort_ids(index, &mut ids, sort_key, ascending);

    let total = ids.len();
    let total_size: u64 = ids.iter().map(|id| index.entries[*id as usize].size).sum();

    let offset = q.offset.min(total);
    let end = (offset + q.max_results).min(total);
    let page = &ids[offset..end];

    let paths: Vec<String> = page
        .iter()
        .map(|id| {
            let path = index.get_path(*id).unwrap_or("").to_string();
            if q.highlight && !query.terms.is_empty() {
                highlight_path(&path, &query)
            } else {
                path
            }
        })
        .collect();

    Response::Results(ResultsResponse {
        id: q.id,
        total_count: total,
        total_size,
        paths,
    })
}

fn highlight_path(path: &str, query: &Query) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let parent_end = path.len().saturating_sub(name.len());
    let parent = &path[..parent_end];

    let mut highlighted = name.to_string();
    for needle in query.terms.iter().flat_map(term_needles) {
        let needle_lower = needle.to_lowercase();
        let name_lower = name.to_lowercase();
        let mut result = String::new();
        let mut last = 0;
        for (pos, _) in name_lower.match_indices(&needle_lower) {
            result.push_str(&name[last..pos]);
            result.push('*');
            result.push_str(&name[pos..pos + needle.len()]);
            result.push('*');
            last = pos + needle.len();
        }
        if last > 0 {
            result.push_str(&name[last..]);
            highlighted = result;
        }
    }

    if highlighted != name {
        format!("{}{}", parent, highlighted)
    } else {
        path.to_string()
    }
}

fn term_needles(term: &needle_core::query::TextTerm) -> Vec<String> {
    match term {
        needle_core::query::TextTerm::Substring(s) => vec![s.clone()],
        needle_core::query::TextTerm::Wildcard(p) => {
            let clean: String = p.chars().filter(|c| *c != '*' && *c != '?').collect();
            if clean.is_empty() {
                Vec::new()
            } else {
                vec![clean]
            }
        }
        needle_core::query::TextTerm::Regex(p) => {
            let clean: String = p.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.is_empty() {
                Vec::new()
            } else {
                vec![clean]
            }
        }
        needle_core::query::TextTerm::Not(_) => Vec::new(),
        needle_core::query::TextTerm::Or(items) => items.iter().flat_map(term_needles).collect(),
    }
}

fn sort_params(sort: needle_core::query::Sort) -> (SortKey, bool) {
    match sort {
        needle_core::query::Sort::NameAsc => (SortKey::Name, true),
        needle_core::query::Sort::NameDesc => (SortKey::Name, false),
        needle_core::query::Sort::PathAsc => (SortKey::Path, true),
        needle_core::query::Sort::PathDesc => (SortKey::Path, false),
        needle_core::query::Sort::SizeAsc => (SortKey::Size, true),
        needle_core::query::Sort::SizeDesc => (SortKey::Size, false),
        needle_core::query::Sort::ModifiedAsc => (SortKey::Modified, true),
        needle_core::query::Sort::ModifiedDesc => (SortKey::Modified, false),
        needle_core::query::Sort::CreatedAsc => (SortKey::Created, true),
        needle_core::query::Sort::CreatedDesc => (SortKey::Created, false),
        needle_core::query::Sort::AccessedAsc => (SortKey::Accessed, true),
        needle_core::query::Sort::AccessedDesc => (SortKey::Accessed, false),
        needle_core::query::Sort::ExtensionAsc => (SortKey::Extension, true),
        needle_core::query::Sort::ExtensionDesc => (SortKey::Extension, false),
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
    if len > 10 * 1024 * 1024 {
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
    fs::create_dir_all(state_dir.parent().unwrap_or(&state_dir))?;
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;
    eprintln!("Listening on {}", socket_path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
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
            Err(e) => eprintln!("Connection error: {}", e),
        }
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
            _ => eprintln!("Unknown argument: {}", arg),
        }
    }

    let state_dir = state_dir.unwrap_or_else(default_state_dir);
    let config_dir = default_config_dir();
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&config_dir).unwrap();

    if clean {
        let _ = fs::remove_file(state_dir.join("index.bin"));
    }

    let config = config_path
        .as_deref()
        .map(Config::load)
        .unwrap_or_else(|| Config::load(&config_dir.join("config.toml")))
        .unwrap_or_else(|_| Config::default_config());

    let socket = socket_path.unwrap_or_else(|| state_dir.join("needled.sock"));

    let state = Arc::new(Mutex::new(DaemonState {
        index: Index::new(),
        is_ready: false,
        build_duration_ms: 0,
        watcher: WatcherStatus::default(),
    }));

    let index_state_dir = state_dir.clone();
    let index_config = config.clone();
    let index_state = Arc::clone(&state);
    let spawn_result = thread::Builder::new().spawn(move || {
        let (index, duration) = build_index(&index_state_dir, &index_config);
        let _ = save_index(&index, &index_state_dir);
        let mut st = index_state.lock().unwrap();
        st.index = index;
        st.build_duration_ms = duration;
        st.is_ready = true;
    });

    if let Err(err) = spawn_result {
        eprintln!("background indexing unavailable: {}", err);
        let (index, duration) = build_index(&state_dir, &config);
        let _ = save_index(&index, &state_dir);
        let mut st = state.lock().unwrap();
        st.index = index;
        st.build_duration_ms = duration;
        st.is_ready = true;
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
        .name("inotify-watcher".into())
        .spawn(move || {
            loop {
                {
                    let st = state.lock().unwrap();
                    if st.is_ready {
                        break;
                    }
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }

            let mut watcher = match InotifyWatcher::new() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create inotify watcher: {}", e);
                    return;
                }
            };

            let dirs = {
                let st = state.lock().unwrap();
                watched_dirs(&st.index)
            };

            {
                let mut st = state.lock().unwrap();
                install_watches(&mut watcher, &dirs, &mut st.watcher);
            }

            loop {
                let events = match watcher.poll_events() {
                    Ok(ev) => ev,
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            thread::sleep(std::time::Duration::from_millis(100));
                            continue;
                        }
                        eprintln!("inotify poll error: {}", e);
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
                                if is_own_path(path, &state_dir, &config_dir) {
                                    continue;
                                }
                                if *is_dir {
                                    match watcher.watch(&PathBuf::from(path)) {
                                        Ok(()) => st.watcher.watched_dir_count += 1,
                                        Err(e)
                                            if e.kind() == io::ErrorKind::PermissionDenied
                                                || e.kind() == io::ErrorKind::NotFound
                                                || e.raw_os_error() == Some(28) =>
                                        {
                                            st.watcher.watch_failure_count += 1;
                                            st.watcher.is_healthy = false;
                                        }
                                        Err(e) => {
                                            st.watcher.watch_failure_count += 1;
                                            st.watcher.is_healthy = false;
                                            eprintln!("Failed to watch {}: {}", path, e);
                                        }
                                    }
                                }
                                let md = std::fs::metadata(path).ok();
                                let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
                                let now = current_unix_time();
                                st.index
                                    .insert_with_metadata(path, *is_dir, size, now, now, now);
                            }
                            WatchEvent::Delete { path } => {
                                if is_own_path(path, &state_dir, &config_dir) {
                                    continue;
                                }
                                st.index.remove(path);
                            }
                            WatchEvent::Modify { path } => {
                                if is_own_path(path, &state_dir, &config_dir) {
                                    continue;
                                }
                                st.index.update_metadata(path);
                            }
                            WatchEvent::Move { from, to } => {
                                if is_own_path(to, &state_dir, &config_dir)
                                    || is_own_path(from, &state_dir, &config_dir)
                                {
                                    continue;
                                }
                                st.index.remove(from);
                                let is_dir = std::path::Path::new(to).is_dir();
                                let size = std::fs::metadata(to).ok().map(|m| m.len()).unwrap_or(0);
                                let now = current_unix_time();
                                st.index
                                    .insert_with_metadata(to, is_dir, size, now, now, now);
                                st.index.update_metadata(to);
                            }
                            WatchEvent::Overflow { .. } => {
                                eprintln!(
                                    "inotify queue overflow — some events may have been lost"
                                );
                                st.watcher.watch_overflow_count += 1;
                                st.watcher.is_healthy = false;
                                needs_reindex = true;
                            }
                        }
                    }
                    st.watcher.is_healthy = st.watcher.watch_failure_count == 0 && !needs_reindex;
                }

                if needs_reindex {
                    let _ = fs::remove_file(state_dir.join("index.bin"));
                    let (new_index, duration) = build_index(&state_dir, &config);
                    let _ = save_index(&new_index, &state_dir);
                    let dirs = watched_dirs(&new_index);
                    {
                        let mut st = state.lock().unwrap();
                        st.index = new_index;
                        st.build_duration_ms = duration;
                        st.is_ready = true;
                        install_watches(&mut watcher, &dirs, &mut st.watcher);
                    }
                }
            }
        })
        .ok();
}

fn is_own_path(path: &str, state_dir: &Path, config_dir: &Path) -> bool {
    path.starts_with(state_dir.to_str().unwrap_or(""))
        || path.starts_with(config_dir.to_str().unwrap_or(""))
}

#[cfg(test)]
mod tests;
