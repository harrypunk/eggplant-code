//! The preview pane: a peek into a file around a match — numbered
//! context lines painted through the SAME styling path as the editor
//! (`components::editor::style_cells`), so previews get real syntax
//! highlighting for free. Pure props → Element.

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::text::{Line, Span};

use eggplant_core::HighlightedSpan;

use crate::components::editor::{EditorLine, spans_from, style_cells};
use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// One context row: highlighted spans (from a core `Peek`) plus the
/// match band on the focus row.
pub struct PreviewRow {
    pub spans: Vec<HighlightedSpan>,
    /// `(start, end, is_current)` — same shape the editor consumes.
    pub search_marks: Vec<(usize, usize, bool)>,
}

/// Everything the preview needs — nothing more.
pub struct PreviewProps {
    /// Header ("src/config.rs:12").
    pub title: String,
    /// 0-based line number of `rows[0]` (gutter numbering base).
    pub first_line: usize,
    pub rows: Vec<PreviewRow>,
    /// Index into `rows` of the hit row (its number is accented).
    pub focus_row: usize,
}

pub fn view(props: &PreviewProps, _area: Rect, sheet: &Stylesheet) -> Element {
    let base = sheet.style(StyleClass::Text);
    let number_width = (props.first_line + props.rows.len()).max(1).ilog10() as usize + 1;

    let rows: Vec<Line> = props
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let number_style = if i == props.focus_row {
                base.patch(sheet.style(StyleClass::AccentAlt))
            } else {
                base.patch(sheet.style(StyleClass::Muted))
            };
            let mut spans = vec![Span::styled(
                format!(" {:>w$} │ ", props.first_line + i + 1, w = number_width),
                number_style,
            )];
            // The editor's own styling pipeline — syntax colors, match band.
            let cells = style_cells(
                &EditorLine {
                    spans: row.spans.clone(),
                    selection: None,
                    search_marks: row.search_marks.clone(),
                    labels: Vec::new(),
                },
                false,
                sheet,
            );
            spans.extend(spans_from(&cells).spans);
            Line::from(spans)
        })
        .collect();

    // Borderless: the picker's outer border + divider are the pane
    // chrome; the title is a header row above the context lines.
    Element::Layout {
        direction: Direction::Vertical,
        constraints: vec![Constraint::Length(1), Constraint::Min(1)],
        children: vec![
            Element::Text {
                lines: vec![Line::from(Span::styled(
                    format!(" {} ", props.title),
                    sheet.style(StyleClass::Title),
                ))],
                style: base,
                wrap: false,
            },
            Element::Text {
                lines: rows,
                style: base,
                wrap: false,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use eggplant_core::SyntaxScope;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn syntax_scopes_and_match_columns_paint() {
        let theme = Theme::default();
        let props = PreviewProps {
            title: "a.rs:2".to_owned(),
            first_line: 0,
            rows: vec![
                PreviewRow {
                    spans: vec![HighlightedSpan {
                        text: "before".to_owned(),
                        scope: Some(SyntaxScope::Comment),
                    }],
                    search_marks: Vec::new(),
                },
                PreviewRow {
                    spans: vec![HighlightedSpan {
                        text: "the match here".to_owned(),
                        scope: Some(SyntaxScope::String),
                    }],
                    search_marks: vec![(4, 9, true)],
                },
            ],
            focus_row: 1,
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(&props, area, &Stylesheet::new(&theme)), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // Header at y=0; rows start at y=1 (focus row y=2). Gutter
        // " 2 │ " is 5 cells; match cols 4..9 start at x=5+4.
        let gutter = 5u16;
        // Syntax colors come through the editor's scope mapping.
        assert_eq!(buffer[(gutter, 1)].fg, theme.comment, "comment scope");
        assert_eq!(buffer[(gutter, 2)].fg, theme.syntax.string, "string scope");
        // The match band paints on the focus row only.
        for x in gutter + 4..gutter + 9 {
            assert_eq!(buffer[(x, 2)].bg, theme.search_current, "col {x} matched");
        }
        assert_eq!(buffer[(gutter, 1)].bg, theme.bg, "context row unmarked");
        // The focus row's line number is accented.
        assert_eq!(buffer[(1, 2)].fg, theme.accent_alt);
    }
}
