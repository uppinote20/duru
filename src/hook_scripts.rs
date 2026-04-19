//! Embedded hook script contents. Written to disk by `duru hooks install`.
//!
//! Invariants:
//!  - Always `exit 0` — never block Claude Code.
//!  - Silent on stderr — avoid terminal corruption.
//!  - Atomic write via mktemp + mv — no partial files visible.

pub const SESSION_START_SH: &str = include_str!("_hook_scripts/session-start.sh");
pub const USER_PROMPT_SUBMIT_SH: &str = include_str!("_hook_scripts/user-prompt-submit.sh");
pub const PRE_TOOL_USE_SH: &str = include_str!("_hook_scripts/user-prompt-submit.sh");
pub const POST_TOOL_USE_SH: &str = include_str!("_hook_scripts/user-prompt-submit.sh");
pub const STOP_SH: &str = include_str!("_hook_scripts/user-prompt-submit.sh");
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
