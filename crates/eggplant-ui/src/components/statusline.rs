//! The global statusline: mode, document, focus hint, cursor position.

use eggplant_core::Mode;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

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

pub fn view(props: &StatuslineProps, area: Rect, theme: &Theme) -> Element {
    let mode_bg = match props.mode {
        Mode::Normal => theme.mode_normal,
        Mode::Insert => theme.mode_insert,
        Mode::Visual => theme.accent_alt,
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
        Span::styled(
            format!(" {} ", props.mode),
            Style::default()
                .fg(theme.bg)
                .bg(mode_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(name_span),
        Span::styled(focus_tag, Style::default().fg(theme.accent_alt)),
        Span::raw(" ".repeat(padding)),
        Span::styled(
            pending,
            Style::default()
                .fg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(right),
    ]);

    Element::Text {
        lines: vec![line],
        style: Style::default().fg(theme.fg).bg(theme.statusline),
        wrap: false,
    }
}
