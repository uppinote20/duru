// collapsible_match: the project intentionally uses `match arm => { if cond { ... } }`
// instead of match guards, for readability when the condition references captured state.
#![allow(clippy::collapsible_match, clippy::collapsible_if)]

mod app;
mod hook_scripts;
mod hooks_install;
mod markdown;
mod registry;
mod scan;
mod secrets;
mod sessions;
mod theme;
mod ui;
mod warm;

use std::io;
use std::path::PathBuf;
use std::process::Command;

use clap::{Parser, Subcommand};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::App;
use scan::{demo_projects, scan_claude_dir};
use theme::Theme;

#[derive(Parser, Debug)]
#[command(
    name = "duru",
    version,
    about = "Claude Code memory and sessions dashboard"
)]
struct Cli {
    /// Force color theme (dark or light)
    #[arg(long)]
    theme: Option<String>,

    /// Custom ~/.claude/ path
    #[arg(long)]
    path: Option<PathBuf>,

    /// Use demo data (for screenshots and testing)
    #[arg(long)]
    demo: bool,

    #[command(subcommand)]
    command: Option<TopCommand>,
}

#[derive(Subcommand, Debug)]
enum TopCommand {
    /// Install, uninstall, or check duru Claude Code hooks
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
    /// Cache warming daemon controls (MVP3)
    Warm {
        #[command(subcommand)]
        action: warm::WarmAction,
    },
}

#[derive(Subcommand, Debug)]
enum HooksAction {
    /// Install duru hooks into ~/.claude/settings.json
    Install {
        /// Don't modify anything; print what would happen
        #[arg(long)]
        dry_run: bool,
        /// Non-interactive; skip star prompt
        #[arg(long)]
        yes: bool,
        /// Star the repo without asking
        #[arg(long)]
        star: bool,
        /// Re-ask the star prompt even if previously asked
        #[arg(long)]
        force_star_prompt: bool,
    },
    /// Remove duru hooks from ~/.claude/settings.json
    Uninstall {
        #[arg(long)]
        dry_run: bool,
        /// Also delete ~/.claude/duru/ (hooks, registry, markers)
        #[arg(long)]
        force: bool,
    },
    /// Show current hook installation status
    Status,
}

fn resolve_editor() -> String {
    resolve_editor_from(
        std::env::var("VISUAL").ok().as_deref(),
        std::env::var("EDITOR").ok().as_deref(),
    )
}

/// Pure helper for testability — avoids `unsafe` `set_var` in Rust 2024.
fn resolve_editor_from(visual: Option<&str>, editor: Option<&str>) -> String {
    visual.or(editor).unwrap_or("vi").to_string()
}

fn run_hooks_command(home: &std::path::Path, action: HooksAction) -> io::Result<()> {
    match action {
        HooksAction::Install {
            dry_run,
            yes,
            star,
            force_star_prompt,
        } => hooks_install::install(
            home,
            &hooks_install::InstallOpts {
                dry_run,
                yes,
                star,
                force_star_prompt,
            },
        ),
        HooksAction::Uninstall { dry_run, force } => {
            hooks_install::uninstall(home, &hooks_install::UninstallOpts { dry_run, force })
        }
        HooksAction::Status => {
            let report = hooks_install::status(home)?;
            hooks_install::print_status(&report);
            Ok(())
        }
    }
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home dir"))?;

    // `--path <dir>` treats <dir> as the `.claude` root, so the home directory
    // we pass into the hooks command is the parent of <dir>. When `--path` is
    // omitted, the real home directory is used.
    let hooks_home = match &cli.path {
        Some(claude_root) => claude_root
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| home.clone()),
        None => home.clone(),
    };

    match cli.command {
        Some(TopCommand::Hooks { action }) => return run_hooks_command(&hooks_home, action),
        Some(TopCommand::Warm { action }) => return warm::run(&hooks_home, action),
        None => {}
    }

    let claude_dir = cli.path.clone().unwrap_or_else(|| home.join(".claude"));

    let projects = if cli.demo {
        demo_projects()
    } else {
        if !claude_dir.is_dir() {
            eprintln!("error: {} does not exist", claude_dir.display());
            std::process::exit(1);
        }

        let projects = scan_claude_dir(&claude_dir);
        if projects.is_empty() {
            eprintln!(
                "no CLAUDE.md or memory files found in {}",
                claude_dir.display()
            );
            std::process::exit(0);
        }
        projects
    };

    let theme = Theme::from_option(cli.theme.as_deref());

    let use_alt_screen = std::env::var("DURU_NO_ALT_SCREEN").is_err();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if use_alt_screen {
        execute!(stdout, EnterAlternateScreen)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(projects);
    if cli.demo {
        app = app.with_demo_sessions(sessions::demo_sessions());
    }
    let result = run_app(&mut terminal, &mut app, &theme, use_alt_screen, &claude_dir);

    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    theme: &Theme,
    use_alt_screen: bool,
    claude_dir: &std::path::Path,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app, theme))?;

        if crossterm::event::poll(std::time::Duration::from_millis(100))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
        {
            app.handle_key(key);
        }

        // Periodic Sessions refresh
        if !app.skip_real_refresh
            && app.mode == app::AppMode::Sessions
            && app.last_refresh.elapsed() >= app.refresh_interval()
        {
            app.refresh_sessions(claude_dir);
        }

        // Consume wants_refresh flag
        if app.wants_refresh {
            app.wants_refresh = false;
            if !app.skip_real_refresh {
                app.refresh_sessions(claude_dir);
            }
        }

        if app.wants_edit {
            app.wants_edit = false;

            if let Some(path) = app.selected_file_path().map(|p| p.to_path_buf()) {
                // Suspend the terminal.
                disable_raw_mode()?;
                if use_alt_screen {
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                }

                // Spawn $EDITOR. Split on whitespace so values like
                // "emacsclient -t" or "nano -l" work correctly.
                let editor = resolve_editor();
                let mut parts = editor.split_whitespace();
                let editor_result = if let Some(cmd) = parts.next() {
                    Command::new(cmd).args(parts).arg(&path).status()
                } else {
                    Err(io::Error::new(io::ErrorKind::InvalidInput, "empty $EDITOR"))
                };

                // Resume the terminal.
                if use_alt_screen {
                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                }
                enable_raw_mode()?;
                terminal.clear()?;

                match editor_result {
                    Ok(_) => {
                        // Refresh file size.
                        if let Some(project) = app.projects.get_mut(app.project_index)
                            && let Some(file) = project.files.get_mut(app.file_index)
                            && let Ok(meta) = std::fs::metadata(&file.path)
                        {
                            file.size = meta.len();
                        }

                        // Reload content, preserving scroll position.
                        let saved_scroll = app.scroll_offset;
                        app.load_content();
                        let total = app.content.lines().count() as u16;
                        app.scroll_offset = saved_scroll.min(total.saturating_sub(1));
                    }
                    Err(e) => {
                        app.content = format!("(failed to launch editor: {e})");
                        app.scroll_offset = 0;
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use warm::WarmAction;

    #[test]
    fn resolve_editor_prefers_visual() {
        assert_eq!(resolve_editor_from(Some("nvim"), Some("vi")), "nvim");
    }

    #[test]
    fn resolve_editor_falls_back_to_editor() {
        assert_eq!(resolve_editor_from(None, Some("nano")), "nano");
    }

    #[test]
    fn resolve_editor_defaults_to_vi() {
        assert_eq!(resolve_editor_from(None, None), "vi");
    }

    fn warm_action(args: &[&str]) -> WarmAction {
        let cli = Cli::try_parse_from(args).expect("parse");
        match cli.command {
            Some(TopCommand::Warm { action }) => action,
            other => panic!("expected Warm subcommand, got {other:?}"),
        }
    }

    #[test]
    fn warm_set_key_no_args() {
        let action = warm_action(&["duru", "warm", "set-key"]);
        assert!(matches!(action, WarmAction::SetKey { from_env: None }));
    }

    #[test]
    fn warm_set_key_from_env() {
        let action = warm_action(&["duru", "warm", "set-key", "--from-env", "ANTHROPIC_API_KEY"]);
        match action {
            WarmAction::SetKey { from_env } => {
                assert_eq!(from_env.as_deref(), Some("ANTHROPIC_API_KEY"));
            }
            _ => panic!("expected SetKey"),
        }
    }

    #[test]
    fn warm_set_key_rejects_positional_arg() {
        // Positional arg would leak into shell history (umbrella #18: MUST NOT accept).
        assert!(Cli::try_parse_from(["duru", "warm", "set-key", "sk-ant-leaked"]).is_err());
    }

    #[test]
    fn warm_check_key() {
        assert!(matches!(
            warm_action(&["duru", "warm", "check-key"]),
            WarmAction::CheckKey
        ));
    }

    #[test]
    fn warm_unset_key() {
        assert!(matches!(
            warm_action(&["duru", "warm", "unset-key"]),
            WarmAction::UnsetKey
        ));
    }

    #[test]
    fn warm_dry_run_requires_session_id() {
        let action = warm_action(&["duru", "warm", "dry-run", "676b2e79"]);
        match action {
            WarmAction::DryRun { session_id } => assert_eq!(session_id, "676b2e79"),
            _ => panic!("expected DryRun"),
        }
        assert!(Cli::try_parse_from(["duru", "warm", "dry-run"]).is_err());
    }

    #[test]
    fn warm_install_flags() {
        let default = warm_action(&["duru", "warm", "install"]);
        assert!(matches!(default, WarmAction::Install { dry_run: false }));
        let dry = warm_action(&["duru", "warm", "install", "--dry-run"]);
        assert!(matches!(dry, WarmAction::Install { dry_run: true }));
    }

    #[test]
    fn warm_uninstall() {
        assert!(matches!(
            warm_action(&["duru", "warm", "uninstall"]),
            WarmAction::Uninstall
        ));
    }

    #[test]
    fn warm_daemon() {
        assert!(matches!(
            warm_action(&["duru", "warm", "daemon"]),
            WarmAction::Daemon
        ));
    }

    #[test]
    fn warm_status_variants() {
        let plain = warm_action(&["duru", "warm", "status"]);
        assert!(matches!(
            plain,
            WarmAction::Status {
                daemon: false,
                recent: None
            }
        ));
        let daemon_only = warm_action(&["duru", "warm", "status", "--daemon"]);
        assert!(matches!(
            daemon_only,
            WarmAction::Status {
                daemon: true,
                recent: None
            }
        ));
        let recent = warm_action(&["duru", "warm", "status", "--recent", "5"]);
        match recent {
            WarmAction::Status { daemon, recent } => {
                assert!(!daemon);
                assert_eq!(recent, Some(5));
            }
            _ => panic!("expected Status"),
        }
    }
}
