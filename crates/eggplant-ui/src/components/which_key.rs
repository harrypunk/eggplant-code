//! The which-key component: a one-row hint bar at the bottom of the screen
//! showing the keys available under the current prefix.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// One available key: the char to press + what it does.
pub struct KeyHint {
    pub key: char,
    pub description: &'static str,
}

/// Everything the which-key bar needs — nothing more.
pub struct WhichKeyProps {
    /// The prefix path so far (e.g. "SPC" or "SPC f").
    pub path: String,
    pub hints: Vec<KeyHint>,
}

pub fn view(props: &WhichKeyProps, _area: Rect, theme: &Theme) -> Element {
    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled(
            format!(" {} ", props.path),
            Style::default().fg(theme.accent_alt),
        ),
        Span::raw("→ "),
    ];
    for hint in &props.hints {
        spans.push(Span::styled(format!("{} ", hint.key), key_style));
        spans.push(Span::styled(
            format!("{}   ", hint.description),
            Style::default().fg(theme.comment),
        ));
    }

    // Bottom bar, declaratively: spacer above, one row of hints.
    Element::Layout {
        direction: ratatui::layout::Direction::Vertical,
        constraints: vec![
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
        ],
        children: vec![
            Element::Empty,
            Element::cleared(Element::Text {
                lines: vec![Line::from(spans)],
                style: Style::default().fg(theme.fg).bg(theme.surface),
                wrap: false,
            }),
        ],
    }
}
