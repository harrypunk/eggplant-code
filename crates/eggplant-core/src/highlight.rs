//! Syntax highlighting vocabulary and per-line span extraction.
//!
//! Language support itself is **configuration-driven**: helix-core's
//! `syntax::Loader` reads `languages.toml`, `.scm` queries and tree-sitter
//! grammars (`.so`) from runtime directories at startup — adding a language
//! never touches this crate. What this module owns is the *vocabulary*
//! (`SyntaxScope`) that query capture names map onto and that themes color:
//! registered with the loader via `Loader::set_scopes`, longest-prefix
//! matching (e.g. `keyword.storage.modifier` → `Keyword`).

use helix_core::ropey::Rope;
use helix_core::syntax::{Highlight, HighlightEvent, Loader, Syntax};

/// Semantic highlight scopes — the vocabulary between backend and theme.
/// Order must match [`SCOPE_NAMES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxScope {
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Constant,
    Number,
    Variable,
    Operator,
    Punctuation,
    Attribute,
    Special,
}

/// Scope names registered with the loader, in `SyntaxScope` order.
pub const SCOPE_NAMES: &[&str] = &[
    "keyword",
    "string",
    "comment",
    "function",
    "type",
    "constant",
    "number",
    "variable",
    "operator",
    "punctuation",
    "attribute",
    "special",
];

/// One contiguous run of text on a line; `None` scope = plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedSpan {
    pub text: String,
    pub scope: Option<SyntaxScope>,
}

/// A snippet's highlighted lines: one span vec per line.
pub type HighlightedLines = Vec<Vec<HighlightedSpan>>;

/// The one highlighting capability read-only surfaces need (chat
/// markdown, previews). Segregated from the `Editor` facade so views
/// depend on this trait, not on the whole editor.
pub trait SnippetHighlighter {
    /// Highlight a code snippet by language name. `None` when the
    /// language is unknown — callers fall back to plain code styling.
    fn highlight_snippet(&self, code: &str, language: &str) -> Option<HighlightedLines>;
}

/// Resolve a registered scope name (loader vocabulary) to a `SyntaxScope`.
/// Unknown names (shouldn't happen — only registered names resolve) are plain.
fn scope_of(highlight: Highlight, loader: &Loader) -> Option<SyntaxScope> {
    let scopes = loader.scopes();
    let name = scopes.get(highlight.idx())?;
    SCOPE_NAMES
        .iter()
        .position(|s| name.starts_with(s))
        .map(|i| match i {
            0 => SyntaxScope::Keyword,
            1 => SyntaxScope::String,
            2 => SyntaxScope::Comment,
            3 => SyntaxScope::Function,
            4 => SyntaxScope::Type,
            5 => SyntaxScope::Constant,
            6 => SyntaxScope::Number,
            7 => SyntaxScope::Variable,
            8 => SyntaxScope::Operator,
            9 => SyntaxScope::Punctuation,
            10 => SyntaxScope::Attribute,
            _ => SyntaxScope::Special,
        })
}

/// Highlight one line (0-based, clipped of its line ending) by walking the
/// syntax highlighter's event stream. An empty vec for out-of-range lines.
pub fn highlight_line(
    text: &Rope,
    syntax: &Syntax,
    loader: &Loader,
    line: usize,
) -> Vec<HighlightedSpan> {
    if line >= text.len_lines() {
        return Vec::new();
    }
    let start = text.line_to_byte(line);
    let mut end = text.line_to_byte((line + 1).min(text.len_lines()));
    while end > start && matches!(text.byte(end - 1), b'\n' | b'\r') {
        end -= 1;
    }
    if start >= end {
        return Vec::new();
    }

    let mut highlighter = syntax.highlighter(text.slice(..), loader, start as u32..end as u32);
    let mut active: Vec<Highlight> = Vec::new();
    let mut spans = Vec::new();
    let mut pos = start;

    loop {
        let next = highlighter.next_event_offset() as usize;
        if next == u32::MAX as usize || next >= end {
            break;
        }
        // Injection layers (e.g. markdown in doc comments) emit events whose
        // offsets revisit bytes before the current position: never move the
        // scan position backward, or text would be emitted twice.
        if next > pos {
            push_span(&mut spans, text, pos..next, active.last(), loader);
            pos = next;
        }
        let (event, new) = highlighter.advance();
        match event {
            HighlightEvent::Refresh => active = new.collect(),
            HighlightEvent::Push => active.extend(new),
        }
    }
    if pos < end {
        push_span(&mut spans, text, pos..end, active.last(), loader);
    }
    spans
}

fn push_span(
    spans: &mut Vec<HighlightedSpan>,
    text: &Rope,
    range: std::ops::Range<usize>,
    highlight: Option<&Highlight>,
    loader: &Loader,
) {
    let scope = highlight.and_then(|h| scope_of(*h, loader));
    // Merge with the previous span when the scope continues.
    if let Some(last) = spans.last_mut()
        && last.scope == scope
    {
        last.text.push_str(&text.byte_slice(range).to_string());
        return;
    }
    spans.push(HighlightedSpan {
        text: text.byte_slice(range).to_string(),
        scope,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_vocabulary_is_aligned() {
        assert_eq!(SCOPE_NAMES.len(), 12);
    }
}
