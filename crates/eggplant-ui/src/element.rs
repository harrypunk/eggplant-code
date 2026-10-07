//! The declarative element tree — the only module that touches `Frame`.
//!
//! Components (see `crate::components`) are pure functions returning an
//! `Element` tree that *describes* the UI; [`paint`] interprets that
//! description onto the terminal frame. This is the React/Compose split:
//! components describe, the renderer draws (Rule 5: UI = f(state)).

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// A declarative UI description: *what* to draw, not *how*.
///
/// Built by components, consumed once by [`paint`]. The tree is fully owning
/// (`Line<'static>`), so no lifetime parameters leak into component or layer
/// signatures; components clone the few short strings they display. Ratatui's
/// `Line`/`Span`/`Style`/`Constraint`/`Rect` are reused as the vocabulary.
pub enum Element {
    /// Nothing to draw.
    Empty,
    /// Text lines (ratatui `Paragraph` equivalent).
    Text {
        lines: Vec<Line<'static>>,
        style: Style,
        wrap: bool,
    },
    /// Split the area by `constraints` and paint children into the splits.
    Layout {
        direction: Direction,
        constraints: Vec<Constraint>,
        children: Vec<Element>,
    },
    /// Border/title chrome around a child.
    Bordered {
        title: Option<Line<'static>>,
        border_style: Style,
        style: Style,
        child: Box<Element>,
    },
    /// Opaque background, then the child (floats, panels, toasts).
    Cleared(Box<Element>),
    /// Paint the child at an absolute rect (clamped to the parent area).
    Fixed { area: Rect, child: Box<Element> },
    /// Paint children into the same area, in order (bottom-up).
    Stack(Vec<Element>),
    /// Place the terminal cursor (absolute position; last one painted wins).
    Cursor(Position),
    /// A one-line text input: styled prompt + text, with the terminal
    /// cursor placed after the text. The renderer knows the rect — views
    /// never compute cursor coordinates.
    Input {
        prompt: Line<'static>,
        text: String,
        style: Style,
    },
    /// A vertical rule (│) filling its rect — a pane divider.
    VRule(Style),
}

impl Element {
    /// Plain text lines.
    pub fn text(lines: Vec<Line<'static>>) -> Self {
        Element::Text {
            lines,
            style: Style::default(),
            wrap: false,
        }
    }

    /// Paint `child` at an absolute rect.
    pub fn fixed(area: Rect, child: Element) -> Self {
        Element::Fixed {
            area,
            child: Box::new(child),
        }
    }

    /// Opaque background, then `child`.
    pub fn cleared(child: Element) -> Self {
        Element::Cleared(Box::new(child))
    }

    /// Border/title chrome around `child`.
    pub fn bordered(title: impl Into<Line<'static>>, border_style: Style, child: Element) -> Self {
        Element::Bordered {
            title: Some(title.into()),
            border_style,
            style: Style::default(),
            child: Box::new(child),
        }
    }

    /// Place the terminal cursor.
    pub fn cursor(x: u16, y: u16) -> Self {
        Element::Cursor(Position::new(x, y))
    }
}

/// Interpret an element tree onto the frame. The single place where
/// descriptions become terminal output.
pub fn paint(frame: &mut Frame, element: Element, area: Rect) {
    match element {
        Element::Empty => {}
        Element::Text { lines, style, wrap } => {
            let paragraph = Paragraph::new(lines).style(style);
            let paragraph = if wrap {
                paragraph.wrap(Wrap { trim: false })
            } else {
                paragraph
            };
            frame.render_widget(paragraph, area);
        }
        Element::Layout {
            direction,
            constraints,
            children,
        } => {
            let splits = Layout::default()
                .direction(direction)
                .constraints(constraints)
                .split(area);
            for (child, split) in children.into_iter().zip(splits.iter()) {
                paint(frame, child, *split);
            }
        }
        Element::Bordered {
            title,
            border_style,
            style,
            child,
        } => {
            let mut block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .style(style);
            if let Some(title) = title {
                block = block.title(title);
            }
            let inner = block.inner(area);
            frame.render_widget(block, area);
            paint(frame, *child, inner);
        }
        Element::Cleared(child) => {
            frame.render_widget(Clear, area);
            paint(frame, *child, area);
        }
        Element::Fixed { area: fixed, child } => {
            paint(frame, *child, fixed.intersection(area));
        }
        Element::Stack(children) => {
            for child in children {
                paint(frame, child, area);
            }
        }
        Element::Cursor(position) => frame.set_cursor_position(position),
        Element::Input {
            prompt,
            text,
            style,
        } => {
            let text_width = text.chars().count() as u16;
            let prompt_width = prompt.width() as u16;
            let mut spans = prompt.spans;
            spans.push(ratatui::text::Span::styled(text, style));
            frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
            // Cursor after the text, clamped inside the area.
            let x = (area.x + prompt_width + text_width).min(area.right().saturating_sub(1));
            if area.height > 0 && x >= area.x {
                frame.set_cursor_position(Position::new(x, area.y));
            }
        }
        Element::VRule(style) => {
            let buffer = frame.buffer_mut();
            for y in area.y..area.bottom() {
                if let Some(cell) = buffer.cell_mut((area.x, y)) {
                    cell.set_symbol("│").set_style(style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;

    fn render_to_string(element: Element, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                paint(frame, element, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn text_paints_lines() {
        let out = render_to_string(Element::text(vec![Line::from("hi")]), 5, 2);
        assert_eq!(out, "hi   \n     ");
    }

    #[test]
    fn bordered_wraps_child_with_frame() {
        let element = Element::bordered(
            " t ",
            Style::default().fg(Color::Cyan),
            Element::text(vec![Line::from("x")]),
        );
        let out = render_to_string(element, 6, 3);
        assert_eq!(out, "┌ t ─┐\n│x   │\n└────┘");
    }

    #[test]
    fn layout_splits_area_for_children() {
        let element = Element::Layout {
            direction: Direction::Horizontal,
            constraints: vec![Constraint::Length(2), Constraint::Min(1)],
            children: vec![
                Element::text(vec![Line::from("ab")]),
                Element::text(vec![Line::from("cd")]),
            ],
        };
        let out = render_to_string(element, 5, 1);
        assert_eq!(out, "abcd ");
    }

    #[test]
    fn fixed_positions_child_within_parent() {
        let element = Element::fixed(
            Rect::new(2, 1, 3, 1),
            Element::text(vec![Line::from(Span::raw("hey"))]),
        );
        let out = render_to_string(element, 8, 3);
        assert_eq!(out, "        \n  hey   \n        ");
    }

    #[test]
    fn paint_consumes_a_tree() {
        // Compile-level guarantee of one-shot semantics, plus a smoke test
        // that Stack paints in order (later children overwrite earlier ones).
        let element = Element::Stack(vec![
            Element::text(vec![Line::from("aaa")]),
            Element::fixed(Rect::new(0, 0, 1, 1), Element::text(vec![Line::from("b")])),
        ]);
        let out = render_to_string(element, 3, 1);
        assert_eq!(out, "baa");
    }
}
