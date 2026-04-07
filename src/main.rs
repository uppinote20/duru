mod theme;

use clap::Parser;

#[derive(Parser)]
#[command(name = "duru", version, about = "Claude Code memory viewer")]
struct Cli {
    /// Force color theme (dark or light)
    #[arg(long)]
    theme: Option<String>,

    /// Custom ~/.claude/ path
    #[arg(long)]
    path: Option<std::path::PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    println!("duru v{}", env!("CARGO_PKG_VERSION"));
    println!("theme: {:?}, path: {:?}", cli.theme, cli.path);
}
