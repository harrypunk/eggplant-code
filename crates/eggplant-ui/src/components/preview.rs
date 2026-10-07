//! The preview pane: a numbered context window around a match, with the
//! focus row banded and the match columns highlighted (see
//! docs/design/live-grep.md). Pure props → Element.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// Everything the preview needs — nothing more.
pub use eggplant_core::grep::ContextWindow as PreviewProps;

pub fn view(props: &PreviewProps, _area: Rect, theme: &Theme) -> Element {
    let base = Style::default().fg(theme.fg).bg(theme.bg);
    let number_width = (props.first_line + props.lines.len()).max(1).ilog10() as usize + 1;

    let rows: Vec<Line> = props
        .lines
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let focused = i == props.focus_row;
            let number_style = if focused {
                base.fg(theme.accent_alt)
            } else {
                base.fg(theme.comment)
            };
            let mut spans = vec![Span::styled(
                format!(" {:>w$} │ ", props.first_line + i + 1, w = number_width),
                number_style,
            )];
            spans.extend(body_spans(text, focused, props.focus_cols, theme, base));
            Line::from(spans)
        })
        .collect();

    // Borderless: the picker's outer border + divider are the pane
    // chrome; the title is a header row above the context lines.
    Element::Layout {
        direction: ratatui::layout::Direction::Vertical,
        constraints: vec![
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(1),
        ],
        children: vec![
            Element::Text {
                lines: vec![Line::from(Span::styled(
                    format!(" {} ", props.title),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(ratatui::style::Modifier::BOLD),
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

/// The row's text: plain, or banded (focus row) with the match columns
/// highlighted on top.
fn body_spans(
    text: &str,
    focused: bool,
    focus_cols: (usize, usize),
    theme: &Theme,
    base: Style,
) -> Vec<Span<'static>> {
    if !focused {
        return vec![Span::styled(text.to_owned(), base.fg(theme.comment))];
    }
    let band = base.bg(theme.selection);
    let (start, end) = focus_cols;
    text.chars()
        .enumerate()
        .map(|(col, c)| {
            let style = if (start..end).contains(&col) {
                band.bg(theme.search_current)
            } else {
                band
            };
            (c, style)
        })
        .fold(Vec::new(), |mut spans: Vec<Span>, (c, style)| {
            match spans.last_mut() {
                Some(last) if last.style == style => last.content.to_mut().push(c),
                _ => spans.push(Span::styled(c.to_string(), style)),
            }
            spans
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn focus_row_is_banded_and_match_cols_highlighted() {
        let theme = Theme::default();
        let props = PreviewProps {
            title: "a.rs:2".to_owned(),
            first_line: 0,
            lines: vec![
                "before".to_owned(),
                "the match here".to_owned(),
                "after".to_owned(),
            ],
            focus_row: 1,
            focus_cols: (4, 9),
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(&props, area, &theme), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // Header row at y=0; context rows start at y=1 (focus row y=2).
        // Gutter " 2 │ " is 5 cells wide; match cols 4..9 start at x=5+4.
        let gutter = 5;
        assert_eq!(buffer[(gutter, 2)].bg, theme.selection, "focus row banded");
        for x in gutter + 4..gutter + 9 {
            assert_eq!(buffer[(x, 2)].bg, theme.search_current, "col {x} matched");
        }
        assert_ne!(
            buffer[(gutter, 1)].bg,
            theme.selection,
            "context row not banded"
        );
    }
}
