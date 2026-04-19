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
