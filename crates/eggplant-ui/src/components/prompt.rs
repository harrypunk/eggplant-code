//! The search prompt: a single input row pinned to the bottom (vim `/`).

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// Everything the prompt needs — nothing more.
pub struct PromptProps {
    /// Leading label ("/" for search).
    pub label: &'static str,
    pub input: String,
}

pub fn view(props: &PromptProps, area: Rect, theme: &Theme) -> Element {
    let row = Rect {
        x: area.x,
        y: area.bottom().saturating_sub(1),
        width: area.width,
        height: 1,
    };
    let base = Style::default().fg(theme.fg).bg(theme.surface);
    let line = Line::from(vec![
        Span::styled(
            props.label,
            Style::default().fg(theme.accent).bg(theme.surface),
        ),
        Span::styled(props.input.clone(), base),
    ]);
    let cursor_x = row.x + props.label.len() as u16 + props.input.chars().count() as u16;
    Element::Fixed {
        area: row,
        child: Box::new(Element::Stack(vec![
            Element::Text {
                lines: vec![line],
                style: base,
                wrap: false,
            },
            Element::cursor(cursor_x, row.y),
        ])),
    }
}
