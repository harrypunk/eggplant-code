//! Markdown → styled lines: pulldown-cmark events folded through a
//! style stack into ratatui `Line`s. Used by the chat (assistant replies
//! are markdown); pure, so any component can render markdown.
//!
//! Scope: headings, bold/italic, inline code + fenced blocks, lists,
//! quotes, rules, links. Tables are NOT enabled — pipes pass through as
//! literal text rather than half-rendering.

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::stylesheet::{StyleClass, Stylesheet};

/// Render markdown text to styled lines (the chat component's view of an
/// assistant message).
pub fn markdown_lines(text: &str, sheet: &Stylesheet) -> Vec<Line<'static>> {
    let mut renderer = Renderer::new(sheet);
    for event in Parser::new(text) {
        renderer.event(event);
    }
    renderer.finish()
}

struct Renderer<'a> {
    sheet: &'a Stylesheet<'a>,
    lines: Vec<Line<'static>>,
    /// Spans of the line being built.
    current: Vec<Span<'static>>,
    /// Inline style stack (strong/emphasis/link push, their ends pop).
    styles: Vec<Style>,
    in_code_block: bool,
    /// List nesting: None = bullet, Some(n) = next ordered number.
    lists: Vec<Option<u64>>,
    quote_depth: usize,
}

impl<'a> Renderer<'a> {
    fn new(sheet: &'a Stylesheet<'a>) -> Self {
        Self {
            sheet,
            lines: Vec::new(),
            current: Vec::new(),
            styles: vec![sheet.style(StyleClass::Text)],
            in_code_block: false,
            lists: Vec::new(),
            quote_depth: 0,
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            // Inline code: its own fill so it reads as code.
            Event::Code(code) => {
                self.push_span(code.into_string(), self.sheet.style(StyleClass::Code))
            }
            Event::SoftBreak | Event::HardBreak => self.flush_line(),
            Event::Rule => {
                self.flush_line();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    self.sheet.style(StyleClass::Muted),
                )));
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_paragraph();
                let style = self.sheet.emphasized(StyleClass::Accent);
                self.styles.push(style);
                // Keep a marker so heading structure stays visible.
                let marks = "#".repeat(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
                self.push_span(format!("{marks} "), style);
            }
            Tag::Strong => {
                let style = self.top().add_modifier(Modifier::BOLD);
                self.styles.push(style);
            }
            Tag::Emphasis => {
                let style = self.top().add_modifier(Modifier::ITALIC);
                self.styles.push(style);
            }
            Tag::CodeBlock(_) => {
                self.flush_paragraph();
                self.in_code_block = true;
            }
            Tag::List(start) => {
                self.flush_paragraph();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_line();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    // Ordered list: this item's number, then advance.
                    Some(Some(n)) => {
                        let marker = format!("{n}. ");
                        *n += 1;
                        marker
                    }
                    _ => "• ".to_string(),
                };
                let indent = "  ".repeat(depth);
                self.push_span(
                    format!("{indent}{marker}"),
                    self.sheet.style(StyleClass::Muted),
                );
            }
            Tag::BlockQuote(_) => {
                self.flush_paragraph();
                self.quote_depth += 1;
            }
            Tag::Link { .. } => {
                let style = self.top().add_modifier(Modifier::UNDERLINED);
                self.styles.push(style);
            }
            Tag::Paragraph => {}
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.styles.pop();
                self.flush_paragraph();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.flush_paragraph();
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.flush_paragraph();
            }
            TagEnd::Item => self.flush_line(),
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.flush_paragraph();
            }
            TagEnd::Paragraph => self.flush_paragraph(),
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_code_block {
            // Code block text arrives as whole chunks with newlines:
            // each source line becomes its own filled line.
            let style = self.sheet.style(StyleClass::Code);
            let mut rest = text;
            while let Some(pos) = rest.find('\n') {
                self.push_span(rest[..pos].to_string(), style);
                self.flush_line();
                rest = &rest[pos + 1..];
            }
            if !rest.is_empty() {
                self.push_span(rest.to_string(), style);
            }
        } else {
            let style = self.top();
            self.push_span(text.to_string(), style);
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        // Trim blank edges (blocks leave trailing empty lines).
        while self.lines.last().is_some_and(|l| l.spans.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }

    // ---- primitives ----

    fn top(&self) -> Style {
        *self.styles.last().unwrap_or(&Style::default())
    }

    fn push_span(&mut self, text: String, style: Style) {
        if !text.is_empty() {
            self.current.push(Span::styled(text, style));
        }
    }

    /// End the current line (soft/hard break, list item boundary).
    fn flush_line(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let mut spans = Vec::new();
        for _ in 0..self.quote_depth {
            spans.push(Span::styled("▌ ", self.sheet.style(StyleClass::Muted)));
        }
        spans.append(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    /// End a block: flush the line and separate from what follows.
    fn flush_paragraph(&mut self) {
        self.flush_line();
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(md: &str) -> Vec<Line<'static>> {
        let theme = crate::theme::Theme::default();
        let sheet = Stylesheet::new(&theme);
        markdown_lines(md, &sheet)
    }

    fn plain(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn bold_italic_code_get_modifiers() {
        let lines = render("**bold** *it* `code`");
        let spans = &lines[0].spans;
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[0].content.as_ref(), "bold");
        assert!(spans[2].style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(spans[4].content.as_ref(), "code");
    }

    #[test]
    fn heading_keeps_marker_and_accent() {
        let lines = render("## Title");
        assert_eq!(plain(&lines[0]), "## Title");
    }

    #[test]
    fn code_block_lines_are_filled() {
        let lines = render("```\nlet a = 1;\nlet b = 2;\n```");
        assert_eq!(plain(&lines[0]), "let a = 1;");
        assert_eq!(plain(&lines[1]), "let b = 2;");
    }

    #[test]
    fn lists_bullet_and_number() {
        let lines = render("- a\n- b\n\n1. x\n2. y");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert!(texts.iter().any(|t| t == "• a"));
        assert!(texts.iter().any(|t| t == "1. x"));
        assert!(texts.iter().any(|t| t == "2. y"));
    }

    #[test]
    fn quote_gets_bar_prefix() {
        let lines = render("> quoted");
        assert!(plain(&lines[0]).starts_with('▌'));
    }
}
