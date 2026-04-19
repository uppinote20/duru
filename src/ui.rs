use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, AppMode, Pane};
use crate::markdown;
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

fn render_sessions_layout(_frame: &mut Frame, _app: &App, _theme: &Theme, _area: Rect) {
    // Implemented in Task 16
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
                    let size = format_size(file.size);

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
    let help = match mode {
        AppMode::Memory => Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(theme.text)),
            Span::styled(" navigate  ", Style::default().fg(theme.muted)),
            Span::styled("←→", Style::default().fg(theme.text)),
            Span::styled(" pane  ", Style::default().fg(theme.muted)),
            Span::styled("e", Style::default().fg(theme.text)),
            Span::styled(" edit  ", Style::default().fg(theme.muted)),
            Span::styled("Tab", Style::default().fg(theme.text)),
            Span::styled(" sessions  ", Style::default().fg(theme.muted)),
            Span::styled("q", Style::default().fg(theme.text)),
            Span::styled(" quit", Style::default().fg(theme.muted)),
        ]),
        AppMode::Sessions => Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(theme.text)),
            Span::styled(" navigate  ", Style::default().fg(theme.muted)),
            Span::styled("←→", Style::default().fg(theme.text)),
            Span::styled(" pane  ", Style::default().fg(theme.muted)),
            Span::styled("s", Style::default().fg(theme.text)),
            Span::styled(" sort  ", Style::default().fg(theme.muted)),
            Span::styled("r", Style::default().fg(theme.text)),
            Span::styled(" refresh  ", Style::default().fg(theme.muted)),
            Span::styled("Tab", Style::default().fg(theme.text)),
            Span::styled(" memory  ", Style::default().fg(theme.muted)),
            Span::styled("q", Style::default().fg(theme.text)),
            Span::styled(" quit", Style::default().fg(theme.muted)),
        ]),
    };
    frame.render_widget(
        Paragraph::new(help).style(Style::default().bg(theme.surface)),
        area,
    );
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else {
        format!("{:.1}K", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_shows_bytes_for_small() {
        assert_eq!(format_size(100), "100B");
        assert_eq!(format_size(0), "0B");
    }

    #[test]
    fn format_size_shows_kb_for_large() {
        assert_eq!(format_size(2048), "2.0K");
        assert_eq!(format_size(1536), "1.5K");
    }
}
