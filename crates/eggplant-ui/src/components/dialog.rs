//! Floating dialogs: a centered message box and its yes/no confirm variant.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;

use crate::element::Element;

/// Centered rect of `percent_x`/`percent_y` within `area`.
fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let [_, vertical, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);
    let [_, horizontal, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(vertical);
    horizontal
}

/// An opaque, centered floating box with a title.
fn float_box<'a>(title: &'a str, lines: Vec<Line<'a>>, area: Rect) -> Element<'a> {
    Element::fixed(
        centered(area, 50, 30),
        Element::cleared(Element::Bordered {
            title: Some(Line::from(format!(" {title} "))),
            border_style: Style::default(),
            style: Style::default().bg(Color::Black),
            child: Box::new(Element::Text {
                lines,
                style: Style::default(),
                wrap: true,
            }),
        }),
    )
}

/// Simple message dialog body.
pub fn dialog_view<'a>(title: &'a str, body: &'a str, area: Rect) -> Element<'a> {
    float_box(title, body.lines().map(Line::from).collect(), area)
}

/// Yes/no confirm dialog body (`message` + key hints).
pub fn confirm_view<'a>(title: &'a str, message: &str, area: Rect) -> Element<'a> {
    let body = format!("{message}\n\n[y] yes   [n] no");
    float_box(
        title,
        body.lines().map(|l| Line::from(l.to_owned())).collect(),
        area,
    )
}
