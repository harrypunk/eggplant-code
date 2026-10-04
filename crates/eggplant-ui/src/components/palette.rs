//! The command palette: input row + filtered command list, hugging the top.

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// One command, projected for display (pre-filtered by the container).
pub struct PaletteItem {
    pub id: &'static str,
    pub description: &'static str,
}

/// Everything the palette needs — nothing more.
pub struct PaletteProps {
    pub input: String,
    pub items: Vec<PaletteItem>,
    pub selected: usize,
}

/// Display cap; the container pre-filters, this caps the rendered rows.
pub const MAX_ROWS: u16 = 8;

pub fn view(props: &PaletteProps, area: Rect, theme: &Theme) -> Element {
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
        Span::styled("> ", Style::default().fg(theme.accent)),
        Span::raw(props.input.clone()),
    ]);
    // Cursor after the input, clamped inside the border.
    let cursor_x =
        (frame_area.x + 3 + props.input.len() as u16).min(frame_area.right().saturating_sub(2));

    let rows: Vec<Line> = props
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            // Contrast-safe on both plain and selected rows.
            let (id_style, desc_style) = if i == props.selected {
                (
                    Style::default()
                        .fg(theme.fg)
                        .bg(theme.selection)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(theme.fg).bg(theme.selection),
                )
            } else {
                (
                    Style::default().fg(theme.fg),
                    Style::default().fg(theme.comment),
                )
            };
            Line::from(vec![
                Span::styled(format!(" {:<20}", item.id), id_style),
                Span::styled(item.description, desc_style),
            ])
        })
        .collect();

    Element::fixed(
        frame_area,
        Element::cleared(Element::Stack(vec![
            Element::Bordered {
                title: Some(Line::from(" palette ")),
                border_style: Style::default().fg(theme.accent),
                style: Style::default().fg(theme.fg).bg(theme.surface),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn selected_row_is_contrast_safe() {
        // Regression: selected description used to be Gray on DarkGray.
        let theme = Theme::default();
        let props = PaletteProps {
            input: String::new(),
            items: vec![PaletteItem {
                id: "app.quit",
                description: "Quit",
            }],
            selected: 0,
        };
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(&props, area, &theme), area);
            })
            .unwrap();
        // Palette: width 36 centered in 60 → x=12; border → inner x=13.
        // Row 0 is at y=3 (frame y=1, border, input row); the description
        // starts after " {:<20}" → x=13+21=34.
        let cell = &terminal.backend().buffer()[(34, 3)];
        assert_eq!(cell.bg, theme.selection);
        assert_eq!(cell.fg, theme.fg);
    }
}
