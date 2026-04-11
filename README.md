<p align="center">
  <img src="assets/mascot.png" alt="duru mascot">
</p>

# duru (두루)

Terminal dashboard for Claude Code — explore, manage, and monitor your setup.

> **두루** (Korean): thoroughly, comprehensively, all around — named after 두루미, the Korean crane

duru scans `~/.claude/` and displays all your CLAUDE.md files and auto-memory across every project in a Miller Columns TUI.

<p align="center">
  <img src="demo.gif" alt="duru demo" width="800">
</p>

## Install

### Homebrew (macOS / Linux)

```bash
brew install uppinote20/tap/duru
```

### Scoop (Windows)

```powershell
scoop bucket add uppinote20 https://github.com/uppinote20/scoop-bucket
scoop install duru
```

### Install script (macOS / Linux)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/uppinote20/duru/main/install.sh | bash
```

### From source

```bash
cargo install --path .
```

### Prebuilt binaries

Download from [Releases](https://github.com/uppinote20/duru/releases) for macOS (ARM/x86_64), Linux (GNU/musl), and Windows (x86_64/ARM64).

## Usage

```bash
duru                    # launch TUI
duru --theme light      # force light theme
duru --path ~/alt/.claude  # custom path
```

### Navigation

| Key | Action |
|-----|--------|
| `↑↓` / `jk` | Navigate within pane |
| `←→` / `hl` | Switch pane |
| `Enter` | Enter next pane |
| `q` | Quit |

## Layout

Miller Columns (3-pane):

- **Pane 1** — All projects that have CLAUDE.md or memory files
- **Pane 2** — Files in the selected project (CLAUDE.md, MEMORY.md, etc.)
- **Pane 3** — File content preview

## Theme

Rosé Pine with automatic dark/light detection.

## License

MIT OR Apache-2.0
