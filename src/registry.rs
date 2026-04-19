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
