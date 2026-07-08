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
