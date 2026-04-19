use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::scan::decode_project_name;

use crate::registry::RegistrySource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    pub session_id: String,
    pub short_id: String,
    pub project_name: String,
    pub cwd: Option<PathBuf>,
    pub transcript_path: PathBuf,
    pub started_at: Option<DateTime<Utc>>,
    pub last_activity: DateTime<Utc>,
    pub file_size: u64,
    pub permission_mode: Option<String>,
    pub registry_source: Option<RegistrySource>,
    pub is_alive: Option<bool>,
}

/// Two-state model aligned with Anthropic's 5-minute prompt cache TTL:
/// either the cache is warm (last write within window) or it's cold.
/// A middle "Idle" grade would be misleading — the cache doesn't have one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionsSort {
    #[default]
    LastActivity,
    CacheTtl,
    Project,
    Size,
}

pub fn short_id(uuid: &str) -> String {
    uuid.chars().take(SHORT_ID_LEN).collect()
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
    } else if bytes < 1024 * 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
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

/// Anthropic's default prompt cache TTL in seconds (5 minutes).
///
/// Canonical source; `ui::render_ttl_cell` imports it as the bar denominator.
/// The `State::Active` window is aligned with this value so the glyph visually
/// signals "cache likely still warm".
pub const TTL_SECS: i64 = 300;

/// First N lines of a transcript scanned to extract session_id, started_at,
/// and cwd. Enough to skip past `file-history-snapshot` records that sometimes
/// precede the first real record.
const FIRST_RECORD_SCAN_LINES: usize = 10;

/// Short-form session ID length (first N chars of the UUID).
const SHORT_ID_LEN: usize = 8;

/// Sessions with mtime older than this skip the JSONL open/parse entirely on
/// discovery. Their rows show correctly as Stale from metadata alone
/// (filename → id, directory → project, mtime → last_activity, stat → size),
/// but started_at and cwd render as "—". Dramatically cuts cold-boot time
/// when `~/.claude/projects` has thousands of historical transcripts.
const LAZY_PARSE_CUTOFF_SECS: i64 = 86_400; // 24 h

/// Classifies a session.
///
/// When a hook registry entry is available, its signals are authoritative:
/// Terminated flag → Stale; dead pid → Stale. Otherwise falls back to the
/// pure-mtime heuristic keyed on the 5-minute prompt-cache TTL.
pub fn state_at(entry: &SessionEntry, now: DateTime<Utc>) -> State {
    if let Some(RegistrySource::Terminated) = entry.registry_source {
        return State::Stale;
    }
    if entry.is_alive == Some(false) {
        return State::Stale;
    }
    let elapsed = (now - entry.last_activity).num_seconds();
    if elapsed < TTL_SECS {
        State::Active
    } else {
        State::Stale
    }
}

pub fn cache_ttl_remaining_secs(entry: &SessionEntry, now: DateTime<Utc>) -> i64 {
    let elapsed = (now - entry.last_activity).num_seconds();
    (TTL_SECS - elapsed).max(0)
}

#[derive(Debug, Default, Clone)]
pub struct FirstRecord {
    pub session_id: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub cwd: Option<String>,
}

pub fn parse_first_record<R: Read>(reader: R) -> FirstRecord {
    let buf = BufReader::new(reader);
    let mut out = FirstRecord::default();

    for line in buf
        .lines()
        .take(FIRST_RECORD_SCAN_LINES)
        .map_while(Result::ok)
    {
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
        if out.session_id.is_some() && out.started_at.is_some() && out.cwd.is_some() {
            break;
        }
    }
    out
}

const TAIL_CHUNK_BYTES: u64 = 8192;

#[derive(Debug, Default, Clone)]
pub struct TailRecord {
    pub last_activity: Option<DateTime<Utc>>,
}

pub fn parse_tail(path: &Path) -> std::io::Result<TailRecord> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let skip_first = file_len > TAIL_CHUNK_BYTES;

    if skip_first {
        file.seek(SeekFrom::End(-(TAIL_CHUNK_BYTES as i64)))?;
    }

    let mut out = TailRecord::default();
    let reader = BufReader::new(file);
    let mut lines = reader.lines().map_while(Result::ok);

    // Single BufReader is required: a second BufReader on the same File would
    // share the OS file offset. The first fill_buf drains up to 8 KiB in one
    // read() syscall, leaving the offset at EOF; a fresh BufReader would then
    // yield nothing.
    if skip_first {
        lines.next(); // potentially-partial first line from mid-file seek
    }

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
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

#[derive(Debug, Default)]
pub struct SessionCache {
    by_path: HashMap<PathBuf, (SessionEntry, SystemTime)>,
}

/// For each pid that appears on multiple Alive entries, keep the entry with
/// the most recent `last_activity` as Alive; mark the others Terminated.
/// Uses `(pid, started_at)` identity — entries with started_at more than
/// 60s apart are treated as distinct processes (pid-reuse guard).
pub(crate) fn dedup_same_pid(
    entries: &mut std::collections::HashMap<PathBuf, (SessionEntry, SystemTime)>,
    pid_lookup: &std::collections::HashMap<PathBuf, u32>,
) {
    use std::collections::HashMap;

    let mut groups: HashMap<u32, Vec<PathBuf>> = HashMap::new();
    for (path, (entry, _)) in entries.iter() {
        if entry.registry_source != Some(RegistrySource::Alive) {
            continue;
        }
        if let Some(&pid) = pid_lookup.get(path) {
            groups.entry(pid).or_default().push(path.clone());
        }
    }

    for (_, paths) in groups.iter().filter(|(_, p)| p.len() > 1) {
        let latest = paths
            .iter()
            .max_by_key(|p| entries[*p].0.last_activity)
            .cloned()
            .unwrap();
        let latest_started = entries[&latest].0.started_at;
        for p in paths {
            if *p == latest {
                continue;
            }
            let this_started = entries[p].0.started_at;
            // If either entry lacks started_at we fall through to the dedup
            // (treat as same process). Very old transcripts from before hooks
            // existed may have no started_at, so this can over-eagerly mark
            // such entries Terminated — acceptable since they would be Stale
            // by mtime anyway.
            let close_enough = match (latest_started, this_started) {
                (Some(a), Some(b)) => (a - b).num_seconds().abs() < 60,
                _ => true,
            };
            if close_enough && let Some((e, _)) = entries.get_mut(p) {
                e.registry_source = Some(RegistrySource::Terminated);
            }
        }
    }
}

impl SessionCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> Vec<SessionEntry> {
        self.by_path.values().map(|(e, _)| e.clone()).collect()
    }

    pub fn refresh(&mut self, claude_dir: &Path) {
        use crate::registry::{self, Registry};

        // 1. Collect transcript files and (re-)parse by mtime.
        let found = walk_session_files(claude_dir);
        let found_paths: std::collections::HashSet<PathBuf> =
            found.iter().map(|(p, _)| p.clone()).collect();

        self.by_path.retain(|path, _| found_paths.contains(path));

        for (path, mtime) in found {
            let needs_reparse = match self.by_path.get(&path) {
                Some((_, prev_mtime)) => *prev_mtime != mtime,
                None => true,
            };
            if !needs_reparse {
                continue;
            }
            if let Some(entry) = parse_session(&path) {
                self.by_path.insert(path.clone(), (entry, mtime));
            }
        }

        // 2. Load registry and merge hook-sourced signals where paths match.
        let reg = Registry::load_all(claude_dir);
        let mut pid_lookup: std::collections::HashMap<PathBuf, u32> =
            std::collections::HashMap::new();
        for (path, (entry, _)) in self.by_path.iter_mut() {
            if let Some(reg_entry) = reg.get_by_transcript_path(path) {
                entry.permission_mode = reg_entry.permission_mode.clone();
                entry.registry_source = Some(registry::classify(reg_entry));
                entry.is_alive = reg_entry.pid.map(registry::is_pid_alive);
                if let Some(pid) = reg_entry.pid {
                    pid_lookup.insert(path.clone(), pid);
                }
            } else {
                entry.permission_mode = None;
                entry.registry_source = None;
                entry.is_alive = None;
            }
        }

        // 3. /clear detection: same pid on multiple alive entries → older → Terminated.
        dedup_same_pid(&mut self.by_path, &pid_lookup);

        // 4. Prune terminated entries older than TERMINATED_TTL_SECS.
        Registry::cleanup_expired(claude_dir, chrono::Utc::now());
    }
}

fn walk_session_files(claude_dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    let projects_dir = claude_dir.join("projects");
    let Ok(project_iter) = std::fs::read_dir(&projects_dir) else {
        return out;
    };
    for project_entry in project_iter.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(file_iter) = std::fs::read_dir(&project_path) else {
            continue;
        };
        for file_entry in file_iter.flatten() {
            let path = file_entry.path();
            if !path.is_file() {
                continue;
            }
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if fname == "skill-injections.jsonl" {
                continue;
            }
            if !fname.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = path.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            out.push((path, mtime));
        }
    }
    out
}

fn parse_session(path: &Path) -> Option<SessionEntry> {
    parse_session_at(path, Utc::now())
}

fn parse_session_at(path: &Path, now: DateTime<Utc>) -> Option<SessionEntry> {
    let meta = path.metadata().ok()?;
    let mtime_sys = meta.modified().ok()?;
    let mtime = DateTime::<Utc>::from(mtime_sys);

    let filename_uuid = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let project_dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let project_name = decode_project_name(&project_dir_name).unwrap_or(project_dir_name);

    // Lazy parse: for files older than the cutoff, skip opening the file.
    // The row still renders correctly as Stale — only started_at and cwd are
    // unknown ("—" in the detail panel), which is a reasonable tradeoff for
    // the 5-10x cold-scan speedup on setups with thousands of historical
    // transcripts.
    let (first, tail) = if (now - mtime).num_seconds() > LAZY_PARSE_CUTOFF_SECS {
        (FirstRecord::default(), TailRecord::default())
    } else {
        let file = File::open(path).ok()?;
        let first = parse_first_record(file);
        let tail = parse_tail(path).unwrap_or_default();
        (first, tail)
    };

    let session_id = first.session_id.unwrap_or(filename_uuid);
    let short = short_id(&session_id);
    let last_activity = tail.last_activity.unwrap_or(mtime);

    Some(SessionEntry {
        session_id,
        short_id: short,
        project_name,
        cwd: first.cwd.map(PathBuf::from),
        transcript_path: path.to_path_buf(),
        started_at: first.started_at,
        last_activity,
        file_size: meta.len(),
        permission_mode: None,
        registry_source: None,
        is_alive: None,
    })
}

/// One-shot scan helper. Equivalent to `SessionCache::new().refresh(dir).entries()`.
/// Used by tests; application code goes through `App::refresh_sessions` so the
/// cache is preserved across refreshes.
#[cfg(test)]
fn scan_sessions(claude_dir: &Path) -> Vec<SessionEntry> {
    let mut cache = SessionCache::new();
    cache.refresh(claude_dir);
    cache.entries()
}

pub fn demo_sessions() -> Vec<SessionEntry> {
    let now = Utc::now();
    let make = |id: &str, project: &str, secs_ago: i64, size: u64| {
        let last_activity = now - chrono::Duration::seconds(secs_ago);
        SessionEntry {
            session_id: id.to_string(),
            short_id: short_id(id),
            project_name: project.to_string(),
            cwd: Some(PathBuf::from(format!("/Users/demo/{project}"))),
            transcript_path: PathBuf::from(format!("/tmp/duru-demo/{id}.jsonl")),
            started_at: Some(last_activity - chrono::Duration::minutes(15)),
            last_activity,
            file_size: size,
            permission_mode: None,
            registry_source: None,
            is_alive: None,
        }
    };
    vec![
        make(
            "676b2e79-2ee5-4a7b-8cd3-2a5034cac2e6",
            "my-webapp",
            12,
            234_000,
        ),
        make("a3f1e2d4-1234-1234-1234-123456789abc", "duru", 120, 187_000),
        make(
            "b9e73dca-aefb-4a83-88f8-4534127e6281",
            "namuldogam",
            240,
            92_000,
        ),
        make(
            "90515568-bd14-4207-a9f5-2bc9d59973e7",
            "chrome-secret",
            1080,
            412_000,
        ),
        make(
            "f3bc49c4-5db3-4e09-8f60-de8c87654f6b",
            "rust-playground",
            7200,
            1_200_000,
        ),
    ]
}

pub fn sort_entries(entries: &mut [SessionEntry], sort: SessionsSort, now: DateTime<Utc>) {
    match sort {
        SessionsSort::LastActivity => {
            entries.sort_by_key(|e| std::cmp::Reverse(e.last_activity));
        }
        SessionsSort::CacheTtl => {
            // Ascending on purpose: sessions closest to expiry come first
            // ("needs attention" order). Do not "fix" to Reverse — that
            // would hide the sessions a user cares most about at the bottom.
            entries.sort_by_key(|e| cache_ttl_remaining_secs(e, now));
        }
        SessionsSort::Project => {
            entries.sort_by_key(|e| e.project_name.to_lowercase());
        }
        SessionsSort::Size => {
            entries.sort_by_key(|e| std::cmp::Reverse(e.file_size));
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
        // 180_000 / 1024 = 175.781… → "175.8K" (binary units match ui::format_size)
        assert_eq!(format_bytes(180_000), "175.8K");
    }

    #[test]
    fn format_bytes_megabytes() {
        // 1_200_000 / (1024 × 1024) = 1.144… → "1.1M"
        assert_eq!(format_bytes(1_200_000), "1.1M");
    }

    #[test]
    fn format_bytes_under_1k() {
        assert_eq!(format_bytes(512), "512B");
    }

    #[test]
    fn format_bytes_matches_ui_format_size_conventions() {
        // Both functions must agree on binary units so the same file shows
        // the same size across Memory and Sessions modes.
        assert_eq!(format_bytes(2048), "2.0K");
        assert_eq!(format_bytes(1_048_576), "1.0M");
    }

    #[test]
    fn middle_truncate_leaves_short_string() {
        assert_eq!(middle_truncate("short", 10), "short");
    }

    #[test]
    fn middle_truncate_respects_max() {
        assert_eq!(middle_truncate("my-very-long-project", 10), "my-v…ject");
    }

    fn make_entry(id: &str, last_activity: DateTime<Utc>) -> SessionEntry {
        SessionEntry {
            session_id: id.to_string(),
            short_id: short_id(id),
            project_name: format!("proj-{id}"),
            cwd: None,
            transcript_path: PathBuf::from(format!("/tmp/{id}.jsonl")),
            started_at: Some(last_activity),
            last_activity,
            file_size: 1000,
            permission_mode: None,
            registry_source: None,
            is_alive: None,
        }
    }

    #[test]
    fn state_at_active_when_recent() {
        let now = Utc::now();
        let entry = make_entry("a", now - chrono::Duration::seconds(30));
        assert_eq!(state_at(&entry, now), State::Active);
    }

    #[test]
    fn state_at_registry_terminated_overrides_mtime_recent() {
        let now = Utc::now();
        let mut entry = make_entry("x", now - chrono::Duration::seconds(30));
        entry.registry_source = Some(RegistrySource::Terminated);
        assert_eq!(state_at(&entry, now), State::Stale);
    }

    #[test]
    fn state_at_registry_alive_uses_mtime() {
        let now = Utc::now();
        let mut entry = make_entry("x", now - chrono::Duration::seconds(30));
        entry.registry_source = Some(RegistrySource::Alive);
        assert_eq!(state_at(&entry, now), State::Active);
    }

    #[test]
    fn state_at_dead_pid_overrides_mtime_recent() {
        let now = Utc::now();
        let mut entry = make_entry("x", now - chrono::Duration::seconds(30));
        entry.is_alive = Some(false);
        assert_eq!(state_at(&entry, now), State::Stale);
    }

    #[test]
    fn state_at_no_registry_falls_back_to_mtime() {
        let now = Utc::now();
        let entry = make_entry("x", now - chrono::Duration::seconds(30));
        assert_eq!(state_at(&entry, now), State::Active);
    }

    #[test]
    fn dedup_same_pid_marks_older_as_terminated() {
        use std::collections::HashMap;
        use std::time::SystemTime;

        let now = Utc::now();
        let pid = 54321;
        let mut entry_old = make_entry("old", now - chrono::Duration::minutes(5));
        entry_old.started_at = Some(now - chrono::Duration::minutes(10));
        entry_old.registry_source = Some(RegistrySource::Alive);
        let mut entry_new = make_entry("new", now - chrono::Duration::seconds(30));
        // started_at 30s apart from entry_old — within the 60s same-process window
        entry_new.started_at = Some(now - chrono::Duration::seconds(10 * 60 - 30));
        entry_new.registry_source = Some(RegistrySource::Alive);

        let mut entries: HashMap<PathBuf, (SessionEntry, SystemTime)> = HashMap::new();
        entries.insert(
            PathBuf::from("/tmp/old.jsonl"),
            (entry_old, SystemTime::UNIX_EPOCH),
        );
        entries.insert(
            PathBuf::from("/tmp/new.jsonl"),
            (entry_new, SystemTime::UNIX_EPOCH),
        );

        let mut pid_lookup: HashMap<PathBuf, u32> = HashMap::new();
        pid_lookup.insert(PathBuf::from("/tmp/old.jsonl"), pid);
        pid_lookup.insert(PathBuf::from("/tmp/new.jsonl"), pid);

        dedup_same_pid(&mut entries, &pid_lookup);

        assert_eq!(
            entries[&PathBuf::from("/tmp/old.jsonl")].0.registry_source,
            Some(RegistrySource::Terminated)
        );
        assert_eq!(
            entries[&PathBuf::from("/tmp/new.jsonl")].0.registry_source,
            Some(RegistrySource::Alive)
        );
    }

    #[test]
    fn dedup_ignores_different_pids() {
        use std::collections::HashMap;
        use std::time::SystemTime;

        let now = Utc::now();
        let mut a = make_entry("a", now);
        a.registry_source = Some(RegistrySource::Alive);
        let mut b = make_entry("b", now);
        b.registry_source = Some(RegistrySource::Alive);

        let mut entries: HashMap<PathBuf, (SessionEntry, SystemTime)> = HashMap::new();
        entries.insert(PathBuf::from("/tmp/a.jsonl"), (a, SystemTime::UNIX_EPOCH));
        entries.insert(PathBuf::from("/tmp/b.jsonl"), (b, SystemTime::UNIX_EPOCH));

        let mut pid_lookup: HashMap<PathBuf, u32> = HashMap::new();
        pid_lookup.insert(PathBuf::from("/tmp/a.jsonl"), 111);
        pid_lookup.insert(PathBuf::from("/tmp/b.jsonl"), 222);

        dedup_same_pid(&mut entries, &pid_lookup);

        for (_, (e, _)) in entries.iter() {
            assert_eq!(e.registry_source, Some(RegistrySource::Alive));
        }
    }

    #[test]
    fn dedup_ignores_same_pid_different_started_at() {
        use std::collections::HashMap;
        use std::time::SystemTime;

        let now = Utc::now();
        let pid = 77777;
        let mut a = make_entry("a", now - chrono::Duration::seconds(30));
        a.started_at = Some(now - chrono::Duration::hours(5));
        a.registry_source = Some(RegistrySource::Alive);
        let mut b = make_entry("b", now - chrono::Duration::seconds(10));
        b.started_at = Some(now - chrono::Duration::seconds(20));
        b.registry_source = Some(RegistrySource::Alive);

        let mut entries: HashMap<PathBuf, (SessionEntry, SystemTime)> = HashMap::new();
        entries.insert(PathBuf::from("/tmp/a.jsonl"), (a, SystemTime::UNIX_EPOCH));
        entries.insert(PathBuf::from("/tmp/b.jsonl"), (b, SystemTime::UNIX_EPOCH));

        let mut pid_lookup: HashMap<PathBuf, u32> = HashMap::new();
        pid_lookup.insert(PathBuf::from("/tmp/a.jsonl"), pid);
        pid_lookup.insert(PathBuf::from("/tmp/b.jsonl"), pid);

        dedup_same_pid(&mut entries, &pid_lookup);

        // started_at 5h apart → treat as different processes → both alive.
        for (_, (e, _)) in entries.iter() {
            assert_eq!(e.registry_source, Some(RegistrySource::Alive));
        }
    }

    #[test]
    fn cache_refresh_merges_registry_into_entry() {
        use crate::registry::REGISTRY_DIR_REL;

        let tmp = tempfile::tempdir().unwrap();
        let proj_dir = tmp.path().join("projects").join("-Users-test-proj");
        fs::create_dir_all(&proj_dir).unwrap();
        let uuid = "zzzz1111-2222-3333-4444-555566667777";
        let jsonl = proj_dir.join(format!("{uuid}.jsonl"));
        fs::write(
            &jsonl,
            r#"{"type":"user","timestamp":"2026-04-20T00:00:00Z"}"#,
        )
        .unwrap();

        let reg_dir = tmp.path().join(REGISTRY_DIR_REL);
        fs::create_dir_all(&reg_dir).unwrap();
        fs::write(
            reg_dir.join(format!("{uuid}.json")),
            format!(
                r#"{{
                    "schema_version": 1,
                    "session_id": "{uuid}",
                    "cwd": "/tmp/test-proj",
                    "transcript_path": "{}",
                    "started_at": "2026-04-20T00:00:00Z",
                    "last_heartbeat": "2026-04-20T00:00:30Z",
                    "permission_mode": "auto",
                    "terminated": false
                }}"#,
                jsonl.to_string_lossy()
            ),
        )
        .unwrap();

        let mut cache = SessionCache::new();
        cache.refresh(tmp.path());

        let entries = cache.entries();
        let entry = entries
            .iter()
            .find(|e| e.session_id == uuid)
            .expect("session entry should exist");
        assert_eq!(entry.permission_mode.as_deref(), Some("auto"));
        assert_eq!(entry.registry_source, Some(RegistrySource::Alive));
    }

    #[test]
    fn state_at_stale_when_old() {
        let now = Utc::now();
        let entry = make_entry("c", now - chrono::Duration::hours(2));
        assert_eq!(state_at(&entry, now), State::Stale);
    }

    #[test]
    fn state_at_active_just_under_300s() {
        let now = Utc::now();
        let entry = make_entry("e", now - chrono::Duration::seconds(299));
        assert_eq!(state_at(&entry, now), State::Active);
    }

    #[test]
    fn state_at_stale_at_exactly_300s() {
        let now = Utc::now();
        let entry = make_entry("f", now - chrono::Duration::seconds(300));
        assert_eq!(state_at(&entry, now), State::Stale);
    }

    #[test]
    fn sort_by_last_activity_desc() {
        let now = Utc::now();
        let mut entries = vec![
            make_entry("old", now - chrono::Duration::minutes(10)),
            make_entry("new", now - chrono::Duration::seconds(5)),
            make_entry("mid", now - chrono::Duration::minutes(2)),
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
            make_entry("fresh", now - chrono::Duration::seconds(10)),
            make_entry("expiring", now - chrono::Duration::seconds(270)),
            make_entry("middle", now - chrono::Duration::seconds(120)),
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
            make_entry("c", now),
            make_entry("a", now),
            make_entry("b", now),
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

    use std::fs;

    #[test]
    fn demo_sessions_returns_five_entries() {
        let demos = demo_sessions();
        assert_eq!(demos.len(), 5);
        assert!(demos.iter().any(|e| e.project_name == "my-webapp"));
    }

    #[test]
    fn scan_empty_claude_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let entries = scan_sessions(tmp.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_skips_skill_injections_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let encoded = tmp.path().join("projects").join("-Users-fake-realproj");
        fs::create_dir_all(&encoded).unwrap();
        fs::write(encoded.join("skill-injections.jsonl"), "").unwrap();
        let entries = scan_sessions(tmp.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn cache_refresh_on_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = SessionCache::new();
        cache.refresh(tmp.path());
        assert!(cache.entries().is_empty());
        cache.refresh(tmp.path());
        assert!(cache.entries().is_empty());
    }

    #[test]
    fn cache_removes_deleted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("projects").join("-x");
        fs::create_dir_all(&proj).unwrap();
        let jsonl = proj.join("zzzz1234-zzzz-zzzz-zzzz-zzzzzzzzzzzz.jsonl");
        fs::write(
            &jsonl,
            r#"{"type":"user","timestamp":"2026-04-19T06:00:00Z"}"#,
        )
        .unwrap();

        let mut cache = SessionCache::new();
        cache.refresh(tmp.path());
        let initial_count = cache.entries().len();
        fs::remove_file(&jsonl).unwrap();
        cache.refresh(tmp.path());
        assert!(
            cache.entries().len() < initial_count
                || !cache.entries().iter().any(|e| e.transcript_path == jsonl)
        );
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
    }

    #[test]
    fn parse_tail_handles_empty_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let parsed = parse_tail(file.path()).unwrap();
        assert!(parsed.last_activity.is_none());
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
    fn parse_tail_reads_tail_of_large_file() {
        // Regression test for the double-BufReader bug: when file exceeds
        // TAIL_CHUNK_BYTES (8 KiB), the old impl read nothing because the
        // first BufReader drained the file to EOF.
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for _ in 0..200 {
            writeln!(f, "{}", "x".repeat(46)).unwrap();
        }
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"2026-04-19T08:00:00Z","sessionId":"x"}}"#
        )
        .unwrap();
        let parsed = parse_tail(f.path()).unwrap();
        assert!(
            parsed.last_activity.is_some(),
            "last_activity must be parsed from the tail even for large files"
        );
    }

    #[test]
    fn parse_first_record_extracts_session_id() {
        let content = r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"676b2e79-2ee5-4a7b-8cd3-2a5034cac2e6"}"#;
        let parsed = parse_first_record(content.as_bytes());
        assert_eq!(
            parsed.session_id.as_deref(),
            Some("676b2e79-2ee5-4a7b-8cd3-2a5034cac2e6")
        );
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
    }

    #[test]
    fn parse_first_record_extracts_timestamp_and_cwd_from_user_record() {
        let content = concat!(
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
    }

    #[test]
    fn parse_first_record_handles_empty_input() {
        let parsed = parse_first_record(b"" as &[u8]);
        assert!(parsed.session_id.is_none());
    }

    #[test]
    fn parse_session_at_skips_parse_for_stale_file() {
        // When mtime is older than LAZY_PARSE_CUTOFF_SECS the function must
        // NOT open the file, so started_at and cwd stay None even though the
        // jsonl has parseable content. session_id must fall back to filename.
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("projects").join("-Users-old-project");
        fs::create_dir_all(&proj).unwrap();
        let uuid = "aabbccdd-0000-0000-0000-000000000000";
        let path = proj.join(format!("{uuid}.jsonl"));
        fs::write(
            &path,
            r#"{"type":"user","timestamp":"2024-01-01T00:00:00Z","cwd":"/not-loaded","sessionId":"from-content"}"#,
        )
        .unwrap();

        let mtime: DateTime<Utc> = DateTime::from(path.metadata().unwrap().modified().unwrap());
        let now = mtime + chrono::Duration::hours(25);
        let entry = parse_session_at(&path, now).unwrap();

        assert!(
            entry.started_at.is_none(),
            "lazy path must not parse started_at"
        );
        assert!(entry.cwd.is_none(), "lazy path must not parse cwd");
        assert_eq!(
            entry.session_id, uuid,
            "lazy path falls back to filename uuid"
        );
    }

    #[test]
    fn parse_session_at_parses_when_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("projects").join("-Users-fresh-project");
        fs::create_dir_all(&proj).unwrap();
        let uuid = "11112222-3333-4444-5555-666677778888";
        let path = proj.join(format!("{uuid}.jsonl"));
        fs::write(
            &path,
            r#"{"type":"user","timestamp":"2026-04-19T00:00:00Z","cwd":"/fresh","sessionId":"from-content"}"#,
        )
        .unwrap();

        let mtime: DateTime<Utc> = DateTime::from(path.metadata().unwrap().modified().unwrap());
        let now = mtime + chrono::Duration::minutes(30);
        let entry = parse_session_at(&path, now).unwrap();

        assert_eq!(
            entry.cwd.as_deref().and_then(|p| p.to_str()),
            Some("/fresh")
        );
        assert!(entry.started_at.is_some());
    }

    #[test]
    fn sort_by_size_desc() {
        let now = Utc::now();
        let mut entries = vec![
            make_entry("small", now),
            make_entry("big", now),
            make_entry("mid", now),
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
