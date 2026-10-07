//! Peek: a read-only document for previewing (grep hits, future file
//! previews). A peek is NOT a buffer — it never enters the buffer list,
//! has no cursor, no history, no editing. It exists so previews reuse the
//! exact rendering path of the editor (language detection + tree-sitter
//! spans) instead of a parallel plain-text one.

use std::path::Path;

use helix_core::syntax;
use helix_view::Document;

use crate::highlight::{self, HighlightedSpan};

/// A read-only, syntax-highlightable view of a file.
pub struct Peek {
    doc: Document,
    loader: std::sync::Arc<arc_swap::ArcSwap<syntax::Loader>>,
}

impl Peek {
    pub(crate) fn new(
        doc: Document,
        loader: std::sync::Arc<arc_swap::ArcSwap<syntax::Loader>>,
    ) -> Self {
        Self { doc, loader }
    }

    /// Rope line count (includes the phantom trailing line, like the
    /// facade's `line_count`).
    pub fn line_count(&self) -> usize {
        self.doc.text().len_lines()
    }

    /// A line's content without the line ending.
    pub fn line(&self, line: usize) -> String {
        let text = self.doc.text();
        if line >= text.len_lines() {
            return String::new();
        }
        text.line(line)
            .chars()
            .take_while(|c| *c != '\n' && *c != '\r')
            .collect()
    }

    /// Syntax-highlighted spans for a line — the same query the editor
    /// surface makes of its current buffer. Falls back to one unscoped
    /// span when no grammar is available.
    pub fn highlighted_line(&self, line: usize) -> Vec<HighlightedSpan> {
        let text = self.doc.text();
        if line >= text.len_lines() {
            return Vec::new();
        }
        match self.doc.syntax() {
            Some(syntax) => highlight::highlight_line(text, syntax, &self.loader.load(), line),
            None => vec![HighlightedSpan {
                text: self.line(line),
                scope: None,
            }],
        }
    }
}

/// The file didn't highlight? The caller still wants the raw lines
/// (plain-text fallback preview).
pub fn context_lines(path: &Path, line: usize, context: usize) -> Option<(usize, Vec<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let first = line.saturating_sub(context);
    let window = lines
        .iter()
        .skip(first)
        .take(2 * context + 1)
        .map(|line| (*line).to_owned())
        .collect();
    Some((first, window))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_reads_and_highlights() {
        let path = std::env::temp_dir().join(format!("eggplant-peek-{}.rs", std::process::id()));
        std::fs::write(&path, "fn main() {\n    let x = 1;\n}\n").unwrap();
        let editor = crate::Editor::scratch().unwrap();
        let peek = editor.peek(&path).unwrap();
        assert_eq!(peek.line_count(), 4); // 3 + phantom
        assert_eq!(peek.line(1), "    let x = 1;");
        let spans = peek.highlighted_line(0);
        assert!(!spans.is_empty());
        if peek.doc.syntax().is_some() {
            assert!(
                spans.iter().any(|span| span.scope.is_some()),
                "with a grammar, spans carry scopes"
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn context_lines_window_clamps_at_file_start() {
        let path = std::env::temp_dir().join(format!("eggplant-ctx-{}.txt", std::process::id()));
        std::fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();
        let (first, window) = context_lines(&path, 1, 3).unwrap();
        assert_eq!(first, 0);
        assert_eq!(window, vec!["a", "b", "c", "d", "e"]);
        std::fs::remove_file(path).unwrap();
    }
}
