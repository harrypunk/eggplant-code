//! The chat view: transcript + input, pure. Shared by the modal popup
//! and the right-side panel — one session, two presentations, one
//! component (docs/design/agent.md).

use eggplant_core::SnippetHighlighter;
use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::agent::ChatItem;
use crate::components::markdown::markdown_lines;
use crate::element::{Element, Lines};
use crate::stylesheet::{StyleClass, Stylesheet};

pub struct ChatProps<'a> {
    pub title: &'a str,
    pub items: &'a [ChatItem],
    /// Syntax highlighting for fenced code blocks — the one capability
    /// the chat needs, not the whole editor facade.
    pub highlighter: &'a dyn SnippetHighlighter,
    pub input: &'a str,
    /// A run is in flight (spinner hint in the title).
    pub running: bool,
    /// Lines scrolled up from the bottom (chat semantics: 0 = tail).
    pub scroll: usize,
    /// Only the focused view places the terminal cursor — an unfocused
    /// chat panel must not steal the editor's cursor (paint order is
    /// z-order: panels paint after the base, so last cursor wins).
    pub focused: bool,
}

/// The transcript projected to styled lines: `> user`, assistant text,
/// `⚙ tool ✓/✗/…` chips, blank line between blocks.
fn transcript_lines(
    items: &[ChatItem],
    sheet: &Stylesheet,
    highlighter: &dyn SnippetHighlighter,
) -> Lines {
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
                // Assistant replies are markdown — render it styled.
                lines.extend(markdown_lines(text, sheet, Some(highlighter)));
            }
            ChatItem::Thinking(text) => {
                let style = sheet
                    .style(StyleClass::Muted)
                    .add_modifier(ratatui::style::Modifier::ITALIC);
                lines.push(Line::from(Span::styled("✱ thinking", style)));
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(line.to_owned(), style)));
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
    let lines = transcript_lines(props.items, sheet, props.highlighter);
    // Chat semantics: show the tail unless scrolled up. Slicing is by
    // logical lines (wrap affects display only) — v1 keeps it simple.
    let visible = area.height.saturating_sub(4) as usize; // borders + input
    let end = lines.len().saturating_sub(props.scroll);
    let start = end.saturating_sub(visible);
    let window: Lines = lines[start..end].to_vec();

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
                if props.focused {
                    Element::Input {
                        prompt: Line::from(Span::styled("❯ ", sheet.style(StyleClass::Accent))),
                        text: props.input.to_owned(),
                        style: sheet.style(StyleClass::Text),
                    }
                } else {
                    // Same visuals, no cursor placement.
                    Element::Text {
                        lines: vec![Line::from(vec![
                            Span::styled("❯ ", sheet.style(StyleClass::Accent)),
                            Span::styled(props.input.to_owned(), sheet.style(StyleClass::Text)),
                        ])],
                        style: Style::default(),
                        wrap: false,
                    }
                },
            ],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use eggplant_core::Editor;
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

    fn props_of<'a>(items: &'a [ChatItem], scroll: usize, editor: &'a Editor) -> ChatProps<'a> {
        ChatProps {
            title: "agent",
            items,
            highlighter: editor,
            input: "",
            running: false,
            scroll,
            focused: true,
        }
    }

    #[test]
    fn transcript_renders_blocks_and_tool_marks() {
        let editor = Editor::scratch().unwrap();
        let items = vec![
            ChatItem::User("fix the bug".into()),
            ChatItem::Tool {
                id: "1".into(),
                summary: "edit main.rs".into(),
                is_error: Some(false),
            },
            ChatItem::Assistant("done".into()),
        ];
        let out = paint(&props_of(&items, 0, &editor), 40, 10);
        assert!(out.contains("> fix the bug"), "{out}");
        assert!(out.contains("⚙ edit main.rs ✓"), "{out}");
        assert!(out.contains("done"), "{out}");
    }

    #[test]
    fn unfocused_view_places_no_cursor() {
        // Only the focused layer may place the terminal cursor — panels
        // paint after the editor, so a cursor here would steal focus
        // visually (the bug this regression-tests).
        fn has_input(el: &Element) -> bool {
            match el {
                Element::Input { .. } | Element::Cursor(_) => true,
                Element::Layout { children, .. } | Element::Stack(children) => {
                    children.iter().any(has_input)
                }
                Element::Bordered { child, .. }
                | Element::Cleared(child)
                | Element::Fixed { child, .. } => has_input(child),
                _ => false,
            }
        }
        let editor = Editor::scratch().unwrap();
        let theme = Theme::default();
        let sheet = Stylesheet::new(&theme);
        let mut props = props_of(&[], 0, &editor);
        props.focused = false;
        let tree = view(&props, Rect::new(0, 0, 40, 10), &sheet);
        assert!(!has_input(&tree), "unfocused chat must not place a cursor");
        props.focused = true;
        let tree = view(&props, Rect::new(0, 0, 40, 10), &sheet);
        assert!(has_input(&tree), "focused chat owns the cursor");
    }

    #[test]
    fn scroll_shows_earlier_lines() {
        let editor = Editor::scratch().unwrap();
        let items: Vec<ChatItem> = (0..20)
            .map(|i| ChatItem::Assistant(format!("line {i}")))
            .collect();
        let tail = paint(&props_of(&items, 0, &editor), 30, 8);
        assert!(tail.contains("line 19"), "{tail}");
        // 40 transcript lines (text + blank per item); scroll 38 → the top.
        let scrolled = paint(&props_of(&items, 38, &editor), 30, 8);
        assert!(scrolled.contains("line 0"), "{scrolled}");
        assert!(!scrolled.contains("line 19"), "{scrolled}");
    }
}
