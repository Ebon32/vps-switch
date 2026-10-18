//! 与旧前端一致的数值与时间格式化。

pub fn format_bytes(bytes: f64) -> String {
    if bytes <= 0.0 {
        return "0 B".into();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut index = (bytes.ln() / 1024f64.ln()).floor() as usize;
    index = index.min(UNITS.len() - 1);
    let value = bytes / 1024f64.powi(index as i32);
    if value >= 100.0 || index == 0 {
        format!("{value:.0} {}", UNITS[index])
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

pub fn format_rate(bps: f64) -> String {
    if bps <= 0.0 {
        "0 B/s".into()
    } else {
        format!("{}/s", format_bytes(bps))
    }
}

pub fn format_uptime(seconds: f64) -> String {
    if seconds <= 0.0 {
        return "--".into();
    }
    let secs = seconds as u64;
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}天 {hours}小时")
    } else if hours > 0 {
        format!("{hours}小时 {minutes}分")
    } else {
        format!("{minutes}分钟")
    }
}

/// 将毫秒时间戳格式化为 `YYYY-MM-DD HH:MM`（按 UTC+8 本地常用时区偏移）。
pub fn format_mtime(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let total_secs = (ms / 1000) + 8 * 3600;
    let days = total_secs.div_euclid(86400);
    let day_secs = total_secs.rem_euclid(86400);
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;

    // Howard Hinnant civil_from_days
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02} {hour:02}:{minute:02}")
}
