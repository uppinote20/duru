use std::path::PathBuf;

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

/// Returns true if a process with `pid` currently exists.
/// Uses `kill(pid, 0)` — zero signal means "check only, don't deliver".
/// `EPERM` (process exists but not ours to signal) also counts as alive.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pid_alive_returns_true_for_current_process() {
        let own_pid = std::process::id();
        assert!(is_pid_alive(own_pid));
    }

    #[test]
    fn is_pid_alive_returns_false_for_impossible_pid() {
        assert!(!is_pid_alive(4_000_000));
    }

    #[test]
    fn is_pid_alive_for_zero_returns_false() {
        assert!(!is_pid_alive(0));
    }
}
