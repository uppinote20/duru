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
