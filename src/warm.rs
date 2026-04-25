//! `duru warm` — cache warming daemon controls.
//!
//! Secret management and the CLI surface land here; session-picking policy,
//! the daemon loop, and supervisor installation arrive in follow-up work.
//! See the MVP3 umbrella (#17) for the full design.

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use clap::Subcommand;

use crate::hooks_install::registry_dir;
use crate::secrets::{self, KeyringBackend, SecretBackend};

// Issue numbers referenced by stub messages. Keeping them here means
// a renumber or rescope touches one table, not eight `writeln!` calls.
const ISSUE_PREFIX_RECON: u32 = 19;
const ISSUE_DAEMON: u32 = 21;
const ISSUE_OBSERVABILITY: u32 = 23;

#[derive(Subcommand, Debug)]
pub enum WarmAction {
    /// Store the Anthropic API key in the OS keychain
    SetKey {
        /// Read key value from the named environment variable instead of stdin
        #[arg(long, value_name = "VAR")]
        from_env: Option<String>,
    },
    /// Show whether an API key is configured (redacted)
    CheckKey,
    /// Remove any stored API key from the keychain
    UnsetKey,
    /// Dry-run: reconstruct a cache-hit ping for a session and print it
    DryRun {
        /// Session id from the duru Sessions view
        session_id: String,
    },
    /// Install the launchd/systemd supervisor unit
    Install {
        /// Print what would happen without modifying anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the supervisor unit
    Uninstall,
    /// Run the warming loop in the foreground
    Daemon,
    /// Show key + daemon + recent-ping state
    Status {
        /// Only report daemon running/stopped
        #[arg(long)]
        daemon: bool,
        /// Show the last N ping outcomes
        #[arg(long, value_name = "N")]
        recent: Option<usize>,
    },
}

pub fn run(home: &Path, action: WarmAction) -> io::Result<()> {
    preflight(home)?;
    let backend = KeyringBackend;
    let interactive = io::stdin().is_terminal();
    dispatch_with_env(
        &backend,
        action,
        &mut io::stdin().lock(),
        &mut io::stdout(),
        interactive,
        |v| std::env::var(v).ok(),
    )
}

pub fn preflight(home: &Path) -> io::Result<()> {
    if registry_dir(home).is_dir() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "hooks not installed — run `duru hooks install` first",
    ))
}

fn dispatch_with_env<B, F>(
    backend: &B,
    action: WarmAction,
    stdin: &mut impl io::BufRead,
    stdout: &mut impl Write,
    interactive: bool,
    env_lookup: F,
) -> io::Result<()>
where
    B: SecretBackend,
    F: Fn(&str) -> Option<String>,
{
    match action {
        WarmAction::SetKey { from_env } => {
            handle_set_key(backend, from_env, env_lookup, interactive, stdin, stdout)
        }
        WarmAction::CheckKey => handle_check_key(backend, stdout),
        WarmAction::UnsetKey => handle_unset_key(backend, stdout),
        WarmAction::DryRun { session_id } => stub_note(
            stdout,
            &format!("dry-run {session_id}: prefix reconstruction"),
            ISSUE_PREFIX_RECON,
        ),
        WarmAction::Install { dry_run } => stub_note(
            stdout,
            if dry_run {
                "would install supervisor unit"
            } else {
                "install supervisor unit"
            },
            ISSUE_DAEMON,
        ),
        WarmAction::Uninstall => stub_note(stdout, "uninstall supervisor unit", ISSUE_DAEMON),
        WarmAction::Daemon => stub_note(stdout, "daemon loop", ISSUE_DAEMON),
        WarmAction::Status { daemon, recent } => handle_status(backend, daemon, recent, stdout),
    }
}

fn stub_note(stdout: &mut impl Write, what: &str, issue: u32) -> io::Result<()> {
    writeln!(stdout, "{what} (MVP3 #{issue})")
}

fn handle_set_key<B, F>(
    backend: &B,
    from_env: Option<String>,
    env_lookup: F,
    interactive: bool,
    stdin: &mut impl io::BufRead,
    stdout: &mut impl Write,
) -> io::Result<()>
where
    B: SecretBackend,
    F: Fn(&str) -> Option<String>,
{
    let raw = match from_env {
        Some(var) => env_lookup(&var).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("env var {var} is not set"))
        })?,
        None => read_key_from_stdin(interactive, stdin, stdout)?,
    };
    secrets::set_api_key(backend, &raw)?;
    writeln!(stdout, "api key stored in keychain")?;
    Ok(())
}

fn read_key_from_stdin(
    interactive: bool,
    stdin: &mut impl io::BufRead,
    stdout: &mut impl Write,
) -> io::Result<String> {
    if interactive {
        write!(stdout, "Paste Anthropic API key (will not echo): ")?;
        stdout.flush()?;
    }
    let mut line = String::new();
    stdin.read_line(&mut line)?;
    Ok(line)
}

fn handle_check_key<B: SecretBackend>(backend: &B, stdout: &mut impl Write) -> io::Result<()> {
    match secrets::get_api_key(backend)? {
        Some(key) => writeln!(
            stdout,
            "api key: configured ({})",
            secrets::redact_key(&key)
        )?,
        None => writeln!(stdout, "api key: not configured")?,
    }
    Ok(())
}

fn handle_unset_key<B: SecretBackend>(backend: &B, stdout: &mut impl Write) -> io::Result<()> {
    secrets::remove_api_key(backend)?;
    writeln!(stdout, "api key removed")?;
    Ok(())
}

fn handle_status<B: SecretBackend>(
    backend: &B,
    daemon_only: bool,
    recent: Option<usize>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    if !daemon_only {
        handle_check_key(backend, stdout)?;
    }
    stub_note(stdout, "daemon: not yet implemented", ISSUE_DAEMON)?;
    if let Some(n) = recent {
        stub_note(
            stdout,
            &format!("recent {n} pings: not yet implemented"),
            ISSUE_OBSERVABILITY,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryBackend;
    use tempfile::TempDir;

    fn dispatch_to_string(backend: &MemoryBackend, action: WarmAction, stdin: &str) -> String {
        let mut out = Vec::new();
        let mut input = stdin.as_bytes();
        dispatch_with_env(backend, action, &mut input, &mut out, false, |_| None)
            .expect("dispatch");
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn preflight_fails_without_registry() {
        let tmp = TempDir::new().unwrap();
        let err = preflight(tmp.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("duru hooks install"));
    }

    #[test]
    fn preflight_ok_with_registry() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(registry_dir(tmp.path())).unwrap();
        preflight(tmp.path()).expect("preflight");
    }

    #[test]
    fn set_key_from_stdin_stores_trimmed() {
        let backend = MemoryBackend::new();
        let out = dispatch_to_string(
            &backend,
            WarmAction::SetKey { from_env: None },
            "sk-ant-from-stdin\n",
        );
        assert!(out.contains("stored"));
        assert_eq!(backend.get().unwrap().as_deref(), Some("sk-ant-from-stdin"));
    }

    #[test]
    fn set_key_from_stdin_rejects_blank() {
        let backend = MemoryBackend::new();
        let mut out = Vec::new();
        let mut input = "\n".as_bytes();
        let err = dispatch_with_env(
            &backend,
            WarmAction::SetKey { from_env: None },
            &mut input,
            &mut out,
            false,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn set_key_from_env_reads_via_injected_lookup() {
        let backend = MemoryBackend::new();
        let mut out = Vec::new();
        let mut input = "".as_bytes();
        dispatch_with_env(
            &backend,
            WarmAction::SetKey {
                from_env: Some("ANTHROPIC_API_KEY".into()),
            },
            &mut input,
            &mut out,
            false,
            |v| (v == "ANTHROPIC_API_KEY").then(|| "sk-ant-from-env".to_string()),
        )
        .expect("dispatch");
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("stored"));
        assert_eq!(backend.get().unwrap().as_deref(), Some("sk-ant-from-env"));
    }

    #[test]
    fn set_key_from_env_missing_errors() {
        let backend = MemoryBackend::new();
        let mut out = Vec::new();
        let mut input = "".as_bytes();
        let err = dispatch_with_env(
            &backend,
            WarmAction::SetKey {
                from_env: Some("ANTHROPIC_API_KEY".into()),
            },
            &mut input,
            &mut out,
            false,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn read_key_from_stdin_does_not_prompt_when_non_interactive() {
        let mut input = "sk-ant-direct\n".as_bytes();
        let mut out = Vec::new();
        let raw = read_key_from_stdin(false, &mut input, &mut out).expect("read");
        assert_eq!(raw.trim(), "sk-ant-direct");
        assert!(out.is_empty(), "non-interactive must not print a prompt");
    }

    #[test]
    fn read_key_from_stdin_prompts_when_interactive() {
        let mut input = "sk-ant-direct\n".as_bytes();
        let mut out = Vec::new();
        read_key_from_stdin(true, &mut input, &mut out).expect("read");
        let prompt = String::from_utf8(out).unwrap();
        assert!(prompt.contains("Paste"), "interactive must print a prompt");
    }

    #[test]
    fn check_key_when_absent() {
        let backend = MemoryBackend::new();
        let out = dispatch_to_string(&backend, WarmAction::CheckKey, "");
        assert!(out.contains("not configured"));
    }

    #[test]
    fn check_key_when_present_redacts() {
        let backend = MemoryBackend::new();
        backend.set("sk-ant-api03-secretstuff").unwrap();
        let out = dispatch_to_string(&backend, WarmAction::CheckKey, "");
        assert!(out.contains("configured"));
        assert!(out.contains("sk-ant-ap…"));
        assert!(!out.contains("secretstuff"));
    }

    #[test]
    fn unset_key_is_idempotent() {
        let backend = MemoryBackend::new();
        dispatch_to_string(&backend, WarmAction::UnsetKey, "");
        let out = dispatch_to_string(&backend, WarmAction::UnsetKey, "");
        assert!(out.contains("removed"));
        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn unset_key_clears_existing() {
        let backend = MemoryBackend::new();
        backend.set("sk-ant-xxx").unwrap();
        dispatch_to_string(&backend, WarmAction::UnsetKey, "");
        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn status_default_includes_key_and_daemon() {
        let backend = MemoryBackend::new();
        let out = dispatch_to_string(
            &backend,
            WarmAction::Status {
                daemon: false,
                recent: None,
            },
            "",
        );
        assert!(out.contains("api key"));
        assert!(out.contains("daemon"));
        assert!(!out.contains("recent"));
    }

    #[test]
    fn status_daemon_only_skips_key_line() {
        let backend = MemoryBackend::new();
        let out = dispatch_to_string(
            &backend,
            WarmAction::Status {
                daemon: true,
                recent: None,
            },
            "",
        );
        assert!(!out.contains("api key"));
        assert!(out.contains("daemon"));
    }

    #[test]
    fn status_recent_shows_stub_line() {
        let backend = MemoryBackend::new();
        let out = dispatch_to_string(
            &backend,
            WarmAction::Status {
                daemon: false,
                recent: Some(7),
            },
            "",
        );
        assert!(out.contains("recent 7"));
    }

    #[test]
    fn stub_subcommands_do_not_error() {
        let backend = MemoryBackend::new();
        dispatch_to_string(
            &backend,
            WarmAction::DryRun {
                session_id: "abc".into(),
            },
            "",
        );
        dispatch_to_string(&backend, WarmAction::Install { dry_run: false }, "");
        dispatch_to_string(&backend, WarmAction::Install { dry_run: true }, "");
        dispatch_to_string(&backend, WarmAction::Uninstall, "");
        dispatch_to_string(&backend, WarmAction::Daemon, "");
    }
}
