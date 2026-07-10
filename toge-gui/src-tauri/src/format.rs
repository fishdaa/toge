pub fn format_size(size: u64) -> String {
    if size < 1024 {
        return format!("{} B", size);
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
    let ptr = unsafe { localtime_r(&unix, tm.as_mut_ptr()) };
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
}
