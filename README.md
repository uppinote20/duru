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

### Modes

Press `Tab` to switch between two modes:

- **Memory** (default) — Browse `CLAUDE.md` and memory files across all projects
- **Sessions** — Live table of active Claude Code sessions with cache TTL countdowns

### Memory mode keys

| Key | Action |
|-----|--------|
| `↑↓` / `jk` | Navigate within pane |
| `←→` / `hl` | Switch pane |
| `Enter` | Enter next pane |
| `e` | Edit selected file in `$EDITOR` |
| `d` | Delete selected memory file (asks `y/N`; global `CLAUDE.md` is protected) |
| `Tab` | Switch to Sessions mode |
| `q` | Quit |

### Sessions mode keys

| Key | Action |
|-----|--------|
| `jk` / `↑↓` | Navigate rows (Table) / scroll (Detail) |
| `hl` / `←→` | Toggle Table / Detail focus |
| `s` | Cycle sort (activity → TTL → project → size) |
| `S` | Reverse sort direction (toggle asc/desc) |
| `r` | Force refresh |
| `g` `G` | Jump to top / bottom |
| `Tab` | Switch to Memory mode |
| `q` | Quit |

The active sort column shows a direction arrow in the table header: `↓` for descending (the default for activity and size), `↑` for ascending (the default for cache TTL and project). Press `S` to flip the current column's direction.

## Layout

**Memory mode** uses Miller Columns (3-pane):

- **Pane 1** — All projects that have CLAUDE.md or memory files
- **Pane 2** — Files in the selected project (CLAUDE.md, MEMORY.md, etc.)
- **Pane 3** — File content preview

**Sessions mode** uses a Table + Detail layout:

- **Table** — 7 columns: state glyph, short ID, project, mode, last activity, cache TTL, size
- **Detail** — Fixed 9-row panel showing full session metadata

Cache TTL is shown as a hybrid `mm:ss ████▌·····` bar with color thresholds (green > 50%, yellow 20–50%, red < 20%). The window length follows the session's actual policy — 5 min for `cache_control: ephemeral` (API default) or 60 min for the `ttl: "1h"` form that recent Claude Code versions send. duru reads each session's most recent assistant `usage.cache_creation` to decide the window, per row. Mode is sourced from Claude Code hooks when installed; shows `—` otherwise.

### State glyph

Two-state, aligned with the session's actual prompt-cache TTL window:

- `●` warm — last write within the session's TTL (5 min or 1 h, whichever Claude Code chose) or hook registry reports alive
- `○` cold — last write past the TTL window, or hook registry reports terminated / dead PID

## Hooks

Run `duru hooks install` to add Claude Code event hooks to `~/.claude/settings.json`. From then on, duru shows accurate permission mode, real `/exit` detection, and PID-based liveness instead of just mtime inference.

Requires `jq` on PATH (macOS: `brew install jq`; Debian/Ubuntu: `apt-get install jq`).

```bash
duru hooks install                # interactive, asks about starring on first run
duru hooks install --yes          # non-interactive, skips star prompt
duru hooks install --dry-run      # preview only, no changes
duru hooks status                 # show installation state
duru hooks uninstall              # remove duru entries, preserve others
duru hooks uninstall --force      # also delete ~/.claude/duru/
```

Hooks write per-session state into `~/.claude/duru/registry/<session_id>.json`. Terminated sessions are retained for 7 days, then auto-pruned. Installation preserves any non-duru hooks already in `settings.json`.

duru is safe to run without installing hooks — it falls back to mtime-based inference.

## Theme

Rosé Pine with automatic dark/light detection.

## License

MIT OR Apache-2.0
