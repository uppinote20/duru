use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, HighlightSpacing, List, ListItem, Paragraph, Row, Table,
        TableState, Wrap,
    },
};

use crate::app::{App, AppMode, Pane, SessionsPane};
use crate::markdown;
use crate::sessions::{self, State};
use crate::theme::Theme;

pub fn render(frame: &mut Frame, app: &App, theme: &Theme) {
    let area = frame.area();

    // Layout: tab bar (1) / body (flex) / help bar (1)
    let outer = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    render_tab_bar(frame, app.mode, theme, outer[0]);

    match app.mode {
        AppMode::Memory => render_memory_layout(frame, app, theme, outer[1]),
        AppMode::Sessions => render_sessions_layout(frame, app, theme, outer[1]),
    }

    render_help_bar(frame, app.mode, theme, outer[2]);
}

fn render_memory_layout(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Percentage(30),
        Constraint::Percentage(40),
    ])
    .split(area);

    render_projects_pane(frame, app, theme, chunks[0]);
    render_files_pane(frame, app, theme, chunks[1]);
    render_preview_pane(frame, app, theme, chunks[2]);
}

fn render_sessions_layout(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(SESSION_DETAIL_HEIGHT),
    ])
    .split(area);
    render_sessions_table(frame, app, theme, chunks[0]);
    render_sessions_detail(frame, app, theme, chunks[1]);
}

fn state_glyph(state: State, theme: &Theme) -> Span<'static> {
    match state {
        State::Active => Span::styled("●", Style::default().fg(theme.pine)),
        State::Stale => Span::styled("○", Style::default().fg(theme.muted)),
    }
}

fn relative_age(last: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> String {
    let elapsed = (now - last).num_seconds().max(0);
    sessions::format_duration(elapsed)
}

fn mode_abbrev(mode: Option<&str>) -> &'static str {
    match mode {
        Some("auto") => "auto",
        Some("default") => "default",
        Some("acceptEdits") => "accept",
        Some("plan") => "plan",
        Some(_) => "other",
        None => "—",
    }
}

fn render_sessions_table(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.sessions_focus == SessionsPane::Table;
    let now = chrono::Utc::now();

    let states: Vec<State> = app
        .sessions
        .iter()
        .map(|e| sessions::state_at(e, now))
        .collect();
    let warm = states.iter().filter(|s| **s == State::Active).count();
    let cold = states.iter().filter(|s| **s == State::Stale).count();
    let title = format!("Sessions ({} warm · {} cold)", warm, cold);
    let block = pane_block(&title, focused, theme);

    if app.sessions.is_empty() {
        let p = Paragraph::new("(no sessions found)")
            .block(block)
            .style(Style::default().fg(theme.muted).bg(theme.base))
            .alignment(Alignment::Center);
        frame.render_widget(p, area);
        return;
    }

    let header_cells = sessions_header_cells(app.sessions_sort, app.sort_reverse);
    let header = Row::new(header_cells.to_vec())
        .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .sessions
        .iter()
        .zip(states.iter())
        .map(|(entry, &state)| {
            let row_fg = match state {
                State::Active => theme.text,
                State::Stale => theme.muted,
            };
            let remaining = sessions::cache_ttl_remaining_secs(entry, now);
            let project = sessions::middle_truncate(&entry.project_name, PROJECT_NAME_MAX_WIDTH);

            Row::new(vec![
                Cell::from(Line::from(vec![Span::raw(" "), state_glyph(state, theme)])),
                Cell::from(entry.short_id.clone()),
                Cell::from(project),
                Cell::from(mode_abbrev(entry.permission_mode.as_deref()).to_string()),
                Cell::from(relative_age(entry.last_activity, now)),
                render_ttl_cell(remaining, theme, state),
                Cell::from(sessions::format_bytes(entry.file_size)),
            ])
            .style(Style::default().fg(row_fg))
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Length(8),
        Constraint::Min(20),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(14),
        Constraint::Length(7),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .fg(theme.iris)
                .bg(theme.overlay)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ")
        .highlight_spacing(HighlightSpacing::Always)
        .column_spacing(1);

    let mut state = TableState::default();
    state.select(Some(
        app.session_index.min(app.sessions.len().saturating_sub(1)),
    ));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_sessions_detail(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.sessions_focus == SessionsPane::Detail;

    let Some(entry) = app.sessions.get(app.session_index) else {
        let block = pane_block("Detail", focused, theme);
        frame.render_widget(
            Paragraph::new("")
                .block(block)
                .style(Style::default().bg(theme.base)),
            area,
        );
        return;
    };

    let title = format!("{} · {}", entry.short_id, entry.project_name);
    let block = pane_block(&title, focused, theme);

    let now = chrono::Utc::now();
    let remaining = sessions::cache_ttl_remaining_secs(entry, now);
    let state = sessions::state_at(entry, now);
    let (ttl_text, ttl_color) = ttl_cell_parts(remaining, theme, state);

    let started_line = match entry.started_at {
        Some(ts) => {
            let age = sessions::format_duration((now - ts).num_seconds().max(0));
            format!("{} ({} ago)", ts.format("%Y-%m-%d %H:%M"), age)
        }
        None => "—".to_string(),
    };

    let cwd_display = entry
        .cwd
        .as_deref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "—".to_string());

    let last_str = relative_age(entry.last_activity, now);

    let lines = vec![
        Line::from(vec![
            Span::styled("Session:    ", Style::default().fg(theme.muted)),
            Span::raw(entry.session_id.clone()),
        ]),
        Line::from(vec![
            Span::styled("Project:    ", Style::default().fg(theme.muted)),
            Span::raw(entry.project_name.clone()),
        ]),
        Line::from(vec![
            Span::styled("CWD:        ", Style::default().fg(theme.muted)),
            Span::raw(cwd_display),
        ]),
        Line::from(vec![
            Span::styled("Mode:       ", Style::default().fg(theme.muted)),
            Span::raw(mode_abbrev(entry.permission_mode.as_deref()).to_string()),
        ]),
        Line::from(vec![
            Span::styled("Started:    ", Style::default().fg(theme.muted)),
            Span::raw(started_line),
            Span::styled("    Last: ", Style::default().fg(theme.muted)),
            Span::raw(last_str),
        ]),
        Line::from(vec![
            Span::styled("TTL:        ", Style::default().fg(theme.muted)),
            Span::styled(ttl_text, Style::default().fg(ttl_color)),
        ]),
        Line::from(vec![
            Span::styled("Transcript: ", Style::default().fg(theme.muted)),
            Span::raw(entry.transcript_path.to_string_lossy().to_string()),
        ]),
    ];

    let p = Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(theme.text).bg(theme.base))
        .scroll((app.session_scroll, 0));
    frame.render_widget(p, area);
}

fn render_tab_bar(frame: &mut Frame, mode: AppMode, theme: &Theme, area: Rect) {
    let style_active = Style::default().fg(theme.iris).add_modifier(Modifier::BOLD);
    let style_muted = Style::default().fg(theme.muted);

    let (mem_style, ses_style) = match mode {
        AppMode::Memory => (style_active, style_muted),
        AppMode::Sessions => (style_muted, style_active),
    };

    let line = Line::from(vec![
        Span::styled("  Memory  ", mem_style),
        Span::styled("│", Style::default().fg(theme.overlay)),
        Span::styled("  Sessions  ", ses_style),
        Span::styled("   (Tab to switch)", Style::default().fg(theme.overlay)),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.surface)),
        area,
    );
}

fn pane_block<'a>(title: &'a str, focused: bool, theme: &'a Theme) -> Block<'a> {
    let border_color = if focused { theme.iris } else { theme.overlay };
    let title_style = if focused {
        Style::default().fg(theme.iris).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };

    Block::default()
        .title(Line::from(Span::styled(format!(" {title} "), title_style)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.base))
}

fn render_projects_pane(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.focus == Pane::Projects;
    let block = pane_block("duru", focused, theme);

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .enumerate()
        .map(|(i, project)| {
            let is_selected = i == app.project_index;
            let style = if is_selected {
                Style::default().fg(theme.iris).bg(theme.overlay)
            } else {
                Style::default().fg(theme.text)
            };

            let prefix = if is_selected { "▸ " } else { "  " };
            let count = format!(" ({})", project.files.len());

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(project.name.clone(), style),
                Span::styled(count, Style::default().fg(theme.muted)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_files_pane(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.focus == Pane::Files;
    let project_name = app
        .selected_project()
        .map(|p| p.name.as_str())
        .unwrap_or("");
    let block = pane_block(project_name, focused, theme);

    let items: Vec<ListItem> = app
        .selected_project()
        .map(|project| {
            project
                .files
                .iter()
                .enumerate()
                .map(|(i, file)| {
                    let is_selected = i == app.file_index;
                    let style = if is_selected {
                        Style::default().fg(theme.iris).bg(theme.overlay)
                    } else {
                        Style::default().fg(theme.text)
                    };

                    let prefix = if is_selected { "▸ " } else { "  " };
                    let size = sessions::format_bytes(file.size);

                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(file.name.clone(), style),
                        Span::styled(format!("  {size}"), Style::default().fg(theme.muted)),
                    ]))
                })
                .collect()
        })
        .unwrap_or_default();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_preview_pane(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.focus == Pane::Preview;
    let file_name = app
        .selected_project()
        .and_then(|p| p.files.get(app.file_index))
        .map(|f| f.name.as_str())
        .unwrap_or("");
    let block = pane_block(file_name, focused, theme);

    // Pane width minus the Block's left/right border (1 cell each).
    let content_width = area.width.saturating_sub(2);
    let rendered = markdown::render_markdown(&app.content, theme, content_width);
    let paragraph = Paragraph::new(rendered)
        .block(block)
        .style(Style::default().fg(theme.text).bg(theme.base))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset, 0));

    frame.render_widget(paragraph, area);
}

fn render_help_bar(frame: &mut Frame, mode: AppMode, theme: &Theme, area: Rect) {
    let entries: &[(&str, &str)] = match mode {
        AppMode::Memory => &[
            ("↑↓", "navigate"),
            ("←→", "pane"),
            ("e", "edit"),
            ("Tab", "sessions"),
            ("q", "quit"),
        ],
        AppMode::Sessions => &[
            ("↑↓", "navigate"),
            ("←→", "pane"),
            ("s", "sort"),
            ("S", "reverse"),
            ("r", "refresh"),
            ("Tab", "memory"),
            ("q", "quit"),
        ],
    };

    let mut spans = Vec::with_capacity(entries.len() * 2);
    for (i, (key, label)) in entries.iter().enumerate() {
        let prefix = if i == 0 { " " } else { "" };
        spans.push(Span::styled(
            format!("{prefix}{key}"),
            Style::default().fg(theme.text),
        ));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(theme.muted),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.surface)),
        area,
    );
}

const TTL_BAR_WIDTH: usize = 8;

/// Height of the Sessions-mode detail panel (includes 2 border rows).
const SESSION_DETAIL_HEIGHT: u16 = 9;

/// Max width the Project column renders before middle-truncating.
const PROJECT_NAME_MAX_WIDTH: usize = 22;

/// Cache-TTL color thresholds. Remaining-ratio above WARN → green, between
/// CRIT and WARN → yellow, below CRIT → red. Stale overrides all three to muted.
const TTL_WARN_RATIO: f64 = 0.5;
const TTL_CRIT_RATIO: f64 = 0.2;

/// Remaining-seconds threshold below which the TTL cell renders BOLD as a
/// last-minute urgency cue. Applies to Active sessions only.
const TTL_BOLD_SECS: i64 = 60;

/// For a Stale session the color is forced to muted regardless of remaining —
/// the cache may still be warm on Anthropic's side (resume within TTL = cache
/// hit), but the urgency cues (green/gold/red) shouldn't compete with the `○`
/// glyph. Text stays state-independent so the mm:ss bar remains readable.
fn ttl_cell_parts(remaining_secs: i64, theme: &Theme, state: State) -> (String, Color) {
    if remaining_secs <= 0 {
        return ("— expired".to_string(), theme.muted);
    }
    let mins = remaining_secs / 60;
    let secs = remaining_secs % 60;
    let ratio = remaining_secs as f64 / sessions::TTL_SECS as f64;
    let filled = (ratio * TTL_BAR_WIDTH as f64).round() as usize;
    let filled = filled.min(TTL_BAR_WIDTH);
    let color = if state == State::Stale {
        theme.muted
    } else if ratio > TTL_WARN_RATIO {
        theme.pine
    } else if ratio > TTL_CRIT_RATIO {
        theme.gold
    } else {
        theme.love
    };
    let bar: String = "█".repeat(filled) + &"·".repeat(TTL_BAR_WIDTH - filled);
    let text = format!("{:02}:{:02} {}", mins, secs, bar);
    (text, color)
}

/// True when the TTL cell should render BOLD: Active session with remaining
/// inside `[1, TTL_BOLD_SECS)`. Stale sessions never bold — the `○` glyph
/// already signals the session is cooled off, a BOLD urgency cue would fight it.
fn ttl_urgent(remaining_secs: i64, state: State) -> bool {
    state == State::Active && (1..TTL_BOLD_SECS).contains(&remaining_secs)
}

/// Sessions table header labels, with an arrow appended to the active sort
/// column. `↓` = Desc (newest / largest / longest-TTL first), `↑` = Asc.
fn sessions_header_cells(sort: sessions::SessionsSort, reverse: bool) -> [String; 7] {
    let arrow = match sort.effective_direction(reverse) {
        sessions::SortDirection::Asc => "↑",
        sessions::SortDirection::Desc => "↓",
    };
    let mut cells: [String; 7] = [
        String::new(),
        "ID".into(),
        "Project".into(),
        "Mode".into(),
        "Last".into(),
        "Cache TTL".into(),
        "Size".into(),
    ];
    let idx = match sort {
        sessions::SessionsSort::Project => 2,
        sessions::SessionsSort::LastActivity => 4,
        sessions::SessionsSort::CacheTtl => 5,
        sessions::SessionsSort::Size => 6,
    };
    cells[idx] = format!("{} {}", cells[idx], arrow);
    cells
}

fn render_ttl_cell(remaining_secs: i64, theme: &Theme, state: State) -> Cell<'static> {
    let (text, color) = ttl_cell_parts(remaining_secs, theme, state);
    let style = if ttl_urgent(remaining_secs, state) {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color)
    };
    Cell::from(Line::from(Span::styled(text, style)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_abbrev_maps_accept_edits_to_accept() {
        assert_eq!(mode_abbrev(Some("acceptEdits")), "accept");
    }

    #[test]
    fn mode_abbrev_passes_through_known_short_values() {
        assert_eq!(mode_abbrev(Some("auto")), "auto");
        assert_eq!(mode_abbrev(Some("default")), "default");
        assert_eq!(mode_abbrev(Some("plan")), "plan");
    }

    #[test]
    fn mode_abbrev_falls_back_to_other_for_unknown() {
        assert_eq!(mode_abbrev(Some("bypassPermissions")), "other");
    }

    #[test]
    fn mode_abbrev_returns_dash_for_none() {
        assert_eq!(mode_abbrev(None), "—");
    }

    #[test]
    fn ttl_cell_expired_has_em_dash() {
        let theme = Theme::dark();
        let (text, _) = ttl_cell_parts(0, &theme, State::Active);
        assert!(text.contains("— expired"));
    }

    #[test]
    fn ttl_cell_has_mm_ss_format() {
        let theme = Theme::dark();
        let (text, _) = ttl_cell_parts(277, &theme, State::Active);
        assert!(text.starts_with("04:37"));
    }

    #[test]
    fn ttl_cell_high_uses_pine() {
        let theme = Theme::dark();
        let (_, color) = ttl_cell_parts(270, &theme, State::Active);
        assert_eq!(color, theme.pine);
    }

    #[test]
    fn ttl_cell_medium_uses_gold() {
        let theme = Theme::dark();
        let (_, color) = ttl_cell_parts(120, &theme, State::Active);
        assert_eq!(color, theme.gold);
    }

    #[test]
    fn ttl_cell_low_uses_love() {
        let theme = Theme::dark();
        let (_, color) = ttl_cell_parts(30, &theme, State::Active);
        assert_eq!(color, theme.love);
    }

    #[test]
    fn ttl_cell_bar_shrinks_as_time_elapses() {
        let theme = Theme::dark();
        let (full_text, _) = ttl_cell_parts(300, &theme, State::Active);
        let (empty_text, _) = ttl_cell_parts(1, &theme, State::Active);
        let filled = |s: &str| s.matches('█').count();
        assert!(filled(&full_text) >= filled(&empty_text));
    }

    #[test]
    fn ttl_cell_stale_overrides_color_to_muted() {
        let theme = Theme::dark();
        // remaining=270s would normally be pine (ratio > 0.5); Stale forces muted.
        let (_, color) = ttl_cell_parts(270, &theme, State::Stale);
        assert_eq!(color, theme.muted);
    }

    #[test]
    fn ttl_cell_stale_preserves_text() {
        let theme = Theme::dark();
        let (active_text, _) = ttl_cell_parts(270, &theme, State::Active);
        let (stale_text, _) = ttl_cell_parts(270, &theme, State::Stale);
        assert_eq!(stale_text, active_text);
    }

    #[test]
    fn ttl_urgent_stale_never_bold() {
        assert!(!ttl_urgent(1, State::Stale));
        assert!(!ttl_urgent(30, State::Stale));
        assert!(!ttl_urgent(59, State::Stale));
    }

    #[test]
    fn ttl_urgent_active_under_threshold_is_urgent() {
        assert!(ttl_urgent(1, State::Active));
        assert!(ttl_urgent(59, State::Active));
    }

    #[test]
    fn ttl_urgent_active_at_or_above_threshold_not_urgent() {
        assert!(!ttl_urgent(TTL_BOLD_SECS, State::Active));
        assert!(!ttl_urgent(300, State::Active));
    }

    #[test]
    fn ttl_urgent_non_positive_remaining_not_urgent() {
        assert!(!ttl_urgent(0, State::Active));
        assert!(!ttl_urgent(-10, State::Active));
    }

    #[test]
    fn header_cells_mark_active_sort_field_with_default_arrow() {
        let cells = sessions_header_cells(sessions::SessionsSort::LastActivity, false);
        assert_eq!(cells[4], "Last ↓");
        assert_eq!(cells[5], "Cache TTL", "inactive field has no arrow");
        assert_eq!(cells[6], "Size");
        assert_eq!(cells[2], "Project");
    }

    #[test]
    fn header_cells_arrow_flips_when_reversed() {
        let cells = sessions_header_cells(sessions::SessionsSort::LastActivity, true);
        assert_eq!(cells[4], "Last ↑");
    }

    #[test]
    fn header_cells_cache_ttl_defaults_ascending() {
        // CacheTtl's natural direction is Asc (expiring-first).
        let cells = sessions_header_cells(sessions::SessionsSort::CacheTtl, false);
        assert_eq!(cells[5], "Cache TTL ↑");
    }

    #[test]
    fn header_cells_project_sort_marks_column_two() {
        let cells = sessions_header_cells(sessions::SessionsSort::Project, false);
        assert_eq!(cells[2], "Project ↑");
        assert_eq!(cells[4], "Last");
    }

    #[test]
    fn header_cells_size_sort_marks_column_six() {
        let cells = sessions_header_cells(sessions::SessionsSort::Size, false);
        assert_eq!(cells[6], "Size ↓");
        assert_eq!(cells[4], "Last");
    }
}
