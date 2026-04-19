//! Generate SVG screenshot of duru TUI with demo data.
//! Usage: cargo run --example screenshot > screenshot.svg

// The example imports all src modules via `#[path]` but only exercises a subset,
// so dead_code warnings are expected and suppressed.
#![allow(dead_code)]

use ratatui::{Terminal, backend::TestBackend, style::Color};

// Access internal modules via the binary crate's public API
// Since we can't import from a binary crate, we duplicate the minimal needed code
// This is intentional — the example is self-contained

#[path = "../src/app.rs"]
mod app;
#[path = "../src/markdown.rs"]
mod markdown;
#[path = "../src/registry.rs"]
mod registry;
#[path = "../src/scan.rs"]
mod scan;
#[path = "../src/sessions.rs"]
mod sessions;
#[path = "../src/theme.rs"]
mod theme;
#[path = "../src/ui.rs"]
mod ui;

fn main() {
    let projects = scan::demo_projects();
    let theme = theme::Theme::dark();

    // Simulate navigating to a project with files and preview
    let mut app = app::App::new(projects);
    // Select "my-webapp" (index 2: GLOBAL=0, design-system=1, my-webapp=2)
    app.project_index = 2;
    app.file_index = 0;
    app.focus = app::Pane::Files;
    app.load_content();

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| ui::render(frame, &app, &theme))
        .unwrap();

    let buffer = terminal.backend().buffer().clone();
    print!("{}", buffer_to_svg(&buffer, 120, 30, &theme));
}

fn color_to_hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Reset => "#191724".to_string(), // Rosé Pine base
        _ => "#191724".to_string(),
    }
}

fn buffer_to_svg(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    height: u16,
    theme: &theme::Theme,
) -> String {
    let char_w: f64 = 8.4;
    let char_h: f64 = 17.0;
    let pad: f64 = 16.0;
    let corner: f64 = 10.0;

    let svg_w = (width as f64) * char_w + pad * 2.0;
    let svg_h = (height as f64) * char_h + pad * 2.0 + 32.0; // extra for title bar

    let base_hex = color_to_hex(theme.base);

    let title_offset = pad + 32.0;
    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{svg_w}\" height=\"{svg_h}\" viewBox=\"0 0 {svg_w} {svg_h}\">\n"
    ));
    svg.push_str(&format!(
        "<rect width=\"{svg_w}\" height=\"{svg_h}\" rx=\"{corner}\" fill=\"{base_hex}\"/>\n"
    ));
    svg.push_str(&format!("<g transform=\"translate({pad}, 12)\">\n"));
    svg.push_str("  <circle cx=\"8\" cy=\"8\" r=\"5.5\" fill=\"#eb6f92\"/>\n");
    svg.push_str("  <circle cx=\"26\" cy=\"8\" r=\"5.5\" fill=\"#f6c177\"/>\n");
    svg.push_str("  <circle cx=\"44\" cy=\"8\" r=\"5.5\" fill=\"#31748f\"/>\n");
    svg.push_str("</g>\n");
    svg.push_str(&format!(
        "<g transform=\"translate({pad}, {title_offset})\" font-family=\"SF Mono,Menlo,Monaco,Consolas,monospace\" font-size=\"13\">\n"
    ));

    // Render cells
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            let symbol = cell.symbol();
            if symbol == " " || symbol.is_empty() {
                // Only render background if different from base
                let bg = color_to_hex(cell.bg);
                if bg != base_hex && cell.bg != Color::Reset {
                    svg.push_str(&format!(
                        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
                        (x as f64) * char_w,
                        (y as f64) * char_h,
                        char_w + 0.5,
                        char_h,
                        bg
                    ));
                }
                continue;
            }

            let fg = color_to_hex(cell.fg);
            let bg = color_to_hex(cell.bg);

            // Background rect if not default
            if bg != base_hex && cell.bg != Color::Reset {
                svg.push_str(&format!(
                    r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
                    (x as f64) * char_w,
                    (y as f64) * char_h,
                    char_w + 0.5,
                    char_h,
                    bg
                ));
            }

            // Text
            let escaped = symbol
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");

            let bold = if cell.modifier.contains(ratatui::style::Modifier::BOLD) {
                r#" font-weight="bold""#
            } else {
                ""
            };

            svg.push_str(&format!(
                r#"<text x="{}" y="{}" fill="{}"{bold}>{escaped}</text>"#,
                (x as f64) * char_w,
                (y as f64) * char_h + char_h * 0.75,
                fg
            ));
        }
    }

    svg.push_str("</g>\n</svg>\n");
    svg
}
