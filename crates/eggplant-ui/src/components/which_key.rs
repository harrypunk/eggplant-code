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

pub fn view(props: &WhichKeyProps, area: Rect, theme: &Theme) -> Element {
    let bar_area = Rect {
        height: 1,
        y: area.bottom().saturating_sub(1),
        ..area
    };

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

    Element::fixed(
        bar_area,
        Element::cleared(Element::Text {
            lines: vec![Line::from(spans)],
            style: Style::default().fg(theme.fg).bg(theme.surface),
            wrap: false,
        }),
    )
}
