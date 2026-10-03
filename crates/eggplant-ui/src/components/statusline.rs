//! The global statusline: mode, document, focus hint, cursor position.

use eggplant_core::Mode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;

/// Everything the statusline needs — nothing more.
pub struct StatuslineProps {
    pub mode: Mode,
    pub buffer_name: String,
    pub modified: bool,
    /// (current index, total) for the buffer indicator.
    pub buffers: (usize, usize),
    /// Cursor as (line, col) in document coordinates.
    pub cursor: (usize, usize),
    pub line_count: usize,
    /// Focused layer id, shown as a tag when it isn't the base editor.
    pub focused_layer: Option<&'static str>,
}

pub fn view(props: &StatuslineProps, area: Rect) -> Element {
    let mode_bg = match props.mode {
        Mode::Normal => Color::Cyan,
        Mode::Insert => Color::Green,
    };
    let modified = if props.modified { " [+]" } else { "" };
    let buffers = if props.buffers.1 > 1 {
        format!(" ({}/{})", props.buffers.0 + 1, props.buffers.1)
    } else {
        String::new()
    };
    let focus_tag = match props.focused_layer {
        Some(id) if id != "editor" => format!(" ‹{id}›"),
        _ => String::new(),
    };
    let right = format!(
        " {}:{}/{} ",
        props.cursor.0 + 1,
        props.cursor.1 + 1,
        props.line_count
    );

    let left_width = 2
        + props.mode.as_str().len()
        + 1
        + props.buffer_name.len()
        + modified.len()
        + focus_tag.len();
    let padding = (area.width as usize).saturating_sub(left_width + right.len());

    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", props.mode),
            Style::default()
                .fg(Color::Black)
                .bg(mode_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {}{modified}{buffers}", props.buffer_name)),
        Span::styled(focus_tag, Style::default().fg(Color::Yellow)),
        Span::raw(" ".repeat(padding)),
        Span::raw(right),
    ]);

    Element::Text {
        lines: vec![line],
        style: Style::default().bg(Color::DarkGray),
        wrap: false,
    }
}
