//! @handbook 7.1-frontmatter-peeling
//! @handbook 7.2-style-stack
//! @handbook 7.3-width-aware-rules
//! @handbook 7.4-blockquote-depth-tracking
//! @tested src/markdown.rs#tests

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme::Theme;

/// Render a markdown string into a styled `Text` suitable for a ratatui `Paragraph`.
///
/// `content_width` is the inner width (in cells) of the destination `Paragraph`
/// — i.e. the pane width minus any surrounding `Block` border. Horizontal rules,
/// table header separators, and the frontmatter divider are sized to exactly
/// this width so they render as a single clean line instead of wrapping.
pub fn render_markdown(input: &str, theme: &Theme, content_width: u16) -> Text<'static> {
    let (frontmatter, body) = split_frontmatter(input);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);

    let mut renderer = Renderer::new(theme, content_width);
    if let Some(fm) = frontmatter {
        renderer.emit_frontmatter_header(fm);
    }

    let parser = Parser::new_ext(body, options);
    for event in parser {
        renderer.handle(event);
    }
    renderer.finalize()
}

/// Split a leading YAML frontmatter block from the rest of the document.
///
/// Claude Code memory files start with metadata like `name:`, `description:`,
/// `type:` between a pair of `---` fences. CommonMark has no concept of
/// frontmatter, so leaving it in place causes the fences to be parsed as
/// thematic breaks and the body text as a setext H2 heading. This helper
/// peels off the fenced block (if present) so the renderer can emit it as
/// a styled metadata header while the body goes through the normal parser.
///
/// Returns `(Some(frontmatter_body), body_after_frontmatter)` when a complete
/// block is found, otherwise `(None, full_input)`.
fn split_frontmatter(input: &str) -> (Option<&str>, &str) {
    let Some(after_open) = input
        .strip_prefix("---\n")
        .or_else(|| input.strip_prefix("---\r\n"))
    else {
        return (None, input);
    };
    // Look for a closing fence — a line containing only `---`.
    let mut search_start = 0;
    while let Some(idx) = after_open[search_start..].find("\n---") {
        let fence_start = search_start + idx + 1; // index of the first `-`
        let after_fence = fence_start + 3;
        let tail = &after_open[after_fence..];
        let is_line_end = tail.is_empty() || tail.starts_with('\n') || tail.starts_with("\r\n");
        if is_line_end {
            let fm_body = &after_open[..fence_start - 1]; // drop the preceding `\n`
            // Step past the newline that ends the closing fence. CRLF is 2 bytes,
            // LF is 1 byte, and an EOF-terminated fence contributes nothing.
            let body_start = if tail.starts_with("\r\n") {
                after_fence + 2
            } else if tail.starts_with('\n') {
                after_fence + 1
            } else {
                after_fence
            };
            return (Some(fm_body), &after_open[body_start..]);
        }
        search_start = fence_start + 3;
    }
    // Unterminated frontmatter — leave input as-is.
    (None, input)
}

struct Renderer<'a> {
    theme: &'a Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    pending_link_url: Option<String>,
    pending_image_url: Option<String>,
    list_stack: Vec<Option<u64>>,
    item_prefix_pending: bool,
    blockquote_depth: usize,
    rule_width: usize,
}

impl<'a> Renderer<'a> {
    fn new(theme: &'a Theme, content_width: u16) -> Self {
        Self {
            theme,
            lines: Vec::new(),
            current: Vec::new(),
            style_stack: Vec::new(),
            in_code_block: false,
            pending_link_url: None,
            pending_image_url: None,
            list_stack: Vec::new(),
            item_prefix_pending: false,
            blockquote_depth: 0,
            rule_width: content_width.max(1) as usize,
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

    /// Render a YAML frontmatter block as an aligned metadata header at the
    /// top of the output: each `key: value` becomes one line with the key
    /// bolded in `muted`, the value in `text` color, keys left-padded to the
    /// width of the longest label. A `muted` `─` separator follows, then a
    /// blank line before the body starts.
    fn emit_frontmatter_header(&mut self, content: &str) {
        let pairs: Vec<(&str, &str)> = content
            .lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
            .filter(|(k, _)| !k.is_empty())
            .collect();

        if pairs.is_empty() {
            return;
        }

        let max_key = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        let key_style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);
        let value_style = Style::default().fg(self.theme.text);

        for (key, value) in &pairs {
            let padded = format!("{:<width$}  ", key, width = max_key);
            self.current.push(Span::styled(padded, key_style));
            self.current
                .push(Span::styled((*value).to_string(), value_style));
            self.flush_line();
        }

        let rule = Style::default().fg(self.theme.muted);
        self.current
            .push(Span::styled("─".repeat(self.rule_width), rule));
        self.flush_line();
        self.blank_line();
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
                let rule = "─".repeat(self.rule_width);
                self.current.push(Span::styled(rule, style));
                self.flush_line();
                self.blank_line();
            }
            Event::Html(text) | Event::InlineHtml(text) => {
                // CommonMark allows raw HTML. Terminals can't render it, so emit
                // the raw markup as muted text so it's visible but de-emphasized.
                // `start_line_if_needed` must be called before the first span so
                // that blockquote `▎ ` and list bullet prefixes are emitted for
                // raw HTML just like any other text.
                self.start_line_if_needed();
                let content = text.as_ref();
                let style = Style::default().fg(self.theme.muted);
                let mut first = true;
                for segment in content.split('\n') {
                    if !first {
                        self.flush_line();
                        self.start_line_if_needed();
                    }
                    if !segment.is_empty() {
                        self.current.push(Span::styled(segment.to_string(), style));
                    }
                    first = false;
                }
            }
            Event::Start(Tag::Table(_)) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
            }
            Event::End(TagEnd::Table) => {
                self.blank_line();
            }
            Event::Start(Tag::TableHead) => {
                self.push_style(|s, _| s.add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::TableHead) => {
                self.style_stack.pop();
                self.flush_line();
                // Visual separator under the header row.
                let style = Style::default().fg(self.theme.muted);
                self.current
                    .push(Span::styled("─".repeat(self.rule_width), style));
                self.flush_line();
            }
            Event::Start(Tag::TableRow) => {}
            Event::End(TagEnd::TableRow) => {
                self.flush_line();
            }
            Event::Start(Tag::TableCell) => {
                if !self.current.is_empty()
                    && self
                        .current
                        .last()
                        .map(|s| !s.content.as_ref().ends_with("│ "))
                        .unwrap_or(true)
                {
                    let style = Style::default().fg(self.theme.muted);
                    self.current.push(Span::styled(" │ ".to_string(), style));
                }
            }
            Event::End(TagEnd::TableCell) => {}
            Event::Start(Tag::FootnoteDefinition(label)) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                let style = Style::default().fg(self.theme.muted);
                self.current
                    .push(Span::styled(format!("[^{label}]: "), style));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.flush_line();
                self.blank_line();
            }
            Event::FootnoteReference(label) => {
                self.start_line_if_needed();
                let style = Style::default().fg(self.theme.muted);
                self.current
                    .push(Span::styled(format!("[^{label}]"), style));
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
                self.push_style(|s, theme| s.fg(theme.muted).add_modifier(Modifier::CROSSED_OUT));
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
                    self.current.push(Span::styled(format!(" ({url})"), muted));
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.pending_image_url = Some(dest_url.to_string());
                self.push_style(|s, theme| s.fg(theme.muted));
                self.start_line_if_needed();
                let style = self.current_style();
                self.current
                    .push(Span::styled("[image: ".to_string(), style));
            }
            Event::End(TagEnd::Image) => {
                let style = self.current_style();
                self.current.push(Span::styled("]".to_string(), style));
                self.style_stack.pop();
                if let Some(url) = self.pending_image_url.take() {
                    let muted = Style::default().fg(self.theme.muted);
                    self.current.push(Span::styled(format!(" ({url})"), muted));
                }
            }
            Event::Code(text) => {
                self.start_line_if_needed();
                let style = Style::default().fg(self.theme.rose).bg(self.theme.overlay);
                self.current.push(Span::styled(text.to_string(), style));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                if !self.current.is_empty() {
                    self.flush_line();
                }
                self.in_code_block = true;
                let code_style = Style::default().fg(self.theme.rose).bg(self.theme.surface);
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
                            self.current.push(Span::styled(segment.to_string(), style));
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
        let text = render_markdown("", &theme, 80);
        assert_eq!(text.lines.len(), 0);
    }

    #[test]
    fn single_paragraph_produces_one_line_with_text_color() {
        let theme = Theme::dark();
        let text = render_markdown("hello world", &theme, 80);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "hello world");
        let span = &text.lines[0].spans[0];
        assert_eq!(span.style.fg, Some(theme.text));
    }

    #[test]
    fn two_paragraphs_are_separated_by_blank_line() {
        let theme = Theme::dark();
        let text = render_markdown("first\n\nsecond", &theme, 80);
        assert_eq!(text.lines.len(), 3);
        assert_eq!(line_text(&text.lines[0]), "first");
        assert_eq!(line_text(&text.lines[1]), "");
        assert_eq!(line_text(&text.lines[2]), "second");
    }

    #[test]
    fn soft_break_becomes_single_space() {
        let theme = Theme::dark();
        let text = render_markdown("line one\nline two", &theme, 80);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "line one line two");
    }

    #[test]
    fn hard_break_starts_new_line() {
        let theme = Theme::dark();
        let text = render_markdown("line one  \nline two", &theme, 80);
        assert_eq!(text.lines.len(), 2);
        assert_eq!(line_text(&text.lines[0]), "line one");
        assert_eq!(line_text(&text.lines[1]), "line two");
    }

    #[test]
    fn h1_is_bold_iris() {
        let theme = Theme::dark();
        let text = render_markdown("# Title", &theme, 80);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "Title");
        let span = &text.lines[0].spans[0];
        assert_eq!(span.style.fg, Some(theme.iris));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h2_is_bold_foam() {
        let theme = Theme::dark();
        let text = render_markdown("## Subsection", &theme, 80);
        let span = &text.lines[0].spans[0];
        assert_eq!(span.style.fg, Some(theme.foam));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h3_through_h6_are_bold_gold() {
        let theme = Theme::dark();
        for prefix in ["### ", "#### ", "##### ", "###### "] {
            let text = render_markdown(&format!("{prefix}Heading"), &theme, 80);
            let span = &text.lines[0].spans[0];
            assert_eq!(span.style.fg, Some(theme.gold));
            assert!(span.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn bold_adds_bold_modifier() {
        let theme = Theme::dark();
        let text = render_markdown("a **bold** word", &theme, 80);
        let spans = &text.lines[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].content.as_ref(), "bold");
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert!(!spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn italic_adds_italic_modifier() {
        let theme = Theme::dark();
        let text = render_markdown("a *slanted* word", &theme, 80);
        let spans = &text.lines[0].spans;
        assert_eq!(spans[1].content.as_ref(), "slanted");
        assert!(spans[1].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn strikethrough_uses_muted_and_crossed_out() {
        let theme = Theme::dark();
        let text = render_markdown("a ~~gone~~ word", &theme, 80);
        let spans = &text.lines[0].spans;
        assert_eq!(spans[1].content.as_ref(), "gone");
        assert!(spans[1].style.add_modifier.contains(Modifier::CROSSED_OUT));
        assert_eq!(spans[1].style.fg, Some(theme.muted));
    }

    #[test]
    fn bold_inside_heading_keeps_heading_color() {
        let theme = Theme::dark();
        let text = render_markdown("# A **B** C", &theme, 80);
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
        let text = render_markdown("use `foo()` here", &theme, 80);
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
        let text = render_markdown(input, &theme, 80);
        let code_lines: Vec<&Line> = text.lines.iter().filter(|l| !l.spans.is_empty()).collect();
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
        let text = render_markdown(input, &theme, 80);
        let first_three: Vec<String> = text.lines.iter().take(3).map(line_text).collect();
        assert_eq!(
            first_three,
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
    }

    #[test]
    fn link_text_is_foam_underlined_with_url_suffix() {
        let theme = Theme::dark();
        let text = render_markdown("see [docs](https://example.com) now", &theme, 80);
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
        let text = render_markdown("- first\n- second", &theme, 80);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"• first".to_string()));
        assert!(lines.contains(&"• second".to_string()));
    }

    #[test]
    fn ordered_list_items_get_numbered_prefix() {
        let theme = Theme::dark();
        let text = render_markdown("1. a\n2. b\n3. c", &theme, 80);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"1. a".to_string()));
        assert!(lines.contains(&"2. b".to_string()));
        assert!(lines.contains(&"3. c".to_string()));
    }

    #[test]
    fn ordered_list_respects_start_value() {
        let theme = Theme::dark();
        let text = render_markdown("5. a\n6. b", &theme, 80);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"5. a".to_string()));
        assert!(lines.contains(&"6. b".to_string()));
    }

    #[test]
    fn nested_list_indents_inner_items() {
        let theme = Theme::dark();
        let input = "- outer\n  - inner";
        let text = render_markdown(input, &theme, 80);
        let lines: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(lines.contains(&"• outer".to_string()));
        assert!(lines.iter().any(|l| l == "  • inner"));
    }

    #[test]
    fn task_list_open_uses_muted_marker() {
        let theme = Theme::dark();
        let text = render_markdown("- [ ] todo", &theme, 80);
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
        let text = render_markdown("- [x] done", &theme, 80);
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
        let text = render_markdown("> quoted line", &theme, 80);
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
        let text = render_markdown("> first\n>\n> second", &theme, 80);
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
    fn frontmatter_renders_as_metadata_header_above_body() {
        let theme = Theme::dark();
        let input = "---\nname: example\ntype: project\n---\n\nReal body.";
        let text = render_markdown(input, &theme, 80);
        let contents: Vec<String> = text.lines.iter().map(line_text).collect();

        // Body appears somewhere.
        assert!(contents.iter().any(|c| c == "Real body."));
        // Keys and values appear on their own lines, not merged into a H2 paragraph.
        assert!(
            contents
                .iter()
                .any(|c| c.trim_start().starts_with("name") && c.ends_with("example"))
        );
        assert!(
            contents
                .iter()
                .any(|c| c.trim_start().starts_with("type") && c.ends_with("project"))
        );
        // The body comes AFTER the metadata: find the first line whose content is "Real body."
        // and ensure at least one metadata line precedes it.
        let body_idx = contents.iter().position(|c| c == "Real body.").unwrap();
        assert!(body_idx > 0);
    }

    #[test]
    fn frontmatter_key_is_muted_bold_and_value_is_text_color() {
        let theme = Theme::dark();
        let input = "---\nname: example\n---\n\nbody";
        let text = render_markdown(input, &theme, 80);
        let meta_line = text
            .lines
            .iter()
            .find(|l| line_text(l).ends_with("example"))
            .expect("metadata line missing");
        // First span is the padded key, last span is the value.
        let key_span = &meta_line.spans[0];
        let value_span = meta_line.spans.last().unwrap();
        assert!(key_span.content.as_ref().trim_start().starts_with("name"));
        assert_eq!(key_span.style.fg, Some(theme.muted));
        assert!(key_span.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(value_span.content.as_ref(), "example");
        assert_eq!(value_span.style.fg, Some(theme.text));
    }

    #[test]
    fn frontmatter_keys_are_padded_to_align_values() {
        let theme = Theme::dark();
        let input = "---\nname: x\ndescription: y\n---\n\nbody";
        let text = render_markdown(input, &theme, 80);
        // First two rendered lines are the metadata pairs, in document order.
        assert!(text.lines.len() >= 2);
        let key0 = text.lines[0].spans[0].content.as_ref();
        let key1 = text.lines[1].spans[0].content.as_ref();
        assert_eq!(
            key0.len(),
            key1.len(),
            "keys {key0:?} and {key1:?} not aligned"
        );
        assert!(key0.trim_end().starts_with("name"));
        assert!(key1.trim_end().starts_with("description"));
    }

    #[test]
    fn frontmatter_is_followed_by_muted_rule_separator() {
        let theme = Theme::dark();
        let input = "---\nname: example\n---\n\nbody";
        let text = render_markdown(input, &theme, 80);
        // Find a dashed line styled muted, appearing before the body line.
        let body_idx = text
            .lines
            .iter()
            .position(|l| line_text(l) == "body")
            .unwrap();
        let rule_idx = text.lines[..body_idx]
            .iter()
            .position(|l| {
                l.spans
                    .first()
                    .is_some_and(|s| s.content.as_ref().chars().all(|c| c == '─'))
            })
            .expect("metadata separator missing");
        let span = &text.lines[rule_idx].spans[0];
        assert_eq!(span.style.fg, Some(theme.muted));
        // Separator must match the requested content width exactly so that
        // the Paragraph widget does NOT wrap it to two lines.
        assert_eq!(span.content.as_ref().chars().count(), 80);
    }

    #[test]
    fn rule_width_scales_with_content_width_parameter() {
        let theme = Theme::dark();
        let text = render_markdown("a\n\n---\n\nb", &theme, 40);
        let rule_line = text
            .lines
            .iter()
            .find(|l| {
                l.spans
                    .first()
                    .is_some_and(|s| s.content.as_ref().chars().all(|c| c == '─'))
            })
            .expect("rule missing");
        assert_eq!(rule_line.spans[0].content.as_ref().chars().count(), 40);
    }

    #[test]
    fn file_with_only_frontmatter_produces_just_metadata() {
        let theme = Theme::dark();
        let input = "---\nname: example\ntype: project\n---\n";
        let text = render_markdown(input, &theme, 80);
        let contents: Vec<String> = text.lines.iter().map(line_text).collect();
        assert!(contents.iter().any(|c| c.ends_with("example")));
        assert!(contents.iter().any(|c| c.ends_with("project")));
    }

    #[test]
    fn unterminated_frontmatter_is_left_alone() {
        let theme = Theme::dark();
        let input = "---\nname: example\n\nstill body";
        let text = render_markdown(input, &theme, 80);
        let joined: String = text
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("still body"));
    }

    #[test]
    fn file_without_frontmatter_is_unchanged() {
        let theme = Theme::dark();
        let text = render_markdown("just content", &theme, 80);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(line_text(&text.lines[0]), "just content");
    }

    #[test]
    fn horizontal_rule_produces_muted_line_of_dashes() {
        let theme = Theme::dark();
        let text = render_markdown("one\n\n---\n\ntwo", &theme, 80);
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

    #[test]
    fn image_renders_as_muted_alt_with_url_suffix() {
        let theme = Theme::dark();
        let text = render_markdown("![a cat](cat.png)", &theme, 80);
        let joined: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
            .collect();
        assert!(joined.contains("[image: "));
        assert!(joined.contains("a cat"));
        assert!(joined.contains("cat.png"));
        // All image spans should be muted.
        let image_spans: Vec<&Span> = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| {
                let c = s.content.as_ref();
                c.contains("image") || c.contains("a cat") || c.contains("cat.png")
            })
            .collect();
        for span in image_spans {
            assert_eq!(span.style.fg, Some(theme.muted));
        }
    }

    #[test]
    fn raw_html_block_renders_as_muted_text() {
        let theme = Theme::dark();
        let text = render_markdown("<div>hi</div>", &theme, 80);
        let joined: String = text
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("<div>hi</div>"));
        let span = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.as_ref().contains("<div>"))
            .expect("raw html span missing");
        assert_eq!(span.style.fg, Some(theme.muted));
    }

    #[test]
    fn inline_html_renders_as_muted_text() {
        let theme = Theme::dark();
        let text = render_markdown("line with <br> inside", &theme, 80);
        let joined: String = text
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("<br>"));
    }

    #[test]
    fn raw_html_inside_blockquote_gets_bar_prefix() {
        let theme = Theme::dark();
        let text = render_markdown("> <span>quoted html</span>", &theme, 80);
        let line = text
            .lines
            .iter()
            .find(|l| line_text(l).contains("<span>"))
            .expect("html line missing");
        let content = line_text(line);
        assert!(
            content.starts_with("▎ "),
            "expected blockquote bar prefix, got {content:?}"
        );
        assert!(content.contains("<span>quoted html</span>"));
    }

    #[test]
    fn raw_html_inside_list_item_gets_bullet_prefix() {
        let theme = Theme::dark();
        let text = render_markdown("- <span>inside list</span>", &theme, 80);
        let line = text
            .lines
            .iter()
            .find(|l| line_text(l).contains("<span>"))
            .expect("html line missing");
        let content = line_text(line);
        assert!(
            content.contains("• "),
            "expected bullet prefix, got {content:?}"
        );
        assert!(content.contains("<span>inside list</span>"));
    }

    #[test]
    fn gfm_table_header_is_bold_with_separator_row() {
        let theme = Theme::dark();
        let input = "| a | b |\n|---|---|\n| 1 | 2 |";
        let text = render_markdown(input, &theme, 80);
        let contents: Vec<String> = text.lines.iter().map(line_text).collect();
        // Header content should appear with pipe separators.
        assert!(
            contents
                .iter()
                .any(|c| c.contains("a") && c.contains("│") && c.contains("b"))
        );
        // Data row.
        assert!(
            contents
                .iter()
                .any(|c| c.contains("1") && c.contains("│") && c.contains("2"))
        );
        // A separator line of `─` below the header.
        assert!(
            contents
                .iter()
                .any(|c| !c.is_empty() && c.chars().all(|ch| ch == '─' || ch.is_whitespace()))
        );
        // Header cell spans are bold.
        let head_span = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.as_ref() == "a")
            .expect("header cell `a` missing");
        assert!(head_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn footnote_reference_and_definition_render_as_muted_labels() {
        let theme = Theme::dark();
        let input = "See[^1].\n\n[^1]: the note";
        let text = render_markdown(input, &theme, 80);
        let joined: String = text
            .lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("[^1]"));
        assert!(joined.contains("the note"));
        let ref_span = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.as_ref() == "[^1]")
            .expect("reference span missing");
        assert_eq!(ref_span.style.fg, Some(theme.muted));
    }
}
