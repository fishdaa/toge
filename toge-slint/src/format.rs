#[allow(
    clippy::cast_precision_loss,
    reason = "approximate human-readable size formatting; precision loss only matters above 2^52 bytes"
)]
pub fn format_size(size: u64) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = size as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx + 1 < units.len() {
        value /= 1024.0;
        unit_idx += 1;
    }
    format!("{:.1} {}", value, units[unit_idx])
}

/// Search summary shown in the status bar, as in toge-gui.
pub fn search_status(total_count: usize, total_size: u64, size_indexed: bool) -> String {
    if size_indexed {
        format!("{total_count} results | {}", format_size(total_size))
    } else {
        format!("{total_count} results | size unavailable")
    }
}

/// Daemon summary shown in the status bar, as in toge-gui.
pub fn index_status(status: &toge_core::ipc::StatusResponse) -> String {
    let count = format!("{} indexed", group_digits(status.indexed_count));
    if !status.watcher_healthy && status.watch_failure_count > 0 {
        return format!("Live updates unavailable | {count}");
    }
    let state = format!("{:?}", status.status);
    let message = status.status_message.trim();
    if message.is_empty() || message == state {
        format!("{state} | {count}")
    } else {
        format!("{state} | {message} | {count}")
    }
}

fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn format_time(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }

    if let Some(local) = format_time_local(unix) {
        return local;
    }

    format_time_utc(unix)
}

fn format_time_utc(unix: i64) -> String {
    let dt =
        time::OffsetDateTime::from_unix_timestamp(unix).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute()
    )
}

#[cfg(target_os = "linux")]
fn format_time_local(unix: i64) -> Option<String> {
    use std::mem::MaybeUninit;
    use std::os::raw::{c_char, c_int, c_long};

    #[repr(C)]
    struct Tm {
        tm_sec: c_int,
        tm_min: c_int,
        tm_hour: c_int,
        tm_mday: c_int,
        tm_mon: c_int,
        tm_year: c_int,
        tm_wday: c_int,
        tm_yday: c_int,
        tm_isdst: c_int,
        tm_gmtoff: c_long,
        tm_zone: *const c_char,
    }

    unsafe extern "C" {
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }

    let mut tm = MaybeUninit::<Tm>::uninit();
    let ptr = unsafe { localtime_r(&raw const unix, tm.as_mut_ptr()) };
    if ptr.is_null() {
        return None;
    }

    let tm = unsafe { tm.assume_init() };
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    ))
}

#[cfg(not(target_os = "linux"))]
fn format_time_local(_unix: i64) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_uses_utc_fallback_shape() {
        assert_eq!(format_time_utc(60), "1970-01-01 00:01");
    }

    #[test]
    fn format_time_returns_empty_for_zero() {
        assert_eq!(format_time(0), "");
    }

    #[test]
    fn search_status_matches_gui_format() {
        assert_eq!(search_status(3, 2048, true), "3 results | 2.0 KB");
        assert_eq!(search_status(1, 0, false), "1 results | size unavailable");
    }

    #[test]
    fn index_status_matches_gui_format() {
        let mut status = toge_core::ipc::StatusResponse {
            indexed_count: 1_234_567,
            status: toge_core::ipc::DaemonStatus::Ready,
            status_message: "Ready".into(),
            watcher_healthy: true,
            watched_dir_count: 0,
            watch_failure_count: 0,
            watch_overflow_count: 0,
            watcher_log: Vec::new(),
            last_updated_unix: 0,
            build_duration_ms: 0,
        };
        assert_eq!(index_status(&status), "Ready | 1,234,567 indexed");
        status.status = toge_core::ipc::DaemonStatus::Indexing;
        status.status_message = "Scanning /home".into();
        status.indexed_count = 999;
        assert_eq!(
            index_status(&status),
            "Indexing | Scanning /home | 999 indexed"
        );
        status.watcher_healthy = false;
        status.watch_failure_count = 2;
        assert_eq!(
            index_status(&status),
            "Live updates unavailable | 999 indexed"
        );
    }
}
