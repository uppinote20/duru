use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme::Theme;

/// Render a markdown string into a styled `Text` suitable for a ratatui `Paragraph`.
pub fn render_markdown(input: &str, theme: &Theme) -> Text<'static> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(input, options);
    let mut renderer = Renderer::new(theme);
    for event in parser {
        renderer.handle(event);
    }
    renderer.finalize()
}

struct Renderer<'a> {
    theme: &'a Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    pending_link_url: Option<String>,
    list_stack: Vec<Option<u64>>,
    item_prefix_pending: bool,
    blockquote_depth: usize,
}

impl<'a> Renderer<'a> {
    fn new(theme: &'a Theme) -> Self {
        Self {
            theme,
            lines: Vec::new(),
            current: Vec::new(),
            style_stack: Vec::new(),
            in_code_block: false,
            pending_link_url: None,
            list_stack: Vec::new(),
            item_prefix_pending: false,
            blockquote_depth: 0,
        }
    }

    fn current_style(&self) -> Style {
        self.style_stack
            .last()
            .copied()
            .unwrap_or_else(|| Style::default().fg(self.theme.text))
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.start_line_if_needed();
        let style = self.current_style();
        self.current.push(Span::styled(text.to_string(), style));
    }

    fn start_line_if_needed(&mut self) {
        if !self.current.is_empty() {
            return;
        }
        if self.blockquote_depth > 0 {
            let prefix = "▎ ".repeat(self.blockquote_depth);
            let style = Style::default().fg(self.theme.muted);
            self.current.push(Span::styled(prefix, style));
        }
        if self.item_prefix_pending {
            self.item_prefix_pending = false;
            self.emit_item_prefix();
        }
    }

    fn emit_item_prefix(&mut self) {
        let depth = self.list_stack.len().saturating_sub(1);
        let indent = "  ".repeat(depth);
        let prefix = match self.list_stack.last_mut() {
            Some(Some(counter)) => {
                let n = *counter;
                *counter += 1;
                format!("{indent}{n}. ")
            }
            Some(None) => {
                format!("{indent}• ")
            }
            None => String::new(),
        };
        if !prefix.is_empty() {
            let style = self.current_style();
            self.current.push(Span::styled(prefix, style));
        }
    }

    fn heading_style(&self, level: HeadingLevel) -> Style {
        let color = match level {
            HeadingLevel::H1 => self.theme.iris,
            HeadingLevel::H2 => self.theme.foam,
            HeadingLevel::H3 | HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => {
                self.theme.gold
            }
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    fn push_style(&mut self, transform: impl FnOnce(Style, &Theme) -> Style) {
        let base = self.current_style();
        let next = transform(base, self.theme);
        self.style_stack.push(next);
    }

    fn flush_line(&mut self) {
        let spans = std::mem::take(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    fn blank_line(&mut self) {
        self.lines.push(Line::default());
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                self.flush_line();
                if self.list_stack.is_empty() {
                    self.blank_line();
                }
            }
            Event::Start(Tag::List(start)) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.list_stack.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.blank_line();
                }
            }
            Event::Start(Tag::Item) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.item_prefix_pending = true;
            }
            Event::End(TagEnd::Item) => {
                self.flush_line();
            }
            Event::TaskListMarker(done) => {
                self.item_prefix_pending = false;
                let depth = self.list_stack.len().saturating_sub(1);
                let indent = "  ".repeat(depth);
                if !indent.is_empty() {
                    let style = self.current_style();
                    self.current.push(Span::styled(indent, style));
                }
                let (marker, color) = if done {
                    ("[x] ", self.theme.pine)
                } else {
                    ("[ ] ", self.theme.muted)
                };
                let style = Style::default().fg(color);
                self.current.push(Span::styled(marker.to_string(), style));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.blockquote_depth += 1;
                self.push_style(|s, theme| s.fg(theme.muted).add_modifier(Modifier::ITALIC));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.style_stack.pop();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                if self.blockquote_depth == 0 {
                    self.blank_line();
                }
            }
            Event::Rule => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let style = Style::default().fg(self.theme.muted);
                let rule = "─".repeat(80);
                self.current.push(Span::styled(rule, style));
                self.flush_line();
                self.blank_line();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                self.style_stack.push(self.heading_style(level));
            }
            Event::End(TagEnd::Heading(_)) => {
                self.style_stack.pop();
                self.flush_line();
                self.blank_line();
            }
            Event::Start(Tag::Strong) => {
                self.push_style(|s, _| s.add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::Strong) => {
                self.style_stack.pop();
            }
            Event::Start(Tag::Emphasis) => {
                self.push_style(|s, _| s.add_modifier(Modifier::ITALIC));
            }
            Event::End(TagEnd::Emphasis) => {
                self.style_stack.pop();
            }
            Event::Start(Tag::Strikethrough) => {
                self.push_style(|s, theme| {
                    s.fg(theme.muted).add_modifier(Modifier::CROSSED_OUT)
                });
            }
            Event::End(TagEnd::Strikethrough) => {
                self.style_stack.pop();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.pending_link_url = Some(dest_url.to_string());
                self.push_style(|s, theme| s.fg(theme.foam).add_modifier(Modifier::UNDERLINED));
            }
            Event::End(TagEnd::Link) => {
                self.style_stack.pop();
                if let Some(url) = self.pending_link_url.take() {
                    let muted = Style::default().fg(self.theme.muted);
                    self.current
                        .push(Span::styled(format!(" ({url})"), muted));
                }
            }
            Event::Code(text) => {
                self.start_line_if_needed();
                let style = Style::default()
                    .fg(self.theme.rose)
                    .bg(self.theme.overlay);
                self.current.push(Span::styled(text.to_string(), style));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.in_code_block = true;
                let code_style = Style::default()
                    .fg(self.theme.rose)
                    .bg(self.theme.surface);
                self.style_stack.push(code_style);
            }
            Event::End(TagEnd::CodeBlock) => {
                self.style_stack.pop();
                self.in_code_block = false;
                self.blank_line();
            }
            Event::Text(text) => {
                if self.in_code_block {
                    let content = text.as_ref();
                    let mut first = true;
                    for segment in content.split('\n') {
                        if !first {
                            self.flush_line();
                        }
                        if !segment.is_empty() {
                            let style = self.current_style();
                            self.current
                                .push(Span::styled(segment.to_string(), style));
                        }
                        first = false;
                    }
                } else {
                    self.push_text(text.as_ref());
                }
            }
            Event::SoftBreak => {
                self.push_text(" ");
            }
            Event::HardBreak => {
                self.flush_line();
            }
            _ => {}
        }
    }

    fn finalize(mut self) -> Text<'static> {
        if !self.current.is_empty() {
            self.flush_line();
        }
        // Trim trailing blank line added by the last End(Paragraph).
        while self
            .lines
            .last()
            .map(|l| l.spans.is_empty())
            .unwrap_or(false)
        {
            self.lines.pop();
        }
        Text::from(self.lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn empty_input_produces_empty_text() {
        let theme = Theme::dark();
        let text = render_markdown("", &theme);
        assert_eq!(text.lines.len(), 0);
    }

    #[test]
    fn single_paragraph_produces_one_line_with_text_color() {
        let theme = Theme::dark();
        let text = render_markdown("hello world", &theme);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "hello world");
        let span = &text.lines[0].spans[0];
        assert_eq!(span.style.fg, Some(theme.text));
    }

    #[test]
    fn two_paragraphs_are_separated_by_blank_line() {
        let theme = Theme::dark();
        let text = render_markdown("first\n\nsecond", &theme);
        assert_eq!(text.lines.len(), 3);
        assert_eq!(line_text(&text.lines[0]), "first");
        assert_eq!(line_text(&text.lines[1]), "");
        assert_eq!(line_text(&text.lines[2]), "second");
    }

    #[test]
    fn soft_break_becomes_single_space() {
        let theme = Theme::dark();
        let text = render_markdown("line one\nline two", &theme);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "line one line two");
    }

    #[test]
    fn hard_break_starts_new_line() {
        let theme = Theme::dark();
        let text = render_markdown("line one  \nline two", &theme);
        assert_eq!(text.lines.len(), 2);
        assert_eq!(line_text(&text.lines[0]), "line one");
        assert_eq!(line_text(&text.lines[1]), "line two");
    }

    #[test]
    fn h1_is_bold_iris() {
        let theme = Theme::dark();
        let text = render_markdown("# Title", &theme);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "Title");
        let span = &text.lines[0].spans[0];
        assert_eq!(span.style.fg, Some(theme.iris));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h2_is_bold_foam() {
        let theme = Theme::dark();
        let text = render_markdown("## Subsection", &theme);
        let span = &text.lines[0].spans[0];
        assert_eq!(span.style.fg, Some(theme.foam));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h3_through_h6_are_bold_gold() {
        let theme = Theme::dark();
        for prefix in ["### ", "#### ", "##### ", "###### "] {
            let text = render_markdown(&format!("{prefix}Heading"), &theme);
            let span = &text.lines[0].spans[0];
            assert_eq!(span.style.fg, Some(theme.gold));
            assert!(span.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn bold_adds_bold_modifier() {
        let theme = Theme::dark();
        let text = render_markdown("a **bold** word", &theme);
        let spans = &text.lines[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].content.as_ref(), "bold");
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert!(!spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn italic_adds_italic_modifier() {
        let theme = Theme::dark();
        let text = render_markdown("a *slanted* word", &theme);
        let spans = &text.lines[0].spans;
        assert_eq!(spans[1].content.as_ref(), "slanted");
        assert!(spans[1].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn strikethrough_uses_muted_and_crossed_out() {
        let theme = Theme::dark();
        let text = render_markdown("a ~~gone~~ word", &theme);
        let spans = &text.lines[0].spans;
        assert_eq!(spans[1].content.as_ref(), "gone");
        assert!(spans[1].style.add_modifier.contains(Modifier::CROSSED_OUT));
        assert_eq!(spans[1].style.fg, Some(theme.muted));
    }

    #[test]
    fn bold_inside_heading_keeps_heading_color() {
        let theme = Theme::dark();
        let text = render_markdown("# A **B** C", &theme);
        let spans = &text.lines[0].spans;
        for span in spans {
            assert_eq!(span.style.fg, Some(theme.iris));
        }
        assert_eq!(spans[1].content.as_ref(), "B");
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_code_is_rose_on_overlay() {
        let theme = Theme::dark();
        let text = render_markdown("use `foo()` here", &theme);
        let spans = &text.lines[0].spans;
        let code_span = spans
            .iter()
            .find(|s| s.content.as_ref() == "foo()")
            .expect("code span missing");
        assert_eq!(code_span.style.fg, Some(theme.rose));
        assert_eq!(code_span.style.bg, Some(theme.overlay));
    }

    #[test]
    fn fenced_code_block_is_rose_on_surface_multi_line() {
        let theme = Theme::dark();
        let input = "```\nlet x = 1;\nlet y = 2;\n```";
        let text = render_markdown(input, &theme);
        let code_lines: Vec<&Line> = text
            .lines
            .iter()
            .filter(|l| !l.spans.is_empty())
            .collect();
        assert_eq!(code_lines.len(), 2);
        assert_eq!(line_text(code_lines[0]), "let x = 1;");
        assert_eq!(line_text(code_lines[1]), "let y = 2;");
        for line in &code_lines {
            for span in &line.spans {
                assert_eq!(span.style.fg, Some(theme.rose));
                assert_eq!(span.style.bg, Some(theme.surface));
            }
        }
    }

    #[test]
    fn code_block_preserves_blank_lines_inside() {
        let theme = Theme::dark();
        let input = "```\na\n\nb\n```";
        let text = render_markdown(input, &theme);
        let first_three: Vec<String> = text.lines.iter().take(3).map(line_text).collect();
        assert_eq!(
            first_three,
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
    }

    #[test]
    fn link_text_is_foam_underlined_with_url_suffix() {
        let theme = Theme::dark();
        let text = render_markdown("see [docs](https://example.com) now", &theme);
        let spans = &text.lines[0].spans;

        let link_text = spans
            .iter()
            .find(|s| s.content.as_ref() == "docs")
            .expect("link text span missing");
        assert_eq!(link_text.style.fg, Some(theme.foam));
        assert!(link_text.style.add_modifier.contains(Modifier::UNDERLINED));

        let url_span = spans
            .iter()
            .find(|s| s.content.as_ref().contains("https://example.com"))
            .expect("url suffix missing");
        assert_eq!(url_span.style.fg, Some(theme.muted));
    }

    #[test]
    fn unordered_list_items_get_bullet_prefix() {
        let theme = Theme::dark();
        let text = render_markdown("- first\n- second", &theme);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"• first".to_string()));
        assert!(lines.contains(&"• second".to_string()));
    }

    #[test]
    fn ordered_list_items_get_numbered_prefix() {
        let theme = Theme::dark();
        let text = render_markdown("1. a\n2. b\n3. c", &theme);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"1. a".to_string()));
        assert!(lines.contains(&"2. b".to_string()));
        assert!(lines.contains(&"3. c".to_string()));
    }

    #[test]
    fn ordered_list_respects_start_value() {
        let theme = Theme::dark();
        let text = render_markdown("5. a\n6. b", &theme);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"5. a".to_string()));
        assert!(lines.contains(&"6. b".to_string()));
    }

    #[test]
    fn nested_list_indents_inner_items() {
        let theme = Theme::dark();
        let input = "- outer\n  - inner";
        let text = render_markdown(input, &theme);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"• outer".to_string()));
        assert!(lines.iter().any(|l| l == "  • inner"));
    }

    #[test]
    fn task_list_open_uses_muted_marker() {
        let theme = Theme::dark();
        let text = render_markdown("- [ ] todo", &theme);
        let line = &text.lines[0];
        let marker = line
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains("[ ]"))
            .expect("open marker missing");
        assert_eq!(marker.style.fg, Some(theme.muted));
    }

    #[test]
    fn task_list_done_uses_pine_marker() {
        let theme = Theme::dark();
        let text = render_markdown("- [x] done", &theme);
        let line = &text.lines[0];
        let marker = line
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains("[x]"))
            .expect("done marker missing");
        assert_eq!(marker.style.fg, Some(theme.pine));
    }

    #[test]
    fn blockquote_is_muted_italic_with_bar_prefix() {
        let theme = Theme::dark();
        let text = render_markdown("> quoted line", &theme);
        let line = &text.lines[0];
        assert!(line_text(line).starts_with("▎ "));
        let content_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains("quoted line"))
            .expect("quoted content missing");
        assert_eq!(content_span.style.fg, Some(theme.muted));
        assert!(content_span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn multi_line_blockquote_prefixes_each_line() {
        let theme = Theme::dark();
        let text = render_markdown("> first\n>\n> second", &theme);
        let contents: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(
            contents
                .iter()
                .any(|c| c.starts_with("▎ ") && c.contains("first"))
        );
        assert!(
            contents
                .iter()
                .any(|c| c.starts_with("▎ ") && c.contains("second"))
        );
    }

    #[test]
    fn horizontal_rule_produces_muted_line_of_dashes() {
        let theme = Theme::dark();
        let text = render_markdown("one\n\n---\n\ntwo", &theme);
        let rule_line = text
            .lines
            .iter()
            .find(|l| {
                let content = line_text(l);
                !content.is_empty() && content.chars().all(|c| c == '─' || c.is_whitespace())
            })
            .expect("rule line missing");
        let span = &rule_line.spans[0];
        assert_eq!(span.style.fg, Some(theme.muted));
        assert!(span.content.as_ref().contains('─'));
    }
}
