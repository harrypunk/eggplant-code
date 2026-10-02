//! The command palette: input row + filtered command list, hugging the top.

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;

/// One command, projected for display (pre-filtered by the container).
pub struct PaletteItem {
    pub id: &'static str,
    pub description: &'static str,
}

/// Everything the palette needs — nothing more.
pub struct PaletteProps<'a> {
    pub input: &'a str,
    pub items: Vec<PaletteItem>,
    pub selected: usize,
}

/// Display cap; the container pre-filters, this caps the rendered rows.
pub const MAX_ROWS: u16 = 8;

pub fn view(props: &PaletteProps, area: Rect) -> Element<'static> {
    // Centered horizontally, hugging the top of the body area.
    let width = (area.width * 3 / 5).max(30).min(area.width);
    let height = (MAX_ROWS + 3).min(area.height); // input + rows + borders
    let frame_area = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + 1,
        width,
        height,
    };

    let input_row = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(props.input.to_owned()),
    ]);
    // Cursor after the input, clamped inside the border.
    let cursor_x =
        (frame_area.x + 3 + props.input.len() as u16).min(frame_area.right().saturating_sub(2));

    let rows: Vec<Line> = props
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == props.selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(format!(" {:<20}", item.id), style),
                Span::styled(item.description, style.fg(Color::Gray)),
            ])
        })
        .collect();

    Element::fixed(
        frame_area,
        Element::cleared(Element::Stack(vec![
            Element::Bordered {
                title: Some(Line::from(" palette ")),
                border_style: Style::default(),
                style: Style::default().bg(Color::Black),
                child: Box::new(Element::Layout {
                    direction: Direction::Vertical,
                    constraints: vec![Constraint::Length(1), Constraint::Min(1)],
                    children: vec![Element::text(vec![input_row]), Element::text(rows)],
                }),
            },
            Element::cursor(cursor_x, frame_area.y + 1),
        ])),
    )
}
