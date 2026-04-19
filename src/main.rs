mod app;
mod markdown;
mod scan;
mod sessions;
mod theme;
mod ui;

use std::io;
use std::path::PathBuf;
use std::process::Command;

use clap::Parser;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::App;
use scan::{demo_projects, scan_claude_dir};
use theme::Theme;

#[derive(Parser)]
#[command(name = "duru", version, about = "Claude Code memory viewer")]
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

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    let projects = if cli.demo {
        demo_projects()
    } else {
        let claude_dir = cli.path.unwrap_or_else(|| {
            dirs::home_dir()
                .expect("cannot resolve home directory")
                .join(".claude")
        });

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
    let result = run_app(&mut terminal, &mut app, &theme, use_alt_screen);

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
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app, theme))?;

        if crossterm::event::poll(std::time::Duration::from_millis(100))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
        {
            app.handle_key(key);
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
}
