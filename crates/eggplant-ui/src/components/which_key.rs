//! The which-key component: a helix-style popup — a bordered vertical
//! list anchored bottom-right, titled with the prefix breadcrumb.
//!
//! Pure component: the container projects the available keys into
//! `WhichKeyProps`; layout is declarative (spacers push the popup into
//! the corner; cassowary owns the rects).

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// One available key: the char to press + what it does.
pub struct KeyHint {
    pub key: char,
    pub description: &'static str,
}

/// Everything the which-key popup needs — nothing more.
pub struct WhichKeyProps {
    /// The prefix path so far (e.g. "SPC" or "SPC f") — the box title.
    pub path: String,
    pub hints: Vec<KeyHint>,
}

/// Margin from the screen's bottom/right edges.
const MARGIN: u16 = 1;

pub fn view(props: &WhichKeyProps, area: Rect, sheet: &Stylesheet) -> Element {
    let key_style = sheet.emphasized(StyleClass::Accent);
    let rows: Vec<Line> = props
        .hints
        .iter()
        .map(|hint| {
            Line::from(vec![
                Span::styled(format!(" {} ", hint.key), key_style),
                Span::styled(hint.description, sheet.style(StyleClass::Text)),
            ])
        })
        .collect();

    // Content-sized box (widest row, all entries), clamped to the screen.
    let content_width = props
        .hints
        .iter()
        .map(|hint| 4 + hint.description.len())
        .max()
        .unwrap_or(0) as u16;
    let width = (content_width + 2).min(area.width.saturating_sub(MARGIN * 2));
    let height = (rows.len() as u16 + 2).min(area.height.saturating_sub(MARGIN * 2));

    let popup = Element::cleared(Element::Bordered {
        title: Some(Line::from(format!(" {} ", props.path))),
        border_style: sheet.style(StyleClass::Muted),
        style: sheet.style(StyleClass::Surface),
        child: Box::new(Element::text(rows)),
    });

    // Bottom-right anchor, declaratively: spacers take the slack.
    Element::Layout {
        direction: Direction::Vertical,
        constraints: vec![
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Length(MARGIN),
        ],
        children: vec![
            Element::Empty,
            Element::Layout {
                direction: Direction::Horizontal,
                constraints: vec![
                    Constraint::Min(0),
                    Constraint::Length(width),
                    Constraint::Length(MARGIN),
                ],
                children: vec![Element::Empty, popup, Element::Empty],
            },
            Element::Empty,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn paint(props: &WhichKeyProps, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(props, area, &Stylesheet::new(&theme)), area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn props() -> WhichKeyProps {
        WhichKeyProps {
            path: "SPC".to_owned(),
            hints: vec![
                KeyHint {
                    key: 'f',
                    description: "+file",
                },
                KeyHint {
                    key: 'b',
                    description: "+buffer",
                },
                KeyHint {
                    key: 'q',
                    description: "quit",
                },
            ],
        }
    }

    #[test]
    fn popup_is_a_vertical_list_anchored_bottom_right() {
        let buffer = paint(&props(), 60, 20);
        let row_text = |y: u16| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        // Vertical: each hint on its own row, keys in one column.
        let rows: Vec<u16> = (0..20)
            .filter(|&y| {
                row_text(y).contains("+file")
                    || row_text(y).contains("+buffer")
                    || row_text(y).contains("quit")
            })
            .collect();
        assert_eq!(rows.len(), 3, "three hint rows");
        assert!(
            rows.windows(2).all(|w| w[1] == w[0] + 1),
            "rows are consecutive"
        );
        let key_col = |y: u16| row_text(y).find(|c: char| c.is_alphabetic() && "fbq".contains(c));
        assert!(
            key_col(rows[0]).is_some_and(|c| c == key_col(rows[1]).unwrap()),
            "keys align in one column"
        );
        // Anchored bottom-right: content near the edges, not top-left.
        assert!(rows[2] >= 15, "bottom-anchored, last row at {}", rows[2]);
        assert!(
            row_text(rows[0]).contains("+file"),
            "row order top to bottom"
        );
    }

    #[test]
    fn breadcrumb_is_the_box_title() {
        let buffer = paint(&props(), 60, 20);
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains(" SPC "), "path shown as the popup title");
    }

    #[test]
    fn clamps_to_tiny_screens() {
        let mut props = props();
        props.hints = (b'a'..=b'z')
            .map(|c| KeyHint {
                key: c as char,
                description: "entry",
            })
            .collect();
        let buffer = paint(&props, 30, 10);
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains('a'), "top rows painted");
        // No panic, no overflow — the box never exceeds the area.
    }
}
