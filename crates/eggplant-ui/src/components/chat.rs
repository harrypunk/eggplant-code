//! The chat view: transcript + input, pure. Shared by the modal popup
//! and the right-side panel — one session, two presentations, one
//! component (docs/design/agent.md).

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::agent::ChatItem;
use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

pub struct ChatProps<'a> {
    pub title: &'a str,
    pub items: &'a [ChatItem],
    pub input: &'a str,
    /// A run is in flight (spinner hint in the title).
    pub running: bool,
    /// Lines scrolled up from the bottom (chat semantics: 0 = tail).
    pub scroll: usize,
}

/// The transcript projected to styled lines: `> user`, assistant text,
/// `⚙ tool ✓/✗/…` chips, blank line between blocks.
fn transcript_lines(items: &[ChatItem], sheet: &Stylesheet) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for item in items {
        match item {
            ChatItem::User(text) => {
                lines.push(Line::from(Span::styled(
                    format!("> {text}"),
                    sheet.style(StyleClass::Accent),
                )));
            }
            ChatItem::Assistant(text) => {
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_owned(),
                        sheet.style(StyleClass::Text),
                    )));
                }
            }
            ChatItem::Tool {
                summary, is_error, ..
            } => {
                let (mark, class) = match is_error {
                    None => ("…", StyleClass::Muted),
                    Some(false) => ("✓", StyleClass::Info),
                    Some(true) => ("✗", StyleClass::Error),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("⚙ {summary} "), sheet.style(StyleClass::Muted)),
                    Span::styled(mark, sheet.style(class)),
                ]));
            }
        }
        lines.push(Line::default());
    }
    lines
}

pub fn view(props: &ChatProps, area: Rect, sheet: &Stylesheet) -> Element {
    let lines = transcript_lines(props.items, sheet);
    // Chat semantics: show the tail unless scrolled up. Slicing is by
    // logical lines (wrap affects display only) — v1 keeps it simple.
    let visible = area.height.saturating_sub(4) as usize; // borders + input
    let end = lines.len().saturating_sub(props.scroll);
    let start = end.saturating_sub(visible);
    let window: Vec<Line<'static>> = lines[start..end].to_vec();

    let title = if props.running {
        format!(" {} ● ", props.title)
    } else {
        format!(" {} ", props.title)
    };
    Element::Bordered {
        title: Some(Line::from(title)),
        border_style: sheet.style(StyleClass::Muted),
        style: sheet.style(StyleClass::Surface),
        child: Box::new(Element::Layout {
            direction: Direction::Vertical,
            constraints: vec![Constraint::Min(1), Constraint::Length(1)],
            children: vec![
                Element::Text {
                    lines: window,
                    style: Style::default(),
                    wrap: true,
                },
                Element::Input {
                    prompt: Line::from(Span::styled("❯ ", sheet.style(StyleClass::Accent))),
                    text: props.input.to_owned(),
                    style: sheet.style(StyleClass::Text),
                },
            ],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn paint(props: &ChatProps, width: u16, height: u16) -> String {
        let theme = Theme::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(props, area, &Stylesheet::new(&theme)), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn props_of(items: &[ChatItem], scroll: usize) -> ChatProps<'_> {
        ChatProps {
            title: "agent",
            items,
            input: "",
            running: false,
            scroll,
        }
    }

    #[test]
    fn transcript_renders_blocks_and_tool_marks() {
        let items = vec![
            ChatItem::User("fix the bug".into()),
            ChatItem::Tool {
                id: "1".into(),
                summary: "edit main.rs".into(),
                is_error: Some(false),
            },
            ChatItem::Assistant("done".into()),
        ];
        let out = paint(&props_of(&items, 0), 40, 10);
        assert!(out.contains("> fix the bug"), "{out}");
        assert!(out.contains("⚙ edit main.rs ✓"), "{out}");
        assert!(out.contains("done"), "{out}");
    }

    #[test]
    fn scroll_shows_earlier_lines() {
        let items: Vec<ChatItem> = (0..20)
            .map(|i| ChatItem::Assistant(format!("line {i}")))
            .collect();
        let tail = paint(&props_of(&items, 0), 30, 8);
        assert!(tail.contains("line 19"), "{tail}");
        // 40 transcript lines (text + blank per item); scroll 38 → the top.
        let scrolled = paint(&props_of(&items, 38), 30, 8);
        assert!(scrolled.contains("line 0"), "{scrolled}");
        assert!(!scrolled.contains("line 19"), "{scrolled}");
    }
}
