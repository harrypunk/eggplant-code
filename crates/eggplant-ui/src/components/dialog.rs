//! Floating dialogs: a centered message box and its yes/no confirm variant.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;

use crate::element::Element;
use crate::theme::Theme;

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
fn float_box(title: &str, lines: Vec<Line<'static>>, area: Rect, theme: &Theme) -> Element {
    Element::fixed(
        centered(area, 50, 30),
        Element::cleared(Element::Bordered {
            title: Some(Line::from(format!(" {title} "))),
            border_style: Style::default().fg(theme.comment),
            style: Style::default().fg(theme.fg).bg(theme.surface),
            child: Box::new(Element::Text {
                lines,
                style: Style::default(),
                wrap: true,
            }),
        }),
    )
}

/// Simple message dialog body.
pub fn dialog_view(title: &str, body: &str, area: Rect, theme: &Theme) -> Element {
    float_box(
        title,
        body.lines().map(|l| Line::from(l.to_owned())).collect(),
        area,
        theme,
    )
}

/// Yes/no confirm dialog body (`message` + key hints).
pub fn confirm_view(title: &str, message: &str, area: Rect, theme: &Theme) -> Element {
    let body = format!("{message}\n\n[y] yes   [n] no");
    float_box(
        title,
        body.lines().map(|l| Line::from(l.to_owned())).collect(),
        area,
        theme,
    )
}
