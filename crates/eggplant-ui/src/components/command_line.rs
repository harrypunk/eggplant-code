//! The `:` command line — a one-line input at the bottom of the screen.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;

pub fn view(input: &str, area: Rect) -> Element {
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
            Element::text(vec![Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Cyan)),
                Span::raw(input.to_owned()),
            ])]),
            Element::cursor(cursor_x, line_area.y),
        ])),
    )
}
