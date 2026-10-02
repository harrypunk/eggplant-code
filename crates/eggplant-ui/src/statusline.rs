//! The global statusline: mode, document, focus hint, cursor position.

use eggplant_core::Mode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;

/// Render the statusline. `focused_layer` is the focused layer's id, shown as
/// a tag when it isn't the base editor.
pub fn render(frame: &mut Frame, area: Rect, app: &App, focused_layer: Option<&'static str>) {
    let mode = app.editor.mode();
    let mode_bg = match mode {
        Mode::Normal => Color::Cyan,
        Mode::Insert => Color::Green,
    };
    let modified = if app.editor.is_modified() { " [+]" } else { "" };
    let buffers = if app.editor.buffer_count() > 1 {
        format!(
            " ({}/{})",
            app.editor.current_buffer() + 1,
            app.editor.buffer_count()
        )
    } else {
        String::new()
    };
    let focus_tag = match focused_layer {
        Some(id) if id != "editor" => format!(" ‹{id}›"),
        _ => String::new(),
    };
    let (line, col) = app.editor.cursor();
    let right = format!(" {}:{}/{} ", line + 1, col + 1, app.editor.line_count());

    let left_width = 2
        + mode.as_str().len()
        + 1
        + app.editor.display_name().len()
        + modified.len()
        + focus_tag.len();
    let padding = (area.width as usize).saturating_sub(left_width + right.len());

    let status = Line::from(vec![
        Span::styled(
            format!(" {mode} "),
            Style::default()
                .fg(Color::Black)
                .bg(mode_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {}{modified}{buffers}", app.editor.display_name())),
        Span::styled(focus_tag, Style::default().fg(Color::Yellow)),
        Span::raw(" ".repeat(padding)),
        Span::raw(right),
    ]);
    frame.render_widget(
        Paragraph::new(status).style(Style::default().bg(Color::DarkGray)),
        area,
    );
}
