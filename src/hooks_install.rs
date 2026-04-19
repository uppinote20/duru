use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::hook_scripts;

pub struct InstallOpts {
    pub dry_run: bool,
    pub yes: bool,
    pub star: bool,
    pub force_star_prompt: bool,
}

pub struct UninstallOpts {
    pub dry_run: bool,
    pub force: bool,
}

pub const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];

fn script_filename(event: &str) -> &'static str {
    match event {
        "SessionStart" => "session-start.sh",
        "UserPromptSubmit" => "user-prompt-submit.sh",
        "PreToolUse" => "pre-tool-use.sh",
        "PostToolUse" => "post-tool-use.sh",
        "Stop" => "stop.sh",
        "SessionEnd" => "session-end.sh",
        _ => unreachable!("unknown event: {event}"),
    }
}

pub fn duru_dir(home: &Path) -> PathBuf {
    home.join(".claude/duru")
}

pub fn hooks_dir(home: &Path) -> PathBuf {
    duru_dir(home).join("hooks")
}

pub fn registry_dir(home: &Path) -> PathBuf {
    duru_dir(home).join("registry")
}

pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude/settings.json")
}

pub fn check_jq_available() -> bool {
    Command::new("jq")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn install(home: &Path, opts: &InstallOpts) -> std::io::Result<()> {
    if !check_jq_available() {
        eprintln!("error: `jq` is required but not found on PATH.");
        eprintln!("  macOS: brew install jq");
        eprintln!("  Debian/Ubuntu: apt-get install jq");
        return Err(std::io::Error::other("jq missing"));
    }

    if opts.dry_run {
        println!("[dry-run] would create {}", hooks_dir(home).display());
        println!("[dry-run] would create {}", registry_dir(home).display());
        for (name, _) in hook_scripts::all() {
            println!("[dry-run] would write {}/{name}", hooks_dir(home).display());
        }
        println!(
            "[dry-run] would merge 6 hook entries into {}",
            settings_path(home).display()
        );
        return Ok(());
    }

    fs::create_dir_all(hooks_dir(home))?;
    fs::create_dir_all(registry_dir(home))?;
    for (name, content) in hook_scripts::all() {
        let path = hooks_dir(home).join(name);
        fs::write(&path, content)?;
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }

    merge_settings(home)?;

    println!("✓ Hooks installed.");
    println!("  6 events registered in {}", settings_path(home).display());
    println!("  Registry at {}", registry_dir(home).display());

    maybe_star_prompt(home, opts)?;
    Ok(())
}

fn merge_settings(home: &Path) -> std::io::Result<()> {
    let settings = settings_path(home);
    let hooks_dir_p = hooks_dir(home);

    let backup_name = format!(
        "settings.json.duru.bak.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    if settings.exists() {
        fs::copy(&settings, home.join(".claude").join(&backup_name))?;
    }

    let mut jq_expr = String::from(". as $orig | $orig | .hooks = (( .hooks // {} )");
    for event in EVENTS {
        let script_path = hooks_dir_p
            .join(script_filename(event))
            .to_string_lossy()
            .to_string();
        let command_str = format!("bash {script_path}");
        jq_expr.push_str(&format!(
            " | .[\"{event}\"] = ((.[\"{event}\"] // []) + \
             [{{\"_duru\": true, \"hooks\": [{{\"type\": \"command\", \
             \"command\": \"{command_str}\"}}]}}])"
        ));
    }
    jq_expr.push(')');

    let input = if settings.exists() {
        fs::read_to_string(&settings)?
    } else {
        "{}".to_string()
    };

    let mut child = Command::new("jq")
        .arg(&jq_expr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())?;
    let result = child.wait_with_output()?;

    if !result.status.success() {
        return Err(std::io::Error::other("jq merge failed"));
    }
    if serde_json::from_slice::<serde_json::Value>(&result.stdout).is_err() {
        return Err(std::io::Error::other("merged settings invalid JSON"));
    }

    let tmp = settings.with_extension("json.duru.tmp");
    fs::write(&tmp, &result.stdout)?;
    fs::rename(&tmp, &settings)?;

    Ok(())
}

fn maybe_star_prompt(_home: &Path, _opts: &InstallOpts) -> std::io::Result<()> {
    // Implemented in T16.
    Ok(())
}

pub struct StatusReport {
    pub installed: bool,
    pub events_present: Vec<String>,
    pub events_missing: Vec<String>,
    pub registry_alive: usize,
    pub registry_terminated: usize,
}

pub fn status(home: &Path) -> std::io::Result<StatusReport> {
    let settings = settings_path(home);
    let mut events_present = Vec::new();
    let mut events_missing: Vec<String> = Vec::new();

    if settings.exists() {
        let raw = fs::read_to_string(&settings)?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::other(format!("settings.json invalid: {e}")))?;
        let hooks = &parsed["hooks"];
        for event in EVENTS {
            let has_duru = hooks[event]
                .as_array()
                .map(|arr| {
                    arr.iter().any(|e| {
                        e["_duru"] == true
                            || e["hooks"]
                                .as_array()
                                .map(|hs| {
                                    hs.iter().any(|h| {
                                        h["command"]
                                            .as_str()
                                            .map(|c| c.contains(".claude/duru/hooks/"))
                                            .unwrap_or(false)
                                    })
                                })
                                .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if has_duru {
                events_present.push((*event).to_string());
            } else {
                events_missing.push((*event).to_string());
            }
        }
    } else {
        events_missing = EVENTS.iter().map(|s| (*s).to_string()).collect();
    }

    let mut alive = 0usize;
    let mut terminated = 0usize;
    if let Ok(read_dir) = fs::read_dir(registry_dir(home)) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = fs::read(&path)
                && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                if parsed["terminated"] == true {
                    terminated += 1;
                } else {
                    alive += 1;
                }
            }
        }
    }

    Ok(StatusReport {
        installed: events_missing.is_empty(),
        events_present,
        events_missing,
        registry_alive: alive,
        registry_terminated: terminated,
    })
}

pub fn print_status(report: &StatusReport) {
    println!(
        "Hooks installed: {}",
        if report.installed { "yes" } else { "no" }
    );
    for event in EVENTS {
        let marker = if report.events_present.contains(&(*event).to_string()) {
            "✓"
        } else {
            "✗"
        };
        println!("  {event:18} {marker}");
    }
    println!();
    println!(
        "Registry entries: {} alive, {} terminated",
        report.registry_alive, report.registry_terminated
    );
}

pub fn uninstall(home: &Path, opts: &UninstallOpts) -> std::io::Result<()> {
    if !check_jq_available() {
        eprintln!("error: `jq` is required but not found on PATH.");
        return Err(std::io::Error::other("jq missing"));
    }

    if opts.dry_run {
        println!(
            "[dry-run] would remove duru hook entries from {}",
            settings_path(home).display()
        );
        if opts.force {
            println!("[dry-run] would remove {}", duru_dir(home).display());
        }
        return Ok(());
    }

    let settings = settings_path(home);
    if !settings.exists() {
        return Ok(());
    }

    let backup_name = format!(
        "settings.json.duru.bak.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    fs::copy(&settings, home.join(".claude").join(&backup_name))?;

    let input = fs::read_to_string(&settings)?;
    let filter_expr = r#"
        if .hooks == null then . else
          .hooks |= with_entries(
            .value |= map(
              select(
                (._duru != true) and
                ((.hooks // []) | all((.command // "") | contains(".claude/duru/hooks/") | not))
              )
            )
          )
        end
    "#;

    let mut child = Command::new("jq")
        .arg(filter_expr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child.stdin.as_mut().unwrap().write_all(input.as_bytes())?;
    let result = child.wait_with_output()?;
    if !result.status.success() {
        return Err(std::io::Error::other("jq filter failed"));
    }
    if serde_json::from_slice::<serde_json::Value>(&result.stdout).is_err() {
        return Err(std::io::Error::other("filtered settings invalid JSON"));
    }

    let tmp = settings.with_extension("json.duru.tmp");
    fs::write(&tmp, &result.stdout)?;
    fs::rename(&tmp, &settings)?;

    if opts.force {
        let _ = fs::remove_dir_all(duru_dir(home));
    }

    println!("✓ Hooks uninstalled.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        tmp
    }

    fn opts_install_silent() -> InstallOpts {
        InstallOpts {
            dry_run: false,
            yes: true,
            star: false,
            force_star_prompt: false,
        }
    }

    #[test]
    fn install_creates_hooks_and_registry_dirs() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        assert!(hooks_dir(home.path()).is_dir());
        assert!(registry_dir(home.path()).is_dir());
    }

    #[test]
    fn install_writes_six_hook_scripts() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        for (name, _) in hook_scripts::all() {
            let p = hooks_dir(home.path()).join(name);
            assert!(p.is_file(), "{} missing", p.display());
        }
    }

    #[test]
    fn install_creates_valid_json_settings_from_scratch() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        let s = fs::read_to_string(settings_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        for event in EVENTS {
            assert!(parsed["hooks"][event].is_array(), "{event} not array");
        }
    }

    #[test]
    fn install_preserves_existing_non_duru_hooks_and_env() {
        let home = fake_home();
        let existing = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash /some/other/hook.sh"}]}
                ]
            },
            "env": {"FOO": "bar"}
        }"#;
        fs::write(settings_path(home.path()), existing).unwrap();
        install(home.path(), &opts_install_silent()).unwrap();

        let s = fs::read_to_string(settings_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        let pre = parsed["hooks"]["PreToolUse"].as_array().unwrap();
        let has_other = pre
            .iter()
            .any(|e| e["hooks"][0]["command"].as_str() == Some("bash /some/other/hook.sh"));
        assert!(has_other, "existing non-duru hook must be preserved");
        assert_eq!(parsed["env"]["FOO"].as_str(), Some("bar"));
    }

    #[test]
    fn install_marks_entries_with_duru_flag() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        let s = fs::read_to_string(settings_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        for event in EVENTS {
            let entries = parsed["hooks"][event].as_array().unwrap();
            assert!(
                entries.iter().any(|e| e["_duru"] == true),
                "{event} has no duru-marked entry"
            );
        }
    }

    #[test]
    fn install_creates_backup_when_settings_exists() {
        let home = fake_home();
        fs::write(settings_path(home.path()), r#"{"hooks":{}}"#).unwrap();
        install(home.path(), &opts_install_silent()).unwrap();
        let claude_dir = home.path().join(".claude");
        let backups: Vec<_> = fs::read_dir(&claude_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.json.duru.bak.")
            })
            .collect();
        assert_eq!(backups.len(), 1);
    }

    fn opts_uninstall_silent() -> UninstallOpts {
        UninstallOpts {
            dry_run: false,
            force: false,
        }
    }

    #[test]
    fn uninstall_removes_duru_entries() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        uninstall(home.path(), &opts_uninstall_silent()).unwrap();
        let s = fs::read_to_string(settings_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        for event in EVENTS {
            let arr = parsed["hooks"][event].as_array();
            if let Some(arr) = arr {
                assert!(
                    !arr.iter().any(|e| e["_duru"] == true),
                    "{event} still has duru entry"
                );
            }
        }
    }

    #[test]
    fn uninstall_preserves_non_duru_hooks() {
        let home = fake_home();
        let existing = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "bash /other/hook.sh"}]}
                ]
            }
        }"#;
        fs::write(settings_path(home.path()), existing).unwrap();
        install(home.path(), &opts_install_silent()).unwrap();
        uninstall(home.path(), &opts_uninstall_silent()).unwrap();

        let s = fs::read_to_string(settings_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        let pre = parsed["hooks"]["PreToolUse"].as_array().unwrap();
        let has_other = pre
            .iter()
            .any(|e| e["hooks"][0]["command"].as_str() == Some("bash /other/hook.sh"));
        assert!(has_other);
    }

    #[test]
    fn status_reports_installed_when_all_six_present() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        let report = status(home.path()).unwrap();
        assert!(report.installed);
        for event in EVENTS {
            assert!(
                report.events_present.contains(&(*event).to_string()),
                "{event} not reported present"
            );
        }
    }

    #[test]
    fn status_reports_not_installed_on_empty_settings() {
        let home = fake_home();
        let report = status(home.path()).unwrap();
        assert!(!report.installed);
        assert!(report.events_present.is_empty());
    }

    #[test]
    fn status_reports_registry_count() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();
        fs::write(
            registry_dir(home.path()).join("abc.json"),
            r#"{
                "schema_version": 1,
                "session_id": "abc",
                "cwd": "/tmp",
                "transcript_path": "/tmp/abc.jsonl",
                "started_at": "2026-04-20T00:00:00Z",
                "last_heartbeat": "2026-04-20T00:00:00Z",
                "terminated": false
            }"#,
        )
        .unwrap();
        let report = status(home.path()).unwrap();
        assert_eq!(report.registry_alive, 1);
    }

    #[test]
    fn uninstall_identifies_by_command_path_even_without_marker() {
        let home = fake_home();
        install(home.path(), &opts_install_silent()).unwrap();

        // Strip the _duru markers to simulate a user who edited settings.json.
        let raw = fs::read_to_string(settings_path(home.path())).unwrap();
        let stripped = raw
            .replace("\"_duru\": true,", "")
            .replace("\"_duru\":true,", "");
        fs::write(settings_path(home.path()), stripped).unwrap();

        uninstall(home.path(), &opts_uninstall_silent()).unwrap();

        let s = fs::read_to_string(settings_path(home.path())).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        for event in EVENTS {
            if let Some(arr) = parsed["hooks"][event].as_array() {
                for e in arr.iter() {
                    let cmd = e["hooks"][0]["command"].as_str().unwrap_or("");
                    assert!(
                        !cmd.contains(".claude/duru/hooks/"),
                        "duru path still present after uninstall: {cmd}"
                    );
                }
            }
        }
    }

    #[test]
    fn install_dry_run_does_not_modify() {
        let home = fake_home();
        install(
            home.path(),
            &InstallOpts {
                dry_run: true,
                yes: true,
                star: false,
                force_star_prompt: false,
            },
        )
        .unwrap();
        assert!(!hooks_dir(home.path()).exists());
        assert!(!settings_path(home.path()).exists());
    }
}
