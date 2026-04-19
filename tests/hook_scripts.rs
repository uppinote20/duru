//! Integration tests for the embedded hook shell scripts. Each test spawns
//! `bash` with one of the scripts and a synthetic JSON payload on stdin, then
//! asserts the resulting registry file shape.

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

fn jq_present() -> bool {
    Command::new("jq")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn install_script(tmp: &tempfile::TempDir, filename: &str, content: &str) -> std::path::PathBuf {
    let path = tmp.path().join(filename);
    std::fs::write(&path, content).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

fn run_hook(script_path: &std::path::Path, stdin_json: &str, home: &std::path::Path) {
    let mut child = Command::new("bash")
        .arg(script_path)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_json.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "hook exit non-zero: stderr = {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn fake_home() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude/duru/registry")).unwrap();
    tmp
}

const SESSION_START: &str = include_str!("../src/_hook_scripts/session-start.sh");
const USER_PROMPT: &str = include_str!("../src/_hook_scripts/user-prompt-submit.sh");
const SESSION_END: &str = include_str!("../src/_hook_scripts/session-end.sh");

#[test]
fn session_start_creates_registry_entry() {
    if !jq_present() {
        eprintln!("jq not found, skipping");
        return;
    }
    let home = fake_home();
    let scripts_dir = tempfile::tempdir().unwrap();
    let script = install_script(&scripts_dir, "session-start.sh", SESSION_START);

    let payload = r#"{
        "session_id": "test-session",
        "cwd": "/tmp/work",
        "transcript_path": "/tmp/work/t.jsonl",
        "source": "startup",
        "permission_mode": "",
        "hook_event_name": "SessionStart"
    }"#;
    run_hook(&script, payload, home.path());

    let path = home.path().join(".claude/duru/registry/test-session.json");
    assert!(path.exists(), "registry file should exist");
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["session_id"], "test-session");
    assert_eq!(parsed["terminated"], false);
    assert_eq!(parsed["cwd"], "/tmp/work");
}

#[test]
fn user_prompt_updates_heartbeat_preserves_other_fields() {
    if !jq_present() {
        return;
    }
    let home = fake_home();
    let scripts_dir = tempfile::tempdir().unwrap();
    let start_sh = install_script(&scripts_dir, "session-start.sh", SESSION_START);
    let prompt_sh = install_script(&scripts_dir, "user-prompt-submit.sh", USER_PROMPT);

    let start_payload = r#"{
        "session_id": "heartbeat-test",
        "cwd": "/tmp",
        "transcript_path": "/tmp/h.jsonl",
        "source": "startup"
    }"#;
    run_hook(&start_sh, start_payload, home.path());
    let path = home
        .path()
        .join(".claude/duru/registry/heartbeat-test.json");
    let before: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let started = before["started_at"].clone();
    let pid = before["pid"].clone();

    std::thread::sleep(std::time::Duration::from_millis(1100));

    let prompt_payload = r#"{
        "session_id": "heartbeat-test",
        "cwd": "/tmp",
        "transcript_path": "/tmp/h.jsonl",
        "permission_mode": "auto",
        "prompt": "hi"
    }"#;
    run_hook(&prompt_sh, prompt_payload, home.path());

    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(after["started_at"], started, "started_at must not change");
    assert_eq!(after["pid"], pid, "pid must not change");
    assert_eq!(after["permission_mode"], "auto");
    assert_ne!(after["last_heartbeat"], started);
}

#[test]
fn session_end_marks_terminated() {
    if !jq_present() {
        return;
    }
    let home = fake_home();
    let scripts_dir = tempfile::tempdir().unwrap();
    let start_sh = install_script(&scripts_dir, "session-start.sh", SESSION_START);
    let end_sh = install_script(&scripts_dir, "session-end.sh", SESSION_END);

    let start_payload = r#"{
        "session_id": "end-test",
        "cwd": "/tmp",
        "transcript_path": "/tmp/e.jsonl",
        "source": "startup"
    }"#;
    run_hook(&start_sh, start_payload, home.path());

    let end_payload = r#"{"session_id":"end-test","reason":"exit"}"#;
    run_hook(&end_sh, end_payload, home.path());

    let path = home.path().join(".claude/duru/registry/end-test.json");
    let parsed: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(parsed["terminated"], true);
    assert_eq!(parsed["end_reason"], "exit");
}

#[test]
fn hook_exit_zero_on_malformed_stdin() {
    if !jq_present() {
        return;
    }
    let home = fake_home();
    let scripts_dir = tempfile::tempdir().unwrap();
    let start_sh = install_script(&scripts_dir, "session-start.sh", SESSION_START);

    // Not JSON at all. Hook must still exit 0 (run_hook's assert).
    run_hook(&start_sh, "not json", home.path());
}
