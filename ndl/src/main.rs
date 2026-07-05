//! ndl — CLI client for needled.

use needle_core::highlight::render_ansi;
use needle_core::ipc::{OutputFormat as IpcFormat, QueryRequest, Request, Response};
use needle_core::opts::{NdlOptions, OutputFormat};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

fn usage() {
    println!("ndl [options] <search text>");
    println!();
    println!("Search options:");
    println!("  -r, -regex <search>   Regex search");
    println!("  -i, -case             Match case");
    println!("  -w, -ww               Match whole word");
    println!("  -p, -match-path       Match full path");
    println!("  -o, -offset <n>       Start from result n");
    println!("  -n, -max-results <n>  Max results");
    println!();
    println!("Info:");
    println!("  -status               Daemon status");
    println!("  -save-db              Force daemon to save index");
    println!("  -reindex              Force daemon to rebuild index");
    println!("  -h, -help             Show this help");
    println!("  -v, -version          Show version");
}

fn version() {
    println!("ndl 0.1.1");
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

fn socket_path() -> PathBuf {
    env::var_os("NEEDLE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_state_dir().join("needled.sock"))
}

fn ensure_daemon_running(sock: &Path) {
    if sock.exists() {
        return;
    }
    eprintln!("needled is not running. Starting it...");
    let _ = Command::new("needled").spawn();
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(50));
        if sock.exists() {
            return;
        }
    }
}

fn wait_for_ready(sock: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match send_simple(sock, Request::Status) {
            Ok(Response::Status(status)) if status.is_ready => return Ok(()),
            Ok(Response::Status(_)) => {}
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
    let mut len_buf = [0u8; 8];
    stream.read_exact(&mut len_buf)?;
    let len = u64::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Response::decode(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn run_query(
    sock: &Path,
    raw: &str,
    max_results: usize,
    offset: usize,
    format: OutputFormat,
    highlight: bool,
) -> io::Result<needle_core::ipc::ResultsResponse> {
    let mut stream = connect(sock)?;
    let format = match format {
        OutputFormat::Default => IpcFormat::Default,
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

fn render_results(paths: &[String], format: OutputFormat, no_header: bool) -> String {
    match format {
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

fn send_simple(sock: &Path, req: Request) -> io::Result<Response> {
    let mut stream = connect(sock)?;
    send_request(&mut stream, &req)?;
    read_response(&mut stream)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let opts = match NdlOptions::parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("ndl: {}", e);
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
    ensure_daemon_running(&sock);

    if !sock.exists() {
        eprintln!("needled is not running. Start it with: needled &");
        process::exit(8);
    }

    if opts.status {
        match send_simple(&sock, Request::Status) {
            Ok(Response::Status(s)) => {
                println!(
                    "Index: {} files | ready: {} | watcher healthy: {} | watched dirs: {} | watch failures: {} | overflows: {} | build time: {} ms",
                    s.indexed_count,
                    s.is_ready,
                    s.watcher_healthy,
                    s.watched_dir_count,
                    s.watch_failure_count,
                    s.watch_overflow_count,
                    s.build_duration_ms
                );
            }
            Ok(_) => eprintln!("unexpected response"),
            Err(e) => {
                eprintln!("failed to get status: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if opts.save_db {
        match send_simple(&sock, Request::Flush) {
            Ok(Response::Ok) => {}
            Ok(Response::Error(e)) => {
                eprintln!("error: {}", e);
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
        match send_simple(&sock, Request::Reindex) {
            Ok(Response::Ok) => {}
            Ok(Response::Error(e)) => {
                eprintln!("error: {}", e);
                process::exit(1);
            }
            _ => {
                eprintln!("failed to reindex");
                process::exit(1);
            }
        }
        return;
    }

    if opts.get_result_count {
        if let Err(e) = wait_for_ready(&sock, Duration::from_secs(30)) {
            eprintln!("query failed: {}", e);
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
                eprintln!("query failed: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if opts.get_total_size {
        if let Err(e) = wait_for_ready(&sock, Duration::from_secs(30)) {
            eprintln!("query failed: {}", e);
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
                eprintln!("query failed: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if let Err(e) = wait_for_ready(&sock, Duration::from_secs(30)) {
        eprintln!("query failed: {}", e);
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
            eprintln!("query failed: {}", e);
            process::exit(1);
        }
    };

    let mut paths = results.paths;
    if opts.highlight {
        let color = opts.highlight_color;
        paths = paths.into_iter().map(|p| render_ansi(&p, color)).collect();
    }

    if opts.no_result_error && paths.is_empty() {
        process::exit(9);
    }

    if opts.hide_empty && paths.is_empty() {
        return;
    }

    let output = render_results(&paths, opts.format, opts.no_header);

    if let Some(path) = &opts.export_file {
        if let Err(e) = fs::write(path, &output) {
            eprintln!("failed to write export: {}", e);
            process::exit(1);
        }
        return;
    }

    print!("{}", output);
    if let Err(e) = io::stdout().flush() {
        eprintln!("query failed: {}", e);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests;
