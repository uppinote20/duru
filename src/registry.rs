use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Path suffix under the claude_dir for registry files.
pub const REGISTRY_DIR_REL: &str = "duru/registry";

/// Terminated entries older than this are pruned by `cleanup_expired`.
pub const TERMINATED_TTL_SECS: i64 = 7 * 86_400;

/// Currently supported schema for per-session registry JSON.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryEntry {
    pub schema_version: u32,
    pub session_id: String,
    #[serde(default)]
    pub pid: Option<u32>,
    pub cwd: PathBuf,
    pub transcript_path: PathBuf,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub source: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub terminated: bool,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrySource {
    Alive,
    Terminated,
}

#[derive(Debug, Default, Clone)]
pub struct Registry {
    by_session_id: HashMap<String, RegistryEntry>,
    by_transcript_path: HashMap<PathBuf, String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.by_session_id.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.by_session_id.is_empty()
    }

    #[allow(dead_code)]
    pub fn entries(&self) -> impl Iterator<Item = &RegistryEntry> {
        self.by_session_id.values()
    }

    #[allow(dead_code)]
    pub fn get_by_session_id(&self, sid: &str) -> Option<&RegistryEntry> {
        self.by_session_id.get(sid)
    }

    pub fn get_by_transcript_path(&self, path: &Path) -> Option<&RegistryEntry> {
        self.by_transcript_path
            .get(path)
            .and_then(|sid| self.by_session_id.get(sid))
    }

    /// Deletes terminated entries whose `ended_at` is older than
    /// `TERMINATED_TTL_SECS`. Alive entries are always kept. Delete failures
    /// (permission, race) are silently skipped; caller retries next refresh.
    pub fn cleanup_expired(claude_dir: &Path, now: DateTime<Utc>) {
        let dir = claude_dir.join(REGISTRY_DIR_REL);
        let Ok(read_dir) = fs::read_dir(&dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let Ok(parsed) = serde_json::from_slice::<RegistryEntry>(&bytes) else {
                continue;
            };
            if !parsed.terminated {
                continue;
            }
            let Some(ended) = parsed.ended_at else {
                continue;
            };
            let age = (now - ended).num_seconds();
            if age > TERMINATED_TTL_SECS {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// Loads every well-formed `*.json` file under `<claude_dir>/duru/registry/`.
    /// Corrupt files, files with unknown schema_version, or files that fail
    /// deserialization are silently skipped — duru falls back to MVP1
    /// behavior for the corresponding session.
    pub fn load_all(claude_dir: &Path) -> Self {
        let dir = claude_dir.join(REGISTRY_DIR_REL);
        let mut out = Self::new();
        let Ok(read_dir) = fs::read_dir(&dir) else {
            return out;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let Ok(parsed) = serde_json::from_slice::<RegistryEntry>(&bytes) else {
                continue;
            };
            if parsed.schema_version != CURRENT_SCHEMA_VERSION {
                continue;
            }
            out.by_transcript_path
                .insert(parsed.transcript_path.clone(), parsed.session_id.clone());
            out.by_session_id.insert(parsed.session_id.clone(), parsed);
        }
        out
    }
}

/// Derives the user-visible state for an entry.
/// - Terminated flag from the registry wins unconditionally.
/// - Dead pid → Terminated.
/// - Missing pid → Alive (cannot prove death).
/// - Otherwise Alive.
pub fn classify(entry: &RegistryEntry) -> RegistrySource {
    if entry.terminated {
        return RegistrySource::Terminated;
    }
    if let Some(pid) = entry.pid
        && !is_pid_alive(pid)
    {
        return RegistrySource::Terminated;
    }
    RegistrySource::Alive
}

/// Returns true if a process with `pid` currently exists.
/// On Unix uses `kill(pid, 0)` — zero signal means "check only, don't deliver".
/// `EPERM` (process exists but not ours to signal) also counts as alive.
/// On Windows we cannot check from bash hooks (the captured pid is the bash
/// shell's parent, which does not map cleanly); return `true` so the registry
/// entry falls back to the mtime-based liveness instead.
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let pid_t = pid as libc::pid_t;
    let rc = unsafe { libc::kill(pid_t, 0) };
    if rc == 0 {
        return true;
    }
    let err = std::io::Error::last_os_error().raw_os_error();
    err == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub fn is_pid_alive(_pid: u32) -> bool {
    // Conservative default: we cannot prove death on this platform without
    // extra APIs we have chosen not to depend on. Callers fall through to
    // the mtime-based liveness check, which is the MVP1 behavior.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn is_pid_alive_returns_true_for_current_process() {
        let own_pid = std::process::id();
        assert!(is_pid_alive(own_pid));
    }

    #[cfg(unix)]
    #[test]
    fn is_pid_alive_returns_false_for_impossible_pid() {
        assert!(!is_pid_alive(4_000_000));
    }

    #[cfg(unix)]
    #[test]
    fn is_pid_alive_for_zero_returns_false() {
        assert!(!is_pid_alive(0));
    }

    #[cfg(not(unix))]
    #[test]
    fn is_pid_alive_returns_true_on_non_unix() {
        // Conservative default; documented in is_pid_alive doc comment.
        assert!(is_pid_alive(std::process::id()));
        assert!(is_pid_alive(4_000_000));
    }

    use std::io::Write;

    fn write_entry(dir: &std::path::Path, session_id: &str, contents: &str) -> std::path::PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{session_id}.json"));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn load_all_empty_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::load_all(tmp.path());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn load_all_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::load_all(&tmp.path().join("does-not-exist"));
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn load_all_parses_valid_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        write_entry(
            &dir,
            "abc123",
            r#"{
                "schema_version": 1,
                "session_id": "abc123",
                "pid": 12345,
                "cwd": "/tmp/proj",
                "transcript_path": "/tmp/proj/abc123.jsonl",
                "started_at": "2026-04-20T00:00:00Z",
                "last_heartbeat": "2026-04-20T00:05:00Z",
                "permission_mode": "auto",
                "terminated": false
            }"#,
        );
        let reg = Registry::load_all(tmp.path());
        let entry = reg.get_by_session_id("abc123").unwrap();
        assert_eq!(entry.session_id, "abc123");
        assert_eq!(entry.pid, Some(12345));
        assert_eq!(entry.permission_mode.as_deref(), Some("auto"));
    }

    #[test]
    fn load_all_skips_corrupt_json() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        write_entry(&dir, "bad", "{ not json");
        write_entry(
            &dir,
            "good",
            r#"{
                "schema_version": 1,
                "session_id": "good",
                "cwd": "/tmp",
                "transcript_path": "/tmp/good.jsonl",
                "started_at": "2026-04-20T00:00:00Z",
                "last_heartbeat": "2026-04-20T00:00:00Z",
                "terminated": false
            }"#,
        );
        let reg = Registry::load_all(tmp.path());
        assert!(reg.get_by_session_id("good").is_some());
        assert!(reg.get_by_session_id("bad").is_none());
    }

    #[test]
    fn load_all_skips_wrong_schema_version() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        write_entry(
            &dir,
            "future",
            r#"{
                "schema_version": 99,
                "session_id": "future",
                "cwd": "/tmp",
                "transcript_path": "/tmp/f.jsonl",
                "started_at": "2026-04-20T00:00:00Z",
                "last_heartbeat": "2026-04-20T00:00:00Z",
                "terminated": false
            }"#,
        );
        let reg = Registry::load_all(tmp.path());
        assert!(reg.get_by_session_id("future").is_none());
    }

    fn write_entry_with_ended(
        dir: &std::path::Path,
        session_id: &str,
        terminated: bool,
        ended_at: Option<DateTime<Utc>>,
    ) -> std::path::PathBuf {
        let ended_str = match ended_at {
            Some(t) => format!(r#","ended_at":"{}""#, t.to_rfc3339()),
            None => String::new(),
        };
        let content = format!(
            r#"{{
                "schema_version": 1,
                "session_id": "{sid}",
                "cwd": "/tmp",
                "transcript_path": "/tmp/{sid}.jsonl",
                "started_at": "2026-04-20T00:00:00Z",
                "last_heartbeat": "2026-04-20T00:00:00Z",
                "terminated": {term}{ended}
            }}"#,
            sid = session_id,
            term = terminated,
            ended = ended_str
        );
        write_entry(dir, session_id, &content)
    }

    #[test]
    fn cleanup_expired_removes_old_terminated() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        let long_ago = Utc::now() - chrono::Duration::days(10);
        let path = write_entry_with_ended(&dir, "old", true, Some(long_ago));

        Registry::cleanup_expired(tmp.path(), Utc::now());
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_preserves_recent_terminated() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        let recent = Utc::now() - chrono::Duration::days(3);
        let path = write_entry_with_ended(&dir, "recent", true, Some(recent));

        Registry::cleanup_expired(tmp.path(), Utc::now());
        assert!(path.exists());
    }

    #[test]
    fn cleanup_preserves_alive_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        let path = write_entry_with_ended(&dir, "alive", false, None);

        Registry::cleanup_expired(tmp.path(), Utc::now());
        assert!(path.exists());
    }

    #[test]
    fn cleanup_missing_dir_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        Registry::cleanup_expired(tmp.path(), Utc::now());
    }

    fn sample_entry(last_hb_secs_ago: i64, terminated: bool, pid: Option<u32>) -> RegistryEntry {
        let now = Utc::now();
        RegistryEntry {
            schema_version: 1,
            session_id: "test".to_string(),
            pid,
            cwd: PathBuf::from("/tmp"),
            transcript_path: PathBuf::from("/tmp/test.jsonl"),
            started_at: now - chrono::Duration::hours(1),
            source: Some("startup".to_string()),
            last_heartbeat: now - chrono::Duration::seconds(last_hb_secs_ago),
            permission_mode: Some("auto".to_string()),
            terminated,
            ended_at: if terminated { Some(now) } else { None },
            end_reason: if terminated {
                Some("other".to_string())
            } else {
                None
            },
        }
    }

    #[test]
    fn classify_alive_when_not_terminated_and_pid_alive() {
        let entry = sample_entry(30, false, Some(std::process::id()));
        assert_eq!(classify(&entry), RegistrySource::Alive);
    }

    #[test]
    fn classify_terminated_flag_true() {
        let entry = sample_entry(30, true, Some(std::process::id()));
        assert_eq!(classify(&entry), RegistrySource::Terminated);
    }

    #[cfg(unix)]
    #[test]
    fn classify_dead_pid_is_terminated() {
        let entry = sample_entry(30, false, Some(4_000_000));
        assert_eq!(classify(&entry), RegistrySource::Terminated);
    }

    #[cfg(not(unix))]
    #[test]
    fn classify_treats_any_pid_as_alive_on_non_unix() {
        // Documented behavior: is_pid_alive is conservative on non-Unix,
        // so classify cannot contribute a Terminated verdict from pid alone.
        let entry = sample_entry(30, false, Some(4_000_000));
        assert_eq!(classify(&entry), RegistrySource::Alive);
    }

    #[test]
    fn classify_no_pid_and_not_terminated_is_alive() {
        let entry = sample_entry(30, false, None);
        assert_eq!(classify(&entry), RegistrySource::Alive);
    }

    #[test]
    fn load_all_finds_by_transcript_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(REGISTRY_DIR_REL);
        write_entry(
            &dir,
            "xid",
            r#"{
                "schema_version": 1,
                "session_id": "xid",
                "cwd": "/tmp/p",
                "transcript_path": "/tmp/p/xid.jsonl",
                "started_at": "2026-04-20T00:00:00Z",
                "last_heartbeat": "2026-04-20T00:00:00Z",
                "terminated": false
            }"#,
        );
        let reg = Registry::load_all(tmp.path());
        let entry = reg
            .get_by_transcript_path(std::path::Path::new("/tmp/p/xid.jsonl"))
            .unwrap();
        assert_eq!(entry.session_id, "xid");
    }
}
