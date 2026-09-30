//! toge — CLI client for toged.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::thread;
use std::time::Duration;
use toge_core::highlight::render_ansi;
use toge_core::ipc::{
    DaemonStatus, MAX_IPC_MESSAGE_SIZE, OutputFormat as IpcFormat, QueryRequest, Request, Response,
    ResultRow, StatusResponse, StreamOrder, StreamQueryRequest, StreamSummary, stream_query,
};
use toge_core::opts::{NdlOptions, OutputFormat};

fn usage() {
    println!("toge [options] <search text>");
    println!();
    println!("Search options:");
    println!("  -r, -regex <search>   Regex search");
    println!("  -i, -case             Match case");
    println!("  -w, -ww               Match whole word");
    println!("  -p, -match-path       Match full path");
    println!("  -o, -offset <n>       Start from result n");
    println!("  -n, -max-results <n>  Max results");
    println!("  --stream             Stream results in index order (--sort enables sorting)");
    println!();
    println!("Output:");
    println!("  --json                JSON Lines output (one object per result or status)");
    println!("  --no-wait             Exit with code 10 instead of waiting for the index");
    println!("  --                    Treat all following arguments as search text");
    println!();
    println!("Info:");
    println!("  -status               Daemon status");
    println!("  -save-db              Force daemon to save index");
    println!("  -reindex              Force daemon to rebuild index");
    println!("  -h, -help             Show this help");
    println!("  -v, -version          Show version");
}

fn version() {
    println!("toge {}", env!("CARGO_PKG_VERSION"));
}

fn default_state_dir() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map_or_else(
            || {
                let home = env::var_os("HOME").expect("HOME not set");
                PathBuf::from(home).join(".local/state")
            },
            PathBuf::from,
        )
        .join("toge")
}

fn socket_path() -> PathBuf {
    env::var_os("TOGE_SOCKET").map_or_else(|| default_state_dir().join("toged.sock"), PathBuf::from)
}

fn ensure_daemon_running(sock: &Path) -> io::Result<()> {
    if daemon_responding(sock) {
        return Ok(());
    }
    eprintln!("toged is not running. Starting it...");
    // Detach the daemon into its own process group so it outlives callers
    // that kill the client's group on timeout (e.g. shell launcher plugins).
    daemon_command(sock)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    for _ in 0..100 {
        thread::sleep(Duration::from_millis(50));
        if daemon_responding(sock) {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "daemon did not start",
    ))
}

/// Exit code returned by `--no-wait` when the daemon is still loading or indexing.
const EXIT_NOT_READY: i32 = 10;

/// Wait for the daemon to become ready, or exit immediately with
/// [`EXIT_NOT_READY`] when `--no-wait` is set and it is not ready yet.
fn ensure_ready(sock: &Path, opts: &NdlOptions) -> io::Result<()> {
    if !opts.no_wait {
        return wait_for_ready(sock, Duration::from_secs(30));
    }
    match send_simple(sock, &Request::Status)? {
        Response::Status(status) if status.status == DaemonStatus::Ready => Ok(()),
        Response::Status(status) => {
            if opts.format == OutputFormat::Jsonl {
                println!("{}", render_status_json(&status));
            }
            eprintln!("toge: daemon not ready: {}", status.status_message);
            process::exit(EXIT_NOT_READY);
        }
        Response::Error(e) => Err(io::Error::other(e)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response type",
        )),
    }
}

fn wait_for_ready(sock: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match send_simple(sock, &Request::Status) {
            Ok(Response::Status(status)) if status.status == DaemonStatus::Ready => return Ok(()),
            Ok(Response::Error(e)) => return Err(io::Error::other(e)),
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionAborted
                ) => {}
            Err(e) => return Err(e),
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "daemon did not become ready in time",
    ))
}

fn connect(sock: &Path) -> io::Result<UnixStream> {
    UnixStream::connect(sock)
}

fn send_request(stream: &mut UnixStream, req: &Request) -> io::Result<()> {
    let bytes = req.encode();
    stream.write_all(&(bytes.len() as u64).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn read_response(stream: &mut UnixStream) -> io::Result<Response> {
    read_response_from(stream)
}

fn read_response_from<R: Read>(reader: &mut R) -> io::Result<Response> {
    let mut len_buf = [0u8; 8];
    reader.read_exact(&mut len_buf)?;
    let len = usize::try_from(u64::from_le_bytes(len_buf)).unwrap_or(usize::MAX);
    if len > MAX_IPC_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response too large",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Response::decode(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn daemon_responding(sock: &Path) -> bool {
    matches!(send_simple(sock, &Request::Status), Ok(Response::Status(_)))
}

fn daemon_command(sock: &Path) -> Command {
    if let Ok(current) = env::current_exe()
        && let Some(bin_dir) = current.parent()
    {
        let sibling = bin_dir.join("toged");
        if sibling.exists() {
            let mut cmd = Command::new(sibling);
            cmd.arg("--socket").arg(sock);
            return cmd;
        }
    }
    let mut cmd = Command::new("toged");
    cmd.arg("--socket").arg(sock);
    cmd
}

fn run_query(
    sock: &Path,
    raw: &str,
    max_results: usize,
    offset: usize,
    format: OutputFormat,
    highlight: bool,
) -> io::Result<toge_core::ipc::ResultsResponse> {
    let mut stream = connect(sock)?;
    let format = match format {
        OutputFormat::Default | OutputFormat::Jsonl => IpcFormat::Default,
        OutputFormat::Csv => IpcFormat::Csv,
        OutputFormat::Tsv => IpcFormat::Tsv,
        OutputFormat::Txt => IpcFormat::Txt,
        OutputFormat::Efu => IpcFormat::Efu,
    };
    let req = Request::Query(QueryRequest {
        id: 1,
        raw: raw.to_string(),
        max_results,
        offset,
        format,
        highlight,
    });
    send_request(&mut stream, &req)?;
    let resp = read_response(&mut stream)?;
    match resp {
        Response::Results(r) => Ok(r),
        Response::Error(e) => Err(io::Error::other(e)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected response type",
        )),
    }
}

fn run_streamed_query<W: Write>(
    sock: &Path,
    opts: &NdlOptions,
    output: &mut W,
) -> io::Result<StreamSummary> {
    let mut connection = connect(sock)?;
    let mut header_written = opts.no_header;
    let totals_only = opts.get_result_count || opts.get_total_size;
    let request = StreamQueryRequest {
        query: QueryRequest {
            id: 1,
            raw: opts.search.clone(),
            max_results: if totals_only { 0 } else { opts.max_results },
            offset: opts.offset,
            // Format is rendered by the client, just as with paginated queries.
            format: IpcFormat::Default,
            highlight: opts.highlight && !totals_only,
        },
        // `--sort`, `/o` flags and inline `sort:` all land in the search text.
        order: if opts
            .search
            .split_whitespace()
            .any(|token| token.to_ascii_lowercase().starts_with("sort:"))
        {
            StreamOrder::Sorted
        } else {
            StreamOrder::Index
        },
    };
    let summary = stream_query(&mut connection, &request, |rows| {
        if opts.format == OutputFormat::Jsonl {
            output.write_all(render_jsonl(rows).as_bytes())?;
            return output.flush();
        }
        let paths: Vec<String> = rows
            .iter()
            .map(|row| {
                if opts.highlight {
                    render_ansi(&row.path, opts.highlight_color)
                } else {
                    row.path.clone()
                }
            })
            .collect();
        output.write_all(render_results(&paths, opts.format, header_written).as_bytes())?;
        header_written = true;
        output.flush()
    })?;
    if totals_only {
        if opts.get_result_count {
            writeln!(output, "{}", summary.total_count)?;
        } else {
            writeln!(output, "{}", summary.total_size)?;
        }
    } else if !header_written && !opts.hide_empty && !opts.no_result_error {
        output.write_all(render_results(&[], opts.format, false).as_bytes())?;
    }
    output.flush()?;
    Ok(summary)
}

fn render_results(paths: &[String], format: OutputFormat, no_header: bool) -> String {
    match format {
        OutputFormat::Jsonl => paths.iter().fold(String::new(), |mut output, path| {
            let _ = writeln!(output, "{{\"path\":{}}}", json_string(path));
            output
        }),
        OutputFormat::Csv => render_table(paths, "Name", ",", "\r\n", no_header),
        OutputFormat::Tsv => render_table(paths, "Name", "\t", "\n", no_header),
        OutputFormat::Txt | OutputFormat::Default | OutputFormat::Efu => {
            let mut output = paths.join("\n");
            if !output.is_empty() {
                output.push('\n');
            }
            output
        }
    }
}

fn render_table(
    paths: &[String],
    header: &str,
    sep: &str,
    line_end: &str,
    no_header: bool,
) -> String {
    let mut output = String::new();
    if !no_header {
        output.push_str(header);
        output.push_str(line_end);
    }
    for path in paths {
        if sep == "," {
            output.push('"');
            output.push_str(&path.replace('"', "\"\""));
            output.push('"');
        } else {
            output.push_str(path);
        }
        output.push_str(line_end);
    }
    output
}

fn render_jsonl(rows: &[ResultRow]) -> String {
    let mut output = String::new();
    for row in rows {
        let _ = writeln!(
            output,
            "{{\"path\":{},\"name\":{},\"parent\":{},\"ext\":{},\"is_dir\":{},\"size\":{},\"modified\":{}}}",
            json_string(&row.path),
            json_string(&row.name),
            json_string(&row.parent),
            json_string(&row.extension),
            row.is_dir,
            row.size,
            row.modified_unix,
        );
    }
    output
}

fn render_status_json(status: &StatusResponse) -> String {
    let name = match status.status {
        DaemonStatus::Starting => "starting",
        DaemonStatus::LoadingConfig => "loading_config",
        DaemonStatus::LoadingIndex => "loading_index",
        DaemonStatus::Indexing => "indexing",
        DaemonStatus::StartingWatcher => "starting_watcher",
        DaemonStatus::Ready => "ready",
        DaemonStatus::Error => "error",
    };
    format!(
        "{{\"status\":\"{}\",\"ready\":{},\"message\":{},\"indexed_count\":{},\"watcher_healthy\":{},\"watched_dir_count\":{},\"build_duration_ms\":{}}}",
        name,
        status.status == DaemonStatus::Ready,
        json_string(&status.status_message),
        status.indexed_count,
        status.watcher_healthy,
        status.watched_dir_count,
        status.build_duration_ms,
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn send_simple(sock: &Path, req: &Request) -> io::Result<Response> {
    let mut stream = connect(sock)?;
    send_request(&mut stream, req)?;
    read_response(&mut stream)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let opts = match NdlOptions::parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("toge: {e}");
            process::exit(2);
        }
    };

    if opts.help {
        usage();
        return;
    }
    if opts.version {
        version();
        return;
    }

    let sock = socket_path();
    if let Err(e) = ensure_daemon_running(&sock) {
        eprintln!("failed to start daemon: {e}");
        process::exit(1);
    }

    if !sock.exists() {
        eprintln!("toged is not running. Start it with: toged &");
        process::exit(8);
    }

    if opts.status {
        match send_simple(&sock, &Request::Status) {
            Ok(Response::Status(s)) if opts.format == OutputFormat::Jsonl => {
                println!("{}", render_status_json(&s));
            }
            Ok(Response::Status(s)) => {
                println!(
                    "Index: {} files | status: {:?} | {} | watcher healthy: {} | watched dirs: {} | watch failures: {} | overflows: {} | build time: {} ms",
                    s.indexed_count,
                    s.status,
                    s.status_message,
                    s.watcher_healthy,
                    s.watched_dir_count,
                    s.watch_failure_count,
                    s.watch_overflow_count,
                    s.build_duration_ms
                );
                if !s.watcher_healthy && s.watch_failure_count > 0 {
                    eprintln!(
                        "warning: live updates are unavailable because fanotify setup failed. Run `sudo setcap cap_sys_admin,cap_dac_read_search+ep <path-to-toged>` (the `toged` beside your Toge binaries, or `$(command -v toged)`), then restart Toge."
                    );
                }
            }
            Ok(_) => eprintln!("unexpected response"),
            Err(e) => {
                eprintln!("failed to get status: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if opts.save_db {
        match send_simple(&sock, &Request::Flush) {
            Ok(Response::Ok) => {}
            Ok(Response::Error(e)) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
            _ => {
                eprintln!("failed to save db");
                process::exit(1);
            }
        }
        return;
    }

    if opts.reindex {
        match send_simple(&sock, &Request::Reindex) {
            Ok(Response::Ok) => {}
            Ok(Response::Error(e)) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
            _ => {
                eprintln!("failed to reindex");
                process::exit(1);
            }
        }
        return;
    }

    if opts.stream {
        let result = (|| -> io::Result<StreamSummary> {
            ensure_ready(&sock, &opts)?;
            if let Some(path) = opts
                .export_file
                .as_ref()
                .filter(|_| !opts.get_result_count && !opts.get_total_size)
            {
                // Stream into a temporary file and rename on completion so an
                // interrupted export does not replace an existing output file.
                let target = Path::new(path);
                let temporary = target.with_extension(format!("stream-{}.tmp", process::id()));
                let mut created = false;
                let result = (|| {
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&temporary)?;
                    created = true;
                    let summary = run_streamed_query(&sock, &opts, &mut file)?;
                    file.sync_all()?;
                    drop(file);
                    fs::rename(&temporary, target)?;
                    Ok(summary)
                })();
                if result.is_err() && created {
                    let _ = fs::remove_file(&temporary);
                }
                result
            } else {
                run_streamed_query(&sock, &opts, &mut io::stdout().lock())
            }
        })();
        match result {
            Ok(summary)
                if opts.no_result_error
                    && summary.returned_count == 0
                    && !opts.get_result_count
                    && !opts.get_total_size =>
            {
                process::exit(9)
            }
            Ok(_) => return,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => return,
            Err(error) => {
                eprintln!("stream query failed: {error}");
                process::exit(1);
            }
        }
    }

    if opts.get_result_count {
        if let Err(e) = ensure_ready(&sock, &opts) {
            eprintln!("query failed: {e}");
            process::exit(1);
        }
        match run_query(
            &sock,
            &opts.search,
            opts.max_results,
            opts.offset,
            opts.format,
            false,
        ) {
            Ok(results) => println!("{}", results.total_count),
            Err(e) => {
                eprintln!("query failed: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if opts.get_total_size {
        if let Err(e) = ensure_ready(&sock, &opts) {
            eprintln!("query failed: {e}");
            process::exit(1);
        }
        match run_query(
            &sock,
            &opts.search,
            opts.max_results,
            opts.offset,
            opts.format,
            false,
        ) {
            Ok(results) => println!("{}", results.total_size),
            Err(e) => {
                eprintln!("query failed: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if let Err(e) = ensure_ready(&sock, &opts) {
        eprintln!("query failed: {e}");
        process::exit(1);
    }

    let results = match run_query(
        &sock,
        &opts.search,
        opts.max_results,
        opts.offset,
        opts.format,
        opts.highlight,
    ) {
        Ok(results) => results,
        Err(e) => {
            eprintln!("query failed: {e}");
            process::exit(1);
        }
    };

    if opts.no_result_error && results.rows.is_empty() {
        process::exit(9);
    }

    if opts.hide_empty && results.rows.is_empty() {
        return;
    }

    let output = if opts.format == OutputFormat::Jsonl {
        render_jsonl(&results.rows)
    } else {
        let mut paths = results.paths();
        if opts.highlight {
            let color = opts.highlight_color;
            paths = paths.into_iter().map(|p| render_ansi(&p, color)).collect();
        }
        render_results(&paths, opts.format, opts.no_header)
    };

    if let Some(path) = &opts.export_file {
        if let Err(e) = fs::write(path, &output) {
            eprintln!("failed to write export: {e}");
            process::exit(1);
        }
        return;
    }

    print!("{output}");
    if let Err(e) = io::stdout().flush() {
        eprintln!("query failed: {e}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests;
