//! ndl — CLI client for needled.

use needle_core::ipc::{OutputFormat as IpcFormat, QueryRequest, Request, Response};
use needle_core::opts::{NdlOptions, OutputFormat};
use std::env;
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
    println!("ndl 0.1.0");
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

fn run_query(sock: &Path, raw: &str, max_results: usize, offset: usize, format: OutputFormat) -> io::Result<()> {
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
    });
    send_request(&mut stream, &req)?;
    let resp = read_response(&mut stream)?;
    match resp {
        Response::Results(r) => {
            for path in r.paths {
                println!("{}", path);
            }
        }
        Response::Error(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
        _ => {}
    }
    Ok(())
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
                    "Index: {} files | ready: {} | build time: {} ms",
                    s.indexed_count, s.is_ready, s.build_duration_ms
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
        match run_query(&sock, &opts.search, opts.max_results, opts.offset, opts.format) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("query failed: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if let Err(e) = run_query(&sock, &opts.search, opts.max_results, opts.offset, opts.format) {
        eprintln!("query failed: {}", e);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests;
