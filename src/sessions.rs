use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

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

pub fn state_at(entry: &SessionEntry, now: DateTime<Utc>) -> State {
    if entry.has_last_prompt {
        return State::Stale;
    }
    let elapsed = (now - entry.last_activity).num_seconds();
    if elapsed < 300 {
        State::Active
    } else if elapsed < 3600 {
        State::Idle
    } else {
        State::Stale
    }
}

pub fn cache_ttl_remaining_secs(entry: &SessionEntry, now: DateTime<Utc>) -> i64 {
    let elapsed = (now - entry.last_activity).num_seconds();
    (300 - elapsed).max(0)
}

#[derive(Debug, Default, Clone)]
pub struct FirstRecord {
    pub session_id: Option<String>,
    pub permission_mode: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub cwd: Option<String>,
}

pub fn parse_first_record<R: Read>(reader: R) -> FirstRecord {
    let buf = BufReader::new(reader);
    let mut out = FirstRecord::default();

    for line in buf.lines().take(10).flatten() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if out.session_id.is_none()
            && let Some(sid) = value.get("sessionId").and_then(|v| v.as_str())
        {
            out.session_id = Some(sid.to_string());
        }
        if out.permission_mode.is_none()
            && value.get("type").and_then(|v| v.as_str()) == Some("permission-mode")
            && let Some(mode) = value.get("permissionMode").and_then(|v| v.as_str())
        {
            out.permission_mode = Some(mode.to_string());
        }
        if out.started_at.is_none()
            && let Some(ts) = value.get("timestamp").and_then(|v| v.as_str())
            && let Ok(dt) = DateTime::parse_from_rfc3339(ts)
        {
            out.started_at = Some(dt.with_timezone(&Utc));
        }
        if out.cwd.is_none()
            && let Some(cwd) = value.get("cwd").and_then(|v| v.as_str())
        {
            out.cwd = Some(cwd.to_string());
        }
        if out.session_id.is_some()
            && out.permission_mode.is_some()
            && out.started_at.is_some()
            && out.cwd.is_some()
        {
            break;
        }
    }
    out
}

const TAIL_CHUNK_BYTES: u64 = 8192;

#[derive(Debug, Default, Clone)]
pub struct TailRecord {
    pub last_activity: Option<DateTime<Utc>>,
    pub has_last_prompt: bool,
}

pub fn parse_tail(path: &Path) -> std::io::Result<TailRecord> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();

    if file_len > TAIL_CHUNK_BYTES {
        file.seek(SeekFrom::End(-(TAIL_CHUNK_BYTES as i64)))?;
        // Drop potentially-partial first line after seek
        let mut discard = String::new();
        let mut reader = BufReader::new(&mut file);
        let _ = reader.read_line(&mut discard);
    }

    let mut out = TailRecord::default();
    let buf = BufReader::new(file);

    for line in buf.lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) == Some("last-prompt") {
            out.has_last_prompt = true;
        }
        if let Some(ts) = value.get("timestamp").and_then(|v| v.as_str())
            && let Ok(dt) = DateTime::parse_from_rfc3339(ts)
        {
            let dt_utc = dt.with_timezone(&Utc);
            out.last_activity = Some(match out.last_activity {
                Some(prev) if prev > dt_utc => prev,
                _ => dt_utc,
            });
        }
    }
    Ok(out)
}

pub fn sort_entries(entries: &mut [SessionEntry], sort: SessionsSort, now: DateTime<Utc>) {
    match sort {
        SessionsSort::LastActivity => {
            entries.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        }
        SessionsSort::CacheTtl => {
            entries.sort_by_key(|e| cache_ttl_remaining_secs(e, now));
        }
        SessionsSort::Project => {
            entries.sort_by(|a, b| {
                a.project_name
                    .to_lowercase()
                    .cmp(&b.project_name.to_lowercase())
            });
        }
        SessionsSort::Size => {
            entries.sort_by(|a, b| b.file_size.cmp(&a.file_size));
        }
    }
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

    fn make_entry(id: &str, last_activity: DateTime<Utc>, has_last_prompt: bool) -> SessionEntry {
        SessionEntry {
            session_id: id.to_string(),
            short_id: short_id(id),
            project_name: format!("proj-{id}"),
            cwd: None,
            transcript_path: PathBuf::from(format!("/tmp/{id}.jsonl")),
            started_at: Some(last_activity),
            last_activity,
            permission_mode: Some("auto".to_string()),
            has_last_prompt,
            file_size: 1000,
        }
    }

    #[test]
    fn state_at_active_when_recent() {
        let now = Utc::now();
        let entry = make_entry("a", now - chrono::Duration::seconds(30), false);
        assert_eq!(state_at(&entry, now), State::Active);
    }

    #[test]
    fn state_at_idle_when_medium() {
        let now = Utc::now();
        let entry = make_entry("b", now - chrono::Duration::minutes(10), false);
        assert_eq!(state_at(&entry, now), State::Idle);
    }

    #[test]
    fn state_at_stale_when_old() {
        let now = Utc::now();
        let entry = make_entry("c", now - chrono::Duration::hours(2), false);
        assert_eq!(state_at(&entry, now), State::Stale);
    }

    #[test]
    fn state_at_stale_when_last_prompt_present() {
        let now = Utc::now();
        let entry = make_entry("d", now - chrono::Duration::seconds(10), true);
        assert_eq!(state_at(&entry, now), State::Stale);
    }

    #[test]
    fn sort_by_last_activity_desc() {
        let now = Utc::now();
        let mut entries = vec![
            make_entry("old", now - chrono::Duration::minutes(10), false),
            make_entry("new", now - chrono::Duration::seconds(5), false),
            make_entry("mid", now - chrono::Duration::minutes(2), false),
        ];
        sort_entries(&mut entries, SessionsSort::LastActivity, now);
        assert_eq!(entries[0].session_id, "new");
        assert_eq!(entries[1].session_id, "mid");
        assert_eq!(entries[2].session_id, "old");
    }

    #[test]
    fn sort_by_cache_ttl_asc_expiring_first() {
        let now = Utc::now();
        let mut entries = vec![
            make_entry("fresh", now - chrono::Duration::seconds(10), false),
            make_entry("expiring", now - chrono::Duration::seconds(270), false),
            make_entry("middle", now - chrono::Duration::seconds(120), false),
        ];
        sort_entries(&mut entries, SessionsSort::CacheTtl, now);
        assert_eq!(entries[0].session_id, "expiring");
        assert_eq!(entries[1].session_id, "middle");
        assert_eq!(entries[2].session_id, "fresh");
    }

    #[test]
    fn sort_by_project_alphabetical() {
        let now = Utc::now();
        let mut entries = vec![
            make_entry("c", now, false),
            make_entry("a", now, false),
            make_entry("b", now, false),
        ];
        sort_entries(&mut entries, SessionsSort::Project, now);
        assert_eq!(entries[0].session_id, "a");
        assert_eq!(entries[2].session_id, "c");
    }

    use std::io::Write;

    fn tempfile_with(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{line}").unwrap();
        }
        f
    }

    #[test]
    fn parse_tail_finds_last_timestamp() {
        let file = tempfile_with(&[
            r#"{"type":"user","timestamp":"2026-04-19T06:00:00Z","sessionId":"x"}"#,
            r#"{"type":"assistant","timestamp":"2026-04-19T06:05:00Z","sessionId":"x"}"#,
            r#"{"type":"user","timestamp":"2026-04-19T06:10:00Z","sessionId":"x"}"#,
        ]);
        let parsed = parse_tail(file.path()).unwrap();
        assert_eq!(
            parsed.last_activity.map(|d| d.to_rfc3339()),
            Some("2026-04-19T06:10:00+00:00".to_string())
        );
        assert!(!parsed.has_last_prompt);
    }

    #[test]
    fn parse_tail_detects_last_prompt_record() {
        let file = tempfile_with(&[
            r#"{"type":"user","timestamp":"2026-04-19T06:00:00Z","sessionId":"x"}"#,
            r#"{"type":"last-prompt","timestamp":"2026-04-19T06:10:00Z","lastPrompt":"bye"}"#,
        ]);
        let parsed = parse_tail(file.path()).unwrap();
        assert!(parsed.has_last_prompt);
    }

    #[test]
    fn parse_tail_handles_empty_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let parsed = parse_tail(file.path()).unwrap();
        assert!(parsed.last_activity.is_none());
        assert!(!parsed.has_last_prompt);
    }

    #[test]
    fn parse_tail_ignores_invalid_json_lines() {
        let file = tempfile_with(&[
            r#"not valid json"#,
            r#"{"type":"user","timestamp":"2026-04-19T06:00:00Z"}"#,
        ]);
        let parsed = parse_tail(file.path()).unwrap();
        assert!(parsed.last_activity.is_some());
    }

    #[test]
    fn parse_first_record_extracts_session_id_from_permission_mode() {
        let content = r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"676b2e79-2ee5-4a7b-8cd3-2a5034cac2e6"}"#;
        let parsed = parse_first_record(content.as_bytes());
        assert_eq!(
            parsed.session_id.as_deref(),
            Some("676b2e79-2ee5-4a7b-8cd3-2a5034cac2e6")
        );
        assert_eq!(parsed.permission_mode.as_deref(), Some("auto"));
    }

    #[test]
    fn parse_first_record_scans_past_file_history_snapshot() {
        let content = concat!(
            r#"{"type":"file-history-snapshot","entries":[]}"#,
            "\n",
            r#"{"type":"permission-mode","permissionMode":"default","sessionId":"abc123"}"#,
            "\n",
        );
        let parsed = parse_first_record(content.as_bytes());
        assert_eq!(parsed.session_id.as_deref(), Some("abc123"));
        assert_eq!(parsed.permission_mode.as_deref(), Some("default"));
    }

    #[test]
    fn parse_first_record_extracts_timestamp_and_cwd_from_user_record() {
        let content = concat!(
            r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"x1"}"#,
            "\n",
            r#"{"type":"user","timestamp":"2026-04-19T06:26:01.121Z","cwd":"/Users/kim/proj","sessionId":"x1"}"#,
            "\n",
        );
        let parsed = parse_first_record(content.as_bytes());
        assert!(parsed.started_at.is_some());
        assert_eq!(parsed.cwd.as_deref(), Some("/Users/kim/proj"));
    }

    #[test]
    fn parse_first_record_returns_empty_on_invalid_json() {
        let content = "not valid json\n";
        let parsed = parse_first_record(content.as_bytes());
        assert!(parsed.session_id.is_none());
        assert!(parsed.permission_mode.is_none());
    }

    #[test]
    fn parse_first_record_handles_empty_input() {
        let parsed = parse_first_record(b"" as &[u8]);
        assert!(parsed.session_id.is_none());
    }

    #[test]
    fn sort_by_size_desc() {
        let now = Utc::now();
        let mut entries = vec![
            make_entry("small", now, false),
            make_entry("big", now, false),
            make_entry("mid", now, false),
        ];
        entries[0].file_size = 100;
        entries[1].file_size = 10_000;
        entries[2].file_size = 1_000;
        sort_entries(&mut entries, SessionsSort::Size, now);
        assert_eq!(entries[0].session_id, "big");
        assert_eq!(entries[1].session_id, "mid");
        assert_eq!(entries[2].session_id, "small");
    }
}
