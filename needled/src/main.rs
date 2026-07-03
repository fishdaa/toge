//! needled — background indexing daemon.

use needle_core::config::Config;
use needle_core::index::Index;
use needle_core::ipc::{QueryRequest, Request, Response, ResultsResponse, StatusResponse};
use needle_core::matcher::match_query;
use needle_core::query::Query;
use needle_core::sort::{sort_ids, SortKey};
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
    println!("needled 0.1.0");
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
    eprintln!("Indexing roots: {:?}", roots);
    for root in roots {
        let count_before = index.count();
        walk(&root, &mut index, &excludes);
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

fn handle_query(index: &Index, q: &QueryRequest) -> Response {
    let query = match Query::parse(&q.raw) {
        Ok(query) => query,
        Err(e) => return Response::Error(e.to_string()),
    };

    let mut ids = match_query(index, &query);
    let (sort_key, ascending) = sort_params(query.sort);
    sort_ids(index, &mut ids, sort_key, ascending);

    let total = ids.len();
    let offset = q.offset.min(total);
    let end = (offset + q.max_results).min(total);
    let page = &ids[offset..end];

    let paths: Vec<String> = page
        .iter()
        .map(|id| index.get_path(*id).unwrap_or("").to_string())
        .collect();

    Response::Results(ResultsResponse {
        id: q.id,
        total_count: total,
        paths,
    })
}

fn sort_params(sort: needle_core::query::Sort) -> (SortKey, bool) {
    match sort {
        needle_core::query::Sort::NameAsc => (SortKey::Name, true),
        needle_core::query::Sort::NameDesc => (SortKey::Name, false),
        needle_core::query::Sort::PathAsc => (SortKey::Path, true),
        needle_core::query::Sort::PathDesc => (SortKey::Path, false),
        needle_core::query::Sort::SizeDesc => (SortKey::Size, false),
        needle_core::query::Sort::ModifiedDesc => (SortKey::Modified, false),
        needle_core::query::Sort::CreatedDesc => (SortKey::Created, false),
        needle_core::query::Sort::AccessedDesc => (SortKey::Accessed, false),
        needle_core::query::Sort::ExtensionAsc => (SortKey::Extension, true),
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

                let mut st = state.lock().unwrap();
                let resp = match req {
                    Request::Flush => match save_index(&st.index, &state_dir) {
                        Ok(()) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    },
                    Request::Reindex => {
                        let _ = fs::remove_file(state_dir.join("index.bin"));
                        let (new_index, duration) = build_index(&state_dir, &config);
                        st.index = new_index;
                        st.build_duration_ms = duration;
                        st.is_ready = true;
                        Response::Ok
                    }
                    Request::Status => Response::Status(StatusResponse {
                        indexed_count: st.index.count(),
                        is_ready: st.is_ready,
                        last_updated_unix: SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64,
                        build_duration_ms: st.build_duration_ms,
                    }),
                    Request::Query(q) => handle_query(&st.index, &q),
                    Request::Quit => unreachable!(),
                };

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

    serve(state_dir, config, state, socket).unwrap();
}

#[cfg(test)]
mod tests;
