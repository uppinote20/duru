use std::path::PathBuf;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    pub session_id: String,
    pub short_id: String,
    pub project_name: String,
    pub cwd: Option<PathBuf>,
    pub transcript_path: PathBuf,
    pub started_at: Option<DateTime<Utc>>,
    pub last_activity: DateTime<Utc>,
    pub permission_mode: Option<String>,
    pub has_last_prompt: bool,
    pub file_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Idle,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionsSort {
    LastActivity,
    CacheTtl,
    Project,
    Size,
}

impl Default for SessionsSort {
    fn default() -> Self {
        Self::LastActivity
    }
}

pub fn short_id(uuid: &str) -> String {
    uuid.chars().take(8).collect()
}

pub fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1_000_000 {
        format!("{}K", bytes / 1000)
    } else {
        format!("{:.1}M", bytes as f64 / 1_000_000.0)
    }
}

pub fn middle_truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1) / 2;
    let chars: Vec<char> = s.chars().collect();
    let head: String = chars.iter().take(keep).collect();
    let tail: String = chars[count - keep..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_takes_first_8_chars() {
        assert_eq!(short_id("676b2e79-2ee5-4a7b-8cd3-2a5034cac2e6"), "676b2e79");
    }

    #[test]
    fn short_id_pads_when_input_shorter_than_8() {
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn format_duration_sec() {
        assert_eq!(format_duration(12), "12s");
    }

    #[test]
    fn format_duration_min() {
        assert_eq!(format_duration(180), "3m");
    }

    #[test]
    fn format_duration_hour() {
        assert_eq!(format_duration(3700), "1h");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(180_000), "180K");
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(1_200_000), "1.2M");
    }

    #[test]
    fn format_bytes_under_1k() {
        assert_eq!(format_bytes(512), "512B");
    }

    #[test]
    fn middle_truncate_leaves_short_string() {
        assert_eq!(middle_truncate("short", 10), "short");
    }

    #[test]
    fn middle_truncate_respects_max() {
        assert_eq!(middle_truncate("my-very-long-project", 10), "my-v…ject");
    }
}
