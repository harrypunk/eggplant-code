//! The global statusline: mode, document, focus hint, cursor position.

use eggplant_core::Mode;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// Everything the statusline needs — nothing more.
pub struct StatuslineProps {
    pub mode: Mode,
    /// Current buffer name; `None` when no buffer is open.
    pub buffer_name: Option<String>,
    pub modified: bool,
    /// Cursor (line, col) + total lines; `None` when no buffer is open.
    pub position: Option<(usize, usize, usize)>,
    /// Pending normal-mode input, vim `showcmd` style (`d`, `d2`, `5`).
    pub pending: Option<String>,
    /// Focused layer id, shown as a tag when it isn't the base editor.
    pub focused_layer: Option<&'static str>,
}

pub fn view(props: &StatuslineProps, area: Rect, sheet: &Stylesheet) -> Element {
    let mode_class = match props.mode {
        Mode::Normal => StyleClass::ModeNormal,
        Mode::Insert => StyleClass::ModeInsert,
        Mode::Visual | Mode::VisualLine => StyleClass::ModeVisual,
    };
    let modified = if props.modified { " [+]" } else { "" };
    let name = props.buffer_name.clone().unwrap_or_default();
    let focus_tag = match props.focused_layer {
        Some(id) if id != "editor" => format!(" ‹{id}›"),
        _ => String::new(),
    };
    let right = props.position.map_or(String::new(), |(line, col, total)| {
        format!(" {}:{}/{} ", line + 1, col + 1, total)
    });
    let pending = props
        .pending
        .as_ref()
        .map_or(String::new(), |hint| format!("{hint}  "));

    let left_width =
        2 + props.mode.as_str().len() + 1 + name.len() + modified.len() + focus_tag.len();
    let padding = (area.width as usize).saturating_sub(left_width + pending.len() + right.len());

    let name_span = if props.buffer_name.is_some() {
        format!(" {name}{modified}")
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::styled(format!(" {} ", props.mode), sheet.style(mode_class)),
        Span::raw(name_span),
        Span::styled(focus_tag, sheet.style(StyleClass::AccentAlt)),
        Span::raw(" ".repeat(padding)),
        Span::styled(pending, sheet.emphasized(StyleClass::AccentAlt)),
        Span::raw(right),
    ]);

    Element::Text {
        lines: vec![line],
        style: sheet.style(StyleClass::Bar),
        wrap: false,
    }
}
