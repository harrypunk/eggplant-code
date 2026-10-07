//! The editor text area: gutter + display rows + cursor.
//!
//! The view paints `DisplayRow`s (see docs/design/line-fitting.md): each
//! row is the char segment `[start_col, start_col + width)` of one document
//! line, so soft-wrap and horizontal scroll share this single path —
//! decorations are computed per char-column over the *full* line, then the
//! segment is sliced out.

use eggplant_core::HighlightedSpan;
use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// One visible document line: highlighted spans + optional visual selection
/// as char columns `[start, end)`, plus search marks `(start, end,
/// is_current)`.
pub struct EditorLine {
    pub spans: Vec<HighlightedSpan>,
    pub selection: Option<(usize, usize)>,
    pub search_marks: Vec<(usize, usize, bool)>,
    /// Leap labels on this line: `(col, label)` — painted as chips,
    /// replacing the char under them.
    pub labels: Vec<(usize, char)>,
}

/// What the gutter shows for a row (derived state, not computed here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMark {
    /// First row of a logical line: its 1-based number.
    Number(usize),
    /// Soft-wrap continuation row.
    Continuation,
    /// Past the file's end: vim-style `~`.
    PastEnd,
}

/// One screen row to paint (the view's projection of `DisplayRow`).
pub struct RowProps {
    pub line: EditorLine,
    /// Document line this row belongs to (continuation rows share their
    /// first row's line).
    pub doc_line: usize,
    /// First char column of the segment this row shows.
    pub start_col: usize,
    pub gutter: GutterMark,
}

/// Style every char of the line (per-column decorations: selection,
/// search marks, leap chips), returning `(char, style)` cells. Slicing
/// happens later — decorations must be computed on full-line columns.
fn style_cells(line: &EditorLine, dim: bool, theme: &Theme) -> Vec<(char, Style)> {
    let cells: Vec<(char, Option<eggplant_core::SyntaxScope>)> = line
        .spans
        .iter()
        .flat_map(|span| span.text.chars().map(move |c| (c, span.scope)))
        .collect();
    let selected = |col: usize| {
        line.selection
            .is_some_and(|(start, end)| (start..end).contains(&col))
    };
    let search_bg = |col: usize| {
        line.search_marks
            .iter()
            .find(|(start, end, _)| (*start..*end).contains(&col))
            .map(|(_, _, current)| {
                if *current {
                    theme.search_current
                } else {
                    theme.search_match
                }
            })
    };

    let label_style = Style::default()
        .fg(theme.bg)
        .bg(theme.accent)
        .add_modifier(ratatui::style::Modifier::BOLD);

    cells
        .iter()
        .enumerate()
        .map(|(col, (c, scope))| {
            // Leap labels replace the char under them (leap.nvim-style chip).
            if let Some((_, label)) = line.labels.iter().find(|(label_col, _)| *label_col == col) {
                return (*label, label_style);
            }
            // Leap mode dims all other text; search marks and selection are
            // superseded.
            let style = if dim {
                theme.scope_style(*scope).fg(theme.comment)
            } else {
                let mut style = theme.scope_style(*scope);
                if let Some(bg) = search_bg(col) {
                    style = style.bg(bg);
                }
                if selected(col) {
                    style = style.bg(theme.selection);
                }
                style
            };
            (*c, style)
        })
        .collect()
}

/// Group consecutive equal-styled cells into spans.
fn spans_from(cells: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    for (c, style) in cells {
        match spans.last_mut() {
            Some(last) if last.style == *style => last.content.to_mut().push(*c),
            _ => spans.push(Span::styled(c.to_string(), *style)),
        }
    }
    Line::from(spans)
}

/// Everything the editor view needs — nothing more.
pub struct EditorProps {
    /// Screen rows (already laid out by the viewport), in paint order.
    pub rows: Vec<RowProps>,
    /// User-counted document lines: drives gutter width.
    pub line_count: usize,
    /// Cursor as (line, col) in document coordinates.
    pub cursor: (usize, usize),
    /// Dim all text (leap-jump: only the labels stand out).
    pub dim: bool,
}

/// Gutter column width for a document of `line_count` lines. Shared by
/// the view and the editor surface (which needs the text width for
/// viewport sync) — one formula, one place.
pub fn gutter_width(line_count: usize) -> usize {
    line_count.max(1).ilog10() as usize + 2
}

pub fn view(props: &EditorProps, area: Rect, theme: &Theme) -> Element {
    let gutter_width = gutter_width(props.line_count);
    let text_width = (area.width as usize).saturating_sub(gutter_width).max(1);
    let (cursor_line, cursor_col) = props.cursor;
    let base = Style::default().fg(theme.fg).bg(theme.bg);

    let gutter: Vec<Line> = props
        .rows
        .iter()
        .map(|row| {
            let (text, style) = match row.gutter {
                GutterMark::Number(n) => {
                    let style = if n == cursor_line {
                        base.fg(theme.accent_alt)
                    } else {
                        base.fg(theme.comment)
                    };
                    (format!("{:>w$} ", n + 1, w = gutter_width - 1), style)
                }
                GutterMark::Continuation => (
                    format!("{:<w$} ", "↳", w = gutter_width - 1),
                    base.fg(theme.comment),
                ),
                GutterMark::PastEnd => (
                    format!("{:<w$} ", "~", w = gutter_width - 1),
                    base.fg(theme.comment),
                ),
            };
            Line::from(Span::styled(text, style))
        })
        .collect();

    let text: Vec<Line> = props
        .rows
        .iter()
        .map(|row| {
            let cells = style_cells(&row.line, props.dim, theme);
            spans_from(&slice_segment(&cells, row.start_col, text_width))
        })
        .collect();

    // The cursor sits on the row showing its (line, col) segment; sync
    // guaranteed one exists. Fallback (insert-mode EOL on an exact-fit
    // wrapped line): clamp into the line's last row.
    let cursor_row = props
        .rows
        .iter()
        .enumerate()
        .find(|(_, row)| row.line_spans_col(cursor_line, cursor_col, text_width))
        .or_else(|| {
            props
                .rows
                .iter()
                .enumerate()
                .rfind(|(_, row)| row.line_of() == Some(cursor_line))
        });
    let cursor = match cursor_row {
        Some((y, row)) if row.gutter != GutterMark::PastEnd => {
            let x = area.x
                + gutter_width as u16
                + cursor_col.saturating_sub(row.start_col).min(text_width - 1) as u16;
            let y = area.y + y as u16;
            if x < area.right() && y < area.bottom() {
                Element::cursor(x, y)
            } else {
                Element::Empty
            }
        }
        _ => Element::Empty,
    };

    let styled_text = |lines| Element::Text {
        lines,
        style: base,
        wrap: false,
    };
    Element::Stack(vec![
        Element::Layout {
            direction: Direction::Horizontal,
            constraints: vec![Constraint::Length(gutter_width as u16), Constraint::Min(1)],
            children: vec![styled_text(gutter), styled_text(text)],
        },
        cursor,
    ])
}

/// The `[start_col, start_col + width)` window of the line's cells.
fn slice_segment(cells: &[(char, Style)], start_col: usize, width: usize) -> Vec<(char, Style)> {
    cells.iter().skip(start_col).take(width).copied().collect()
}

impl RowProps {
    /// Document line this row belongs to (`None` past EOF).
    fn line_of(&self) -> Option<usize> {
        match self.gutter {
            GutterMark::PastEnd => None,
            _ => Some(self.doc_line),
        }
    }

    /// Does this row's segment contain (line, col)?
    fn line_spans_col(&self, line: usize, col: usize, width: usize) -> bool {
        self.gutter != GutterMark::PastEnd
            && self.doc_line == line
            && (self.start_col..self.start_col + width).contains(&col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggplant_core::SyntaxScope;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn paint(props: &EditorProps, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(props, area, &theme), area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(line: EditorLine, start_col: usize, gutter: GutterMark, doc_line: usize) -> RowProps {
        RowProps {
            line,
            doc_line,
            start_col,
            gutter,
        }
    }

    fn plain(text: &str) -> EditorLine {
        EditorLine {
            spans: vec![HighlightedSpan {
                text: text.to_owned(),
                scope: None,
            }],
            selection: None,
            search_marks: Vec::new(),
            labels: Vec::new(),
        }
    }

    #[test]
    fn visual_selection_paints_a_background_band() {
        let theme = Theme::default();
        let props = EditorProps {
            rows: vec![row(
                EditorLine {
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
                    search_marks: Vec::new(),
                    labels: Vec::new(),
                },
                0,
                GutterMark::Number(0),
                0,
            )],
            line_count: 1,
            cursor: (0, 0),
            dim: false,
        };
        let buffer = paint(&props, 20, 3);
        let gutter = 2u16; // " 1" + space: ilog10(1) + 2
        assert_eq!(buffer[(gutter, 0)].bg, theme.bg); // 'f' unselected
        assert_eq!(buffer[(gutter + 1, 0)].bg, theme.bg); // 'n' unselected
        for x in gutter + 2..gutter + 5 {
            assert_eq!(buffer[(x, 0)].bg, theme.selection, "col {x} selected");
        }
        // syntax fg survives under the selection bg
        assert_eq!(buffer[(gutter + 3, 0)].fg, theme.syntax.function);
    }

    #[test]
    fn rows_beyond_eof_render_tilde_without_a_number() {
        let props = EditorProps {
            rows: vec![
                row(plain(""), 0, GutterMark::Number(0), 0),
                row(plain(""), 0, GutterMark::Number(1), 1),
                row(plain(""), 0, GutterMark::PastEnd, 2),
                row(plain(""), 0, GutterMark::PastEnd, 3),
            ],
            line_count: 2, // a 2-line file in a 4-row viewport
            cursor: (1, 0),
            dim: false,
        };
        let buffer = paint(&props, 20, 4);
        assert_eq!(buffer[(0, 0)].symbol(), "1"); // numbered
        assert_eq!(buffer[(0, 1)].symbol(), "2"); // numbered
        assert_eq!(buffer[(0, 2)].symbol(), "~"); // past EOF
        assert_eq!(buffer[(0, 3)].symbol(), "~");
    }

    #[test]
    fn leap_dims_text_and_paints_label_chips() {
        let theme = Theme::default();
        let props = EditorProps {
            rows: vec![row(
                EditorLine {
                    spans: vec![HighlightedSpan {
                        text: "foo foo".to_owned(),
                        scope: None,
                    }],
                    selection: None,
                    search_marks: Vec::new(),
                    labels: vec![(4, 'a')],
                },
                0,
                GutterMark::Number(0),
                0,
            )],
            line_count: 1,
            cursor: (0, 0),
            dim: true,
        };
        let buffer = paint(&props, 20, 3);
        let gutter = 2u16;
        // unlabeled text is dimmed
        assert_eq!(buffer[(gutter, 0)].fg, theme.comment);
        // the label chip replaces the char at col 4 ('f' → 'a')
        let chip = &buffer[(gutter + 4, 0)];
        assert_eq!(chip.symbol(), "a");
        assert_eq!(chip.fg, theme.bg);
        assert_eq!(chip.bg, theme.accent);
    }

    #[test]
    fn search_marks_paint_backgrounds_current_distinct() {
        let theme = Theme::default();
        let props = EditorProps {
            rows: vec![row(
                EditorLine {
                    spans: vec![HighlightedSpan {
                        text: "foo foo".to_owned(),
                        scope: None,
                    }],
                    selection: None,
                    search_marks: vec![(0, 3, false), (4, 7, true)],
                    labels: Vec::new(),
                },
                0,
                GutterMark::Number(0),
                0,
            )],
            line_count: 1,
            cursor: (0, 4),
            dim: false,
        };
        let buffer = paint(&props, 20, 3);
        let gutter = 2u16;
        for x in gutter..gutter + 3 {
            assert_eq!(buffer[(x, 0)].bg, theme.search_match, "col {x} plain match");
        }
        assert_eq!(buffer[(gutter + 3, 0)].bg, theme.bg, "space unmarked");
        for x in gutter + 4..gutter + 7 {
            assert_eq!(
                buffer[(x, 0)].bg,
                theme.search_current,
                "col {x} current match"
            );
        }
    }

    #[test]
    fn horizontal_scroll_shows_the_offset_segment() {
        let props = EditorProps {
            rows: vec![row(plain("0123456789abcdef"), 6, GutterMark::Number(0), 0)],
            line_count: 1,
            cursor: (0, 6),
            dim: false,
        };
        let buffer = paint(&props, 12, 1);
        let gutter = 2u16;
        let text: String = (gutter..12)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect();
        assert!(
            text.starts_with("6789abc"),
            "segment from col 6, got '{text}'"
        );
    }

    #[test]
    fn wrap_continuation_rows_repeat_the_line_with_a_marker() {
        let props = EditorProps {
            rows: vec![
                row(plain("0123456789abcdef"), 0, GutterMark::Number(0), 0),
                row(plain("0123456789abcdef"), 10, GutterMark::Continuation, 0),
            ],
            line_count: 1,
            cursor: (0, 12),
            dim: false,
        };
        let buffer = paint(&props, 12, 2);
        let gutter = 2u16;
        let first: String = (gutter..12)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect();
        let second: String = (gutter..12)
            .map(|x| buffer[(x, 1)].symbol().to_owned())
            .collect();
        assert!(
            first.starts_with("0123456789"),
            "first segment, got '{first}'"
        );
        assert!(
            second.starts_with("abcdef"),
            "second segment, got '{second}'"
        );
        assert_eq!(buffer[(0, 1)].symbol(), "↳", "continuation marker");
    }

    #[test]
    fn decorations_survive_slicing_across_the_cut() {
        let theme = Theme::default();
        // Selection covers cols 8..12; the segment starts at 10 — the
        // selection must still paint from the segment's first cell.
        let mut line = plain("0123456789abcdef");
        line.selection = Some((8, 12));
        let props = EditorProps {
            rows: vec![row(line, 10, GutterMark::Continuation, 0)],
            line_count: 1,
            cursor: (0, 10),
            dim: false,
        };
        let buffer = paint(&props, 12, 1);
        let gutter = 2u16;
        assert_eq!(buffer[(gutter, 0)].bg, theme.selection, "col 10 selected");
        assert_eq!(
            buffer[(gutter + 1, 0)].bg,
            theme.selection,
            "col 11 selected"
        );
        assert_eq!(buffer[(gutter + 2, 0)].bg, theme.bg, "col 12 unselected");
    }
}
