//! Embedded hook script contents. Written to disk by `duru hooks install`.
//!
//! Invariants:
//!  - Always `exit 0` — never block Claude Code.
//!  - Silent on stderr — avoid terminal corruption.
//!  - Atomic write via mktemp + mv — no partial files visible.
//!
//! @handbook 8.1-embedded-shell-scripts

pub const SESSION_START_SH: &str = include_str!("_hook_scripts/session-start.sh");

/// Heartbeat script shared by UserPromptSubmit, PreToolUse, PostToolUse, and
/// Stop. All four events just touch `last_heartbeat` (and optionally refresh
/// `permission_mode`). Intentional aliasing — if you add a distinct
/// per-event script, create a new `_hook_scripts/<name>.sh` file and swap the
/// constant below.
pub const USER_PROMPT_SUBMIT_SH: &str = include_str!("_hook_scripts/user-prompt-submit.sh");
pub const PRE_TOOL_USE_SH: &str = USER_PROMPT_SUBMIT_SH;
pub const POST_TOOL_USE_SH: &str = USER_PROMPT_SUBMIT_SH;
pub const STOP_SH: &str = USER_PROMPT_SUBMIT_SH;

pub const SESSION_END_SH: &str = include_str!("_hook_scripts/session-end.sh");

/// Returns (filename, contents) for every hook this binary ships.
pub fn all() -> Vec<(&'static str, &'static str)> {
    vec![
        ("session-start.sh", SESSION_START_SH),
        ("user-prompt-submit.sh", USER_PROMPT_SUBMIT_SH),
        ("pre-tool-use.sh", PRE_TOOL_USE_SH),
        ("post-tool-use.sh", POST_TOOL_USE_SH),
        ("stop.sh", STOP_SH),
        ("session-end.sh", SESSION_END_SH),
    ]
}
