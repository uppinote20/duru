mod app;
mod scan;
mod theme;
mod ui;

use std::io;
use std::path::PathBuf;

use clap::Parser;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::App;
use scan::scan_claude_dir;
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
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

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

    let theme = Theme::from_option(cli.theme.as_deref());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(projects);
    let result = run_app(&mut terminal, &mut app, &theme);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    theme: &Theme,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app, theme))?;

        if crossterm::event::poll(std::time::Duration::from_millis(100))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
        {
            app.handle_key(key);
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
