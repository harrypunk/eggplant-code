//! The search prompt: a single input row pinned to the bottom (vim `/`).

use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// Everything the prompt needs — nothing more.
pub struct PromptProps {
    /// Leading label ("/" for search).
    pub label: &'static str,
    pub input: String,
}

pub fn view(props: &PromptProps, area: Rect, sheet: &Stylesheet) -> Element {
    let _ = area;
    let base = sheet.style(StyleClass::Surface);
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
                prompt: Line::from(sheet.span(StyleClass::Accent, props.label)),
                text: props.input.clone(),
                style: base,
            }),
        ],
    }
}
