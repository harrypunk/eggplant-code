//! The generic picker: input row + filtered item list, hugging the top;
//! with a preview, a taller split panel (list │ preview). Commands
//! palette, buffer grep, project grep, … are all pickers over different
//! items.
//!
//! Layout is fully declarative: percentage spacers center/size the panel
//! (cassowary does the math — no computed rects), the cursor rides along
//! with `Element::Input`.

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::components::preview::{self, PreviewProps};
use crate::element::Element;
use crate::theme::Theme;

/// One item, projected for display (pre-filtered by the container).
pub struct PickerItem {
    /// Leading, fixed-width column (command id, line number, …).
    pub primary: String,
    /// Free-form text after it (description, line text, …).
    pub secondary: String,
}

/// Everything the picker needs — nothing more.
pub struct PickerProps<'a> {
    /// Frame title ("palette", "grep", …).
    pub title: &'static str,
    pub input: String,
    pub items: Vec<PickerItem>,
    pub selected: usize,
    /// Materialized preview of the selected item (project grep); the
    /// panel becomes a taller split when present.
    pub preview: Option<&'a PreviewProps>,
}

/// Rows shown in the compact (preview-less) float; with a preview the
/// panel is a percentage of the screen instead.
pub const MAX_ROWS: u16 = 8;

/// Center `panel` with percentage spacers (top-hugging) — the declarative
/// replacement for computed frame rects.
fn centered(panel: Element, horizontal: [u16; 3], vertical: Vec<Constraint>) -> Element {
    let [left, middle, right] = horizontal;
    Element::Layout {
        direction: Direction::Vertical,
        constraints: vertical,
        children: vec![
            Element::Empty,
            Element::Layout {
                direction: Direction::Horizontal,
                constraints: vec![
                    Constraint::Percentage(left),
                    Constraint::Percentage(middle),
                    Constraint::Percentage(right),
                ],
                children: vec![Element::Empty, panel, Element::Empty],
            },
            Element::Empty,
        ],
    }
}

pub fn view(props: &PickerProps, area: Rect, theme: &Theme) -> Element {
    let base = Style::default().fg(theme.fg).bg(theme.surface);
    let input = Element::Input {
        prompt: Line::from(Span::styled("> ", Style::default().fg(theme.accent))),
        text: props.input.clone(),
        style: base,
    };

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
                Span::styled(format!(" {:<20}", item.primary), id_style),
                Span::styled(item.secondary.clone(), desc_style),
            ])
        })
        .collect();

    let list = Element::Layout {
        direction: Direction::Vertical,
        constraints: vec![Constraint::Length(1), Constraint::Min(1)],
        children: vec![
            input,
            Element::Text {
                lines: rows,
                style: base,
                wrap: false,
            },
        ],
    };

    // One panel: outer border, panes divided by a vertical rule.
    let body = match props.preview {
        Some(preview) => Element::Layout {
            direction: Direction::Horizontal,
            constraints: vec![
                Constraint::Percentage(40),
                Constraint::Length(1),
                Constraint::Min(1),
            ],
            children: vec![
                list,
                Element::VRule(Style::default().fg(theme.comment)),
                preview::view(preview, area, theme),
            ],
        },
        None => list,
    };

    let panel = Element::cleared(Element::Bordered {
        title: Some(Line::from(format!(" {} ", props.title))),
        border_style: Style::default().fg(theme.accent),
        style: base,
        child: Box::new(body),
    });

    if props.preview.is_some() {
        centered(
            panel,
            [10, 80, 10],
            vec![
                Constraint::Length(1),
                Constraint::Percentage(55),
                Constraint::Min(0),
            ],
        )
    } else {
        centered(
            panel,
            [20, 60, 20],
            vec![
                Constraint::Length(1),
                Constraint::Length(MAX_ROWS + 3),
                Constraint::Min(0),
            ],
        )
    }
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
        let props = PickerProps {
            title: "palette",
            input: String::new(),
            items: vec![PickerItem {
                primary: "app.quit".to_owned(),
                secondary: "Quit".to_owned(),
            }],
            selected: 0,
            preview: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(&props, area, &theme), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let cell = buffer
            .content
            .iter()
            .find(|cell| cell.symbol() == "Q")
            .expect("description rendered");
        assert_eq!(cell.bg, theme.selection);
        assert_eq!(cell.fg, theme.fg);
    }

    #[test]
    fn preview_splits_the_panel_with_a_divider() {
        let theme = Theme::default();
        let preview = PreviewProps {
            title: "a.rs:1".to_owned(),
            first_line: 0,
            lines: vec!["hit".to_owned()],
            focus_row: 0,
            focus_cols: (0, 3),
        };
        let props = PickerProps {
            title: "project grep",
            input: "hit".to_owned(),
            items: vec![PickerItem {
                primary: "a.rs:1".to_owned(),
                secondary: "hit".to_owned(),
            }],
            selected: 0,
            preview: Some(&preview),
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(&props, area, &theme), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer.content.iter().any(|cell| cell.symbol() == "│"),
            "a divider separates list and preview"
        );
        // The preview title renders on the right side.
        let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(text.contains("a.rs:1"), "preview title shown");
    }
}
