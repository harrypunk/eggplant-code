//! The `:` command line — a one-line input at the bottom of the screen.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

pub fn view(input: &str, area: Rect, theme: &Theme) -> Element {
    // One line at the very bottom of the body area.
    let line_area = Rect {
        height: 1,
        y: area.bottom().saturating_sub(1),
        ..area
    };
    // Clamp the cursor inside the line even when input overflows.
    let cursor_x = (line_area.x + 1 + input.len() as u16).min(line_area.right().saturating_sub(1));

    Element::fixed(
        line_area,
        Element::cleared(Element::Stack(vec![
            Element::Text {
                lines: vec![Line::from(vec![
                    Span::styled(":", Style::default().fg(theme.accent)),
                    Span::raw(input.to_owned()),
                ])],
                style: Style::default().fg(theme.fg).bg(theme.bg),
                wrap: false,
            },
            Element::cursor(cursor_x, line_area.y),
        ])),
    )
}
