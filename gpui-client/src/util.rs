//! 与旧前端一致的数值格式化。

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
