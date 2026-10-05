//! The editor text area: gutter + document lines + cursor.

use eggplant_core::HighlightedSpan;
use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// One visible document line: highlighted spans + optional visual selection
/// as char columns `[start, end)`.
pub struct EditorLine {
    pub spans: Vec<HighlightedSpan>,
    pub selection: Option<(usize, usize)>,
}

/// Build a display line: syntax colors, with the visual selection painted
/// as a background band (splitting spans at the selection boundaries).
fn style_line(line: &EditorLine, theme: &Theme) -> Line<'static> {
    // Flatten to per-char (char, scope) cells, then group consecutive cells
    // with equal style into spans. Visible lines are short — clarity wins.
    let cells: Vec<(char, Option<eggplant_core::SyntaxScope>)> = line
        .spans
        .iter()
        .flat_map(|span| span.text.chars().map(move |c| (c, span.scope)))
        .collect();
    let selected = |col: usize| {
        line.selection
            .is_some_and(|(start, end)| (start..end).contains(&col))
    };

    let mut spans: Vec<Span> = Vec::new();
    for (col, (c, scope)) in cells.iter().enumerate() {
        let mut style = theme.scope_style(*scope);
        if selected(col) {
            style = style.bg(theme.selection);
        }
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push(*c),
            _ => spans.push(Span::styled(c.to_string(), style)),
        }
    }
    Line::from(spans)
}

/// Everything the editor view needs — nothing more.
pub struct EditorProps {
    /// Visible document lines (sliced to the viewport).
    pub lines: Vec<EditorLine>,
    /// First visible line, 0-based (gutter numbering base).
    pub scroll: usize,
    /// Total document lines (drives gutter width).
    pub line_count: usize,
    /// Cursor as (line, col) in document coordinates.
    pub cursor: (usize, usize),
}

pub fn view(props: &EditorProps, area: Rect, theme: &Theme) -> Element {
    let gutter_width = props.line_count.max(1).ilog10() as u16 + 2;
    let (cursor_line, cursor_col) = props.cursor;
    let base = Style::default().fg(theme.fg).bg(theme.bg);

    let gutter: Vec<Line> = (props.scroll..props.scroll + props.lines.len())
        .map(|n| {
            let style = if n == cursor_line {
                base.fg(theme.accent_alt)
            } else {
                base.fg(theme.comment)
            };
            Line::from(Span::styled(
                format!("{:>w$} ", n + 1, w = gutter_width as usize - 1),
                style,
            ))
        })
        .collect();
    let text: Vec<Line> = props
        .lines
        .iter()
        .map(|line| style_line(line, theme))
        .collect();

    // Terminal cursor tracks the editor cursor (char-col ≈ display col for now).
    let cursor_x = area.x + gutter_width + cursor_col as u16;
    let cursor_y = area.y + cursor_line.saturating_sub(props.scroll) as u16;
    let cursor = if cursor_x < area.right() && cursor_y < area.bottom() {
        Element::cursor(cursor_x, cursor_y)
    } else {
        Element::Empty
    };

    let styled_text = |lines| Element::Text {
        lines,
        style: base,
        wrap: false,
    };
    Element::Stack(vec![
        Element::Layout {
            direction: Direction::Horizontal,
            constraints: vec![Constraint::Length(gutter_width), Constraint::Min(1)],
            children: vec![styled_text(gutter), styled_text(text)],
        },
        cursor,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggplant_core::SyntaxScope;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn visual_selection_paints_a_background_band() {
        let theme = Theme::default();
        let props = EditorProps {
            lines: vec![EditorLine {
                spans: vec![
                    HighlightedSpan {
                        text: "fn ".to_owned(),
                        scope: Some(SyntaxScope::Keyword),
                    },
                    HighlightedSpan {
                        text: "main".to_owned(),
                        scope: Some(SyntaxScope::Function),
                    },
                ],
                selection: Some((2, 5)), // covers " m a i" — splits both spans
            }],
            scroll: 0,
            line_count: 1,
            cursor: (0, 0),
        };
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(&props, area, &theme), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let gutter = 2u16; // " 1" + space: ilog10(1) + 2
        assert_eq!(buffer[(gutter, 0)].bg, theme.bg); // 'f' unselected
        assert_eq!(buffer[(gutter + 1, 0)].bg, theme.bg); // 'n' unselected
        for x in gutter + 2..gutter + 5 {
            assert_eq!(buffer[(x, 0)].bg, theme.selection, "col {x} selected");
        }
        // syntax fg survives under the selection bg
        assert_eq!(buffer[(gutter + 3, 0)].fg, theme.syntax.function);
    }
}
