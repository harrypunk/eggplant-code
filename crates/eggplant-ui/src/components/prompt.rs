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
    let _ = area;
    let base = Style::default().fg(theme.fg).bg(theme.surface);
    // Bottom row, declaratively: spacer takes everything above it.
    Element::Layout {
        direction: ratatui::layout::Direction::Vertical,
        constraints: vec![
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
        ],
        children: vec![
            Element::Empty,
            Element::cleared(Element::Input {
                prompt: Line::from(Span::styled(props.label, Style::default().fg(theme.accent))),
                text: props.input.clone(),
                style: base,
            }),
        ],
    }
}
