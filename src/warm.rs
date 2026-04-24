//! `duru warm` CLI scaffold — MVP3.
//!
//! PR1 wires only `set-key`, `unset-key`, and `check-key`. Every other
//! subcommand parses correctly but its handler prints a stub pointing at the
//! MVP3 sub-issue that will implement it. This lets later PRs land each slice
//! without renaming or moving the clap surface.

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use clap::Subcommand;

use crate::secrets::{self, KeyringBackend, SecretBackend};

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
    /// Test prefix reconstruction against a session (filled in by #19)
    DryRun {
        /// Session id from the duru Sessions view
        session_id: String,
    },
    /// Install the launchd/systemd supervisor unit (filled in by #21)
    Install {
        /// Print what would happen without modifying anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the supervisor unit (filled in by #21)
    Uninstall,
    /// Run the warming loop in the foreground (filled in by #21)
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

/// Entry point from `main.rs`. Returns an error if preflight fails.
pub fn run(home: &Path, action: WarmAction) -> io::Result<()> {
    preflight(home)?;
    let backend = KeyringBackend::new();
    dispatch(&backend, action, &mut io::stdin().lock(), &mut io::stdout())
}

/// All warm subcommands require `duru hooks install` to have run first.
/// Without the registry there's nothing to warm and nothing to report on.
pub fn preflight(home: &Path) -> io::Result<()> {
    let registry = home.join(".claude").join("duru").join("registry");
    if registry.is_dir() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "hooks not installed — run `duru hooks install` first",
    ))
}

pub fn dispatch<B: SecretBackend>(
    backend: &B,
    action: WarmAction,
    stdin: &mut impl io::BufRead,
    stdout: &mut impl Write,
) -> io::Result<()> {
    match action {
        WarmAction::SetKey { from_env } => handle_set_key(backend, from_env, stdin, stdout),
        WarmAction::CheckKey => handle_check_key(backend, stdout),
        WarmAction::UnsetKey => handle_unset_key(backend, stdout),
        WarmAction::DryRun { session_id } => {
            writeln!(
                stdout,
                "dry-run for {session_id}: prefix reconstruction lands in MVP3 issue #19"
            )?;
            Ok(())
        }
        WarmAction::Install { dry_run } => {
            let verb = if dry_run { "would install" } else { "install" };
            writeln!(stdout, "{verb} supervisor unit lands in MVP3 issue #21")?;
            Ok(())
        }
        WarmAction::Uninstall => {
            writeln!(stdout, "uninstall lands in MVP3 issue #21")?;
            Ok(())
        }
        WarmAction::Daemon => {
            writeln!(stdout, "daemon loop lands in MVP3 issue #21")?;
            Ok(())
        }
        WarmAction::Status { daemon, recent } => handle_status(backend, daemon, recent, stdout),
    }
}

fn handle_set_key<B: SecretBackend>(
    backend: &B,
    from_env: Option<String>,
    stdin: &mut impl io::BufRead,
    stdout: &mut impl Write,
) -> io::Result<()> {
    let raw = match from_env {
        Some(var) => std::env::var(&var).map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, format!("env var {var} is not set"))
        })?,
        None => read_key_from_stdin(stdin, stdout)?,
    };
    secrets::set_api_key(backend, &raw)?;
    writeln!(stdout, "api key stored in keychain")?;
    Ok(())
}

fn read_key_from_stdin(
    stdin: &mut impl io::BufRead,
    stdout: &mut impl Write,
) -> io::Result<String> {
    if io::stdin().is_terminal() {
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
    writeln!(stdout, "daemon: not yet implemented (MVP3 issue #21)")?;
    if let Some(n) = recent {
        writeln!(
            stdout,
            "recent {n} pings: not yet implemented (MVP3 issue #23)"
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
        dispatch(backend, action, &mut input, &mut out).expect("dispatch");
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
        std::fs::create_dir_all(tmp.path().join(".claude/duru/registry")).unwrap();
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
        let err = dispatch(
            &backend,
            WarmAction::SetKey { from_env: None },
            &mut input,
            &mut out,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(backend.get().unwrap(), None);
    }

    #[test]
    fn set_key_from_env_reads_var() {
        let var = "DURU_TEST_API_KEY_VAR";
        // Safe: test-only var, and we clean up immediately.
        // SAFETY: single-threaded test, no other readers of this env var.
        unsafe { std::env::set_var(var, "sk-ant-from-env") };
        let backend = MemoryBackend::new();
        let out = dispatch_to_string(
            &backend,
            WarmAction::SetKey {
                from_env: Some(var.to_string()),
            },
            "",
        );
        // SAFETY: see above.
        unsafe { std::env::remove_var(var) };
        assert!(out.contains("stored"));
        assert_eq!(backend.get().unwrap().as_deref(), Some("sk-ant-from-env"));
    }

    #[test]
    fn set_key_from_env_missing_errors() {
        let backend = MemoryBackend::new();
        let mut out = Vec::new();
        let mut input = "".as_bytes();
        let err = dispatch(
            &backend,
            WarmAction::SetKey {
                from_env: Some("DURU_DEFINITELY_UNSET_VAR_9".to_string()),
            },
            &mut input,
            &mut out,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
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
