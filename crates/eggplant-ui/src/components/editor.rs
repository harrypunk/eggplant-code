//! The editor text area: gutter + document lines + cursor.

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;

/// Everything the editor view needs — nothing more.
pub struct EditorProps {
    /// Visible document lines (already sliced to the viewport).
    pub lines: Vec<String>,
    /// First visible line, 0-based (gutter numbering base).
    pub scroll: usize,
    /// Total document lines (drives gutter width).
    pub line_count: usize,
    /// Cursor as (line, col) in document coordinates.
    pub cursor: (usize, usize),
}

pub fn view(props: &EditorProps, area: Rect) -> Element {
    let gutter_width = props.line_count.max(1).ilog10() as u16 + 2;
    let (cursor_line, cursor_col) = props.cursor;

    let gutter: Vec<Line> = (props.scroll..props.scroll + props.lines.len())
        .map(|n| {
            let style = if n == cursor_line {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(
                format!("{:>w$} ", n + 1, w = gutter_width as usize - 1),
                style,
            ))
        })
        .collect();
    let text: Vec<Line> = props.lines.iter().cloned().map(Line::from).collect();

    // Terminal cursor tracks the editor cursor (char-col ≈ display col for now).
    let cursor_x = area.x + gutter_width + cursor_col as u16;
    let cursor_y = area.y + cursor_line.saturating_sub(props.scroll) as u16;
    let cursor = if cursor_x < area.right() && cursor_y < area.bottom() {
        Element::cursor(cursor_x, cursor_y)
    } else {
        Element::Empty
    };

    Element::Stack(vec![
        Element::Layout {
            direction: Direction::Horizontal,
            constraints: vec![Constraint::Length(gutter_width), Constraint::Min(1)],
            children: vec![Element::text(gutter), Element::text(text)],
        },
        cursor,
    ])
}
