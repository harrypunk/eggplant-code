//! The editor facade: buffers of documents, one view, one cursor (for now).
//!
//! Wraps helix-view's `Document` + helix-core `Transaction`s behind a small,
//! UI-agnostic API. The UI never touches helix types directly, so the backend
//! can be swapped for our own core later.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use arc_swap::ArcSwap;
use helix_core::graphemes::{
    next_grapheme_boundary, nth_next_grapheme_boundary, nth_prev_grapheme_boundary,
    prev_grapheme_boundary,
};
use helix_core::movement::{move_next_word_end, move_next_word_start, move_prev_word_start};
use helix_core::selection::Range;
use helix_core::syntax;
use helix_core::{Selection, Transaction};
use helix_view::document::Document;
use helix_view::editor::{Config, GutterConfig};
use helix_view::{DocumentId, View};

use crate::highlight::{self, HighlightedSpan};
use crate::mode::Mode;

/// Shared, hot-swappable backend configuration (required by helix-view).
struct Backend {
    config: Arc<ArcSwap<Config>>,
    syn_loader: Arc<ArcSwap<syntax::Loader>>,
    /// Save futures from helix-view use `tokio::fs`; a current-thread runtime
    /// is enough since saves are short and rare (async event loop comes later).
    runtime: tokio::runtime::Runtime,
}

impl Backend {
    fn new() -> Result<Self> {
        // Config-driven language support: languages.toml (user's config
        // merged over helix's embedded default), queries and grammars (.so)
        // discovered from runtime directories — no languages compiled in.
        let trust = helix_loader::workspace_trust::WorkspaceTrust::new(Default::default());
        let loader = helix_core::config::user_lang_loader(&trust)
            .unwrap_or_else(|_| helix_core::config::default_lang_loader());
        // Register our highlight vocabulary; query captures resolve to these
        // by longest-prefix match (e.g. "keyword.storage" -> "keyword").
        loader.set_scopes(
            crate::highlight::SCOPE_NAMES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        Ok(Self {
            config: Arc::new(ArcSwap::from_pointee(Config::default())),
            syn_loader: Arc::new(ArcSwap::from_pointee(loader)),
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime")?,
        })
    }

    /// The shared syntax/language loader (guard derefs through to `Loader`).
    fn loader(&self) -> impl std::ops::Deref<Target = Arc<syntax::Loader>> + '_ {
        self.syn_loader.load()
    }

    fn open_document(&self, path: &Path) -> Result<Document> {
        let mut doc = Document::open(
            path,
            None,
            false,
            self.config.clone(),
            self.syn_loader.clone(),
        )
        .with_context(|| format!("failed to open {}", path.display()))?;
        // Detect language + build the syntax tree (no-op gracefully when the
        // grammar/queries aren't available in any runtime directory).
        doc.detect_language(&self.syn_loader.load());
        Ok(doc)
    }

    fn scratch_document(&self) -> Document {
        Document::default(self.config.clone(), self.syn_loader.clone())
    }
}

/// An open document plus its per-buffer state.
struct Buffer {
    doc: Document,
}

/// Display path relative to the working directory (editor chrome style);
/// falls back to the full path for files outside it.
fn relative_display(path: &std::path::Path) -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return path.display().to_string();
    };
    path.strip_prefix(&cwd)
        .unwrap_or(path)
        .display()
        .to_string()
}

impl Buffer {
    fn new(doc: Document) -> Self {
        Self { doc }
    }

    fn display_name(&self) -> String {
        self.doc
            .path()
            .map(relative_display)
            .unwrap_or_else(|| "untitled".to_owned())
    }
}

/// Read-only buffer summary for UIs (topbar, pickers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferInfo {
    pub index: usize,
    pub name: String,
    pub modified: bool,
    pub current: bool,
    /// Scratch buffers have no backing file.
    pub scratch: bool,
}

pub struct Editor {
    buffers: Vec<Buffer>,
    /// Index of the current buffer in `buffers`.
    /// Index of the current buffer; `None` when no buffer is open.
    current: Option<usize>,
    /// The single view (identity for selections + jumplist sync on undo/redo).
    view: View,
    mode: Mode,
    /// Bumped whenever the *identity* of the current document changes
    /// (open/switch/close), so the UI can drop per-document state.
    generation: usize,
    backend: Backend,
}

impl Editor {
    /// Open a file (or an empty document if it doesn't exist yet).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let backend = Backend::new()?;
        let doc = backend.open_document(path.as_ref())?;
        Ok(Self::new(Buffer::new(doc), backend))
    }

    /// A new empty scratch document.
    pub fn scratch() -> Result<Self> {
        let backend = Backend::new()?;
        let doc = backend.scratch_document();
        Ok(Self::new(Buffer::new(doc), backend))
    }

    /// No buffers at all (directory startup: explorer + empty editor).
    pub fn empty() -> Result<Self> {
        Ok(Self {
            buffers: Vec::new(),
            current: None,
            view: View::new(DocumentId::default(), GutterConfig::default()),
            mode: Mode::Normal,
            generation: 0,
            backend: Backend::new()?,
        })
    }

    fn new(buffer: Buffer, backend: Backend) -> Self {
        let view = View::new(buffer.doc.id(), GutterConfig::default());
        let mut editor = Self {
            buffers: vec![buffer],
            current: Some(0),
            view,
            mode: Mode::Normal,
            generation: 0,
            backend,
        };
        editor.reset_cursor();
        editor
    }

    fn reset_cursor(&mut self) {
        let view_id = self.view.id;
        if let Some(doc) = self.doc_mut() {
            doc.set_selection(view_id, Selection::point(0));
        }
    }

    /// Commit pending changes as one undo revision (vim granularity: one
    /// revision per insert session / normal-mode command).
    fn commit_history(&mut self) {
        let Some(i) = self.current else { return };
        self.buffers[i]
            .doc
            .append_changes_to_history(&mut self.view);
    }

    /// Undo the last revision. False when already at the oldest change.
    pub fn undo(&mut self) -> bool {
        let Some(i) = self.current else { return false };
        self.buffers[i].doc.undo(&mut self.view)
    }

    /// Redo the last undone revision. False when nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(i) = self.current else { return false };
        self.buffers[i].doc.redo(&mut self.view)
    }

    /// The current buffer's document. Private helpers assume `Some` — every
    /// public method guards on `has_buffer()` first.
    fn doc(&self) -> &Document {
        self.doc_opt().expect("current buffer")
    }

    fn doc_opt(&self) -> Option<&Document> {
        self.current.map(|i| &self.buffers[i].doc)
    }

    fn doc_mut(&mut self) -> Option<&mut Document> {
        self.current.map(|i| &mut self.buffers[i].doc)
    }

    /// Index of the current buffer; `None` when no buffer is open.
    pub fn current_buffer(&self) -> Option<usize> {
        self.current
    }

    // ---- buffers ----

    /// Open `path` into a buffer and switch to it. If the file is already
    /// open, switches to its buffer instead of duplicating.
    pub fn open_buffer(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(index) = self.buffers.iter().position(|b| b.doc.path() == Some(path)) {
            self.switch_buffer(index)?;
            return Ok(());
        }
        let doc = self.backend.open_document(path)?;
        self.buffers.push(Buffer::new(doc));
        self.current = Some(self.buffers.len() - 1);
        self.reset_cursor();
        self.generation += 1;
        Ok(())
    }

    pub fn switch_buffer(&mut self, index: usize) -> Result<()> {
        if index >= self.buffers.len() {
            bail!("no buffer {}", index + 1);
        }
        if self.current != Some(index) {
            self.current = Some(index);
            self.reset_cursor();
            self.generation += 1;
        }
        Ok(())
    }

    /// Close the current buffer. Refuses when it has unsaved changes unless
    /// `force`. Closing the last buffer leaves the editor empty (no buffer).
    pub fn close_current_buffer(&mut self, force: bool) -> Result<()> {
        let Some(index) = self.current else {
            bail!("no buffer to close");
        };
        if self.is_modified() && !force {
            bail!("unsaved changes (use ! to discard)");
        }
        self.buffers.remove(index);
        self.current = if self.buffers.is_empty() {
            None
        } else {
            Some(index.min(self.buffers.len() - 1))
        };
        self.reset_cursor();
        self.generation += 1;
        Ok(())
    }

    pub fn next_buffer(&mut self) {
        if let Some(current) = self.current {
            let next = (current + 1) % self.buffers.len();
            let _ = self.switch_buffer(next);
        }
    }

    pub fn prev_buffer(&mut self) {
        if let Some(current) = self.current {
            let prev = (current + self.buffers.len() - 1) % self.buffers.len();
            let _ = self.switch_buffer(prev);
        }
    }

    pub fn buffers_info(&self) -> Vec<BufferInfo> {
        self.buffers
            .iter()
            .enumerate()
            .map(|(index, b)| BufferInfo {
                index,
                name: b.display_name(),
                modified: b.doc.is_modified(),
                current: Some(index) == self.current,
                scratch: b.doc.path().is_none(),
            })
            .collect()
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// Whether any buffer is open (false after directory startup).
    pub fn has_buffer(&self) -> bool {
        self.current.is_some()
    }

    /// True when any buffer has unsaved changes (used by quit guards).
    pub fn any_modified(&self) -> bool {
        self.buffers.iter().any(|b| b.doc.is_modified())
    }

    /// Document identity counter — the UI resets scroll etc. when it changes.
    pub fn generation(&self) -> usize {
        self.generation
    }

    // ---- queries (read-only views for the UI) ----

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_modified(&self) -> bool {
        self.current
            .is_some_and(|i| self.buffers[i].doc.is_modified())
    }

    pub fn line_count(&self) -> usize {
        if self.current.is_none() {
            return 0;
        }
        self.doc().text().len_lines()
    }

    /// One line as highlighted spans (plain text when the language is
    /// unsupported). Clipped of the line ending.
    pub fn highlighted_line(&self, line: usize) -> Vec<HighlightedSpan> {
        let Some(i) = self.current else {
            return Vec::new();
        };
        let buffer = &self.buffers[i];
        let text = buffer.doc.text();
        if line >= text.len_lines() {
            return Vec::new();
        }
        match buffer.doc.syntax() {
            Some(syntax) => highlight::highlight_line(text, syntax, &self.backend.loader(), line),
            None => {
                let stripped: String = text
                    .line(line)
                    .chars()
                    .take_while(|c| *c != '\n' && *c != '\r')
                    .collect();
                vec![HighlightedSpan {
                    text: stripped,
                    scope: None,
                }]
            }
        }
    }

    /// Primary cursor as `(line, column)` in char units.
    pub fn cursor(&self) -> (usize, usize) {
        if self.current.is_none() {
            return (0, 0);
        }
        let text = self.doc().text();
        let pos = self.cursor_char_idx();
        let line = text.char_to_line(pos);
        (line, pos - text.line_to_char(line))
    }

    fn cursor_char_idx(&self) -> usize {
        self.doc()
            .selection(self.view.id)
            .primary()
            .cursor(self.doc().text().slice(..))
    }

    /// Raw head of the primary range — the insert position in insert mode.
    fn selection_head(&self) -> usize {
        self.doc().selection(self.view.id).primary().head
    }

    /// Put the cursor at `pos`, respecting helix's block-cursor selection
    /// model: selections are always >= 1 grapheme wide and the range
    /// *direction* encodes the mode —
    ///
    /// - normal: forward `(pos, pos+1)`, block cursor sits at `pos`
    /// - insert: backward `(pos+1, pos)`, insert bar sits at `pos`
    fn set_cursor(&mut self, pos: usize) {
        let view_id = self.view.id;
        let len = self.doc().text().len_chars();
        let pos = pos.min(len);
        let range = match self.mode {
            Mode::Normal => Range::new(pos, (pos + 1).min(len)),
            Mode::Insert => Range::new((pos + 1).min(len), pos),
        };
        self.doc_mut()
            .expect("current buffer")
            .set_selection(view_id, Selection::single(range.anchor, range.head));
    }

    // ---- mode ----

    /// `i` — insert before the block cursor.
    pub fn enter_insert(&mut self) {
        if self.current.is_none() {
            return;
        }
        let pos = self.cursor_char_idx();
        self.mode = Mode::Insert;
        self.set_cursor(pos);
    }

    /// `a` — insert after the block cursor (append).
    pub fn enter_append(&mut self) {
        if self.current.is_none() {
            return;
        }
        let text = self.doc().text().slice(..);
        let pos = self.cursor_char_idx();
        // After the grapheme under the cursor; on empty lines (cursor sits on
        // the line ending) the position itself is the insert point.
        let insert_at = if text.get_char(pos).is_none_or(|c| c == '\n') {
            pos
        } else {
            next_grapheme_boundary(text, pos)
        };
        self.mode = Mode::Insert;
        self.set_cursor(insert_at);
    }

    /// `Esc` — back to normal mode. Vim semantics: the cursor steps back one
    /// grapheme from the insert position and never rests on a line ending
    /// (unless the line is empty).
    pub fn enter_normal(&mut self) {
        if self.current.is_none() {
            return;
        }
        if self.mode == Mode::Normal {
            return;
        }
        let head = self.selection_head();
        let line = self.doc().text().char_to_line(head);
        let line_start = self.doc().text().line_to_char(line);
        let line_len = self.line_char_len(line);
        let col = head - line_start;
        let target = if line_len == 0 {
            line_start
        } else {
            line_start + col.saturating_sub(1).min(line_len - 1)
        };
        self.mode = Mode::Normal;
        self.set_cursor(target);
        // End of an insert session: commit it as one undo revision.
        self.commit_history();
    }

    // ---- movements ----

    pub fn move_left(&mut self, count: usize) {
        if self.current.is_none() {
            return;
        }
        let text = self.doc().text().slice(..);
        let pos = self.cursor_char_idx();
        let line_start = text.line_to_char(text.char_to_line(pos));
        // Clamp at line start: horizontal moves never cross line boundaries.
        let new = nth_prev_grapheme_boundary(text, pos, count).max(line_start);
        self.set_cursor(new);
    }

    pub fn move_right(&mut self, count: usize) {
        if self.current.is_none() {
            return;
        }
        let text = self.doc().text().slice(..);
        let pos = self.cursor_char_idx();
        let new = nth_next_grapheme_boundary(text, pos, count);
        // Clamp at line end (mode-aware) via set_cursor_on_line.
        let (line, _) = self.cursor();
        let col = new.saturating_sub(text.line_to_char(line));
        self.set_cursor_on_line(line, col);
    }

    pub fn move_up(&mut self, count: usize) {
        self.move_lines(-(count.min(isize::MAX as usize) as isize));
    }

    pub fn move_down(&mut self, count: usize) {
        self.move_lines(count.min(isize::MAX as usize) as isize);
    }

    fn move_lines(&mut self, delta: isize) {
        if self.current.is_none() {
            return;
        }
        let (line, col) = self.cursor();
        let last = self.line_count().saturating_sub(1);
        let target = line.saturating_add_signed(delta).min(last);
        self.set_cursor_on_line(target, col);
    }

    /// Put the cursor on `line` at `col`, clamped to that line's content.
    /// Insert mode may rest one past the last char (the bar at end of line);
    /// normal mode stays on the last real char (unless the line is empty).
    fn set_cursor_on_line(&mut self, line: usize, col: usize) {
        let text = self.doc().text().slice(..);
        let start = text.line_to_char(line);
        let len = self.line_char_len(line);
        let max_col = match self.mode {
            Mode::Normal => len.saturating_sub(1),
            Mode::Insert => len,
        };
        self.set_cursor(start + col.min(max_col));
    }

    /// Char length of a line's content, excluding the line ending (if any).
    fn line_char_len(&self, line: usize) -> usize {
        let text = self.doc().text();
        let start = text.line_to_char(line);
        let end = text.line_to_char(line + 1).max(start);
        let mut len = end.saturating_sub(start);
        while len > 0 && matches!(text.get_char(start + len - 1), Some('\r') | Some('\n')) {
            len -= 1;
        }
        len
    }

    pub fn move_line_start(&mut self) {
        if self.current.is_none() {
            return;
        }
        let (line, _) = self.cursor();
        self.set_cursor_on_line(line, 0);
    }

    pub fn move_line_end(&mut self) {
        if self.current.is_none() {
            return;
        }
        let (line, _) = self.cursor();
        let end = self.line_char_len(line).saturating_sub(1);
        self.set_cursor_on_line(line, end);
    }

    pub fn move_last_line(&mut self) {
        if self.current.is_none() {
            return;
        }
        let (_, col) = self.cursor();
        let last = self.line_count().saturating_sub(1);
        self.set_cursor_on_line(last, col);
    }

    /// Go to `line` (0-based, clamped), keeping the column where possible.
    pub fn move_to_line(&mut self, line: usize) {
        if self.current.is_none() {
            return;
        }
        let (_, col) = self.cursor();
        let target = line.min(self.line_count().saturating_sub(1));
        self.set_cursor_on_line(target, col);
    }

    pub fn move_word_forward(&mut self, count: usize) {
        self.word_move(move_next_word_start, |_s, r| r.head, count);
    }

    pub fn move_word_backward(&mut self, count: usize) {
        self.word_move(move_prev_word_start, |_s, r| r.head, count);
    }

    pub fn move_word_end(&mut self, count: usize) {
        // End-motions point one past the last char of the word; the block
        // cursor belongs on the last char itself.
        self.word_move(
            move_next_word_end,
            |s, r| prev_grapheme_boundary(s, r.head),
            count,
        );
    }

    fn word_move(
        &mut self,
        motion: impl Fn(
            helix_core::RopeSlice,
            helix_core::selection::Range,
            usize,
        ) -> helix_core::selection::Range,
        target: impl Fn(helix_core::RopeSlice, helix_core::selection::Range) -> usize,
        count: usize,
    ) {
        if self.current.is_none() {
            return;
        }
        let text = self.doc().text().slice(..);
        let range = self.doc().selection(self.view.id).primary();
        let new = motion(text, range, count);
        if new.head == range.head {
            return; // motion didn't move
        }
        self.set_cursor(target(text, new));
    }

    // ---- edits ----

    fn apply(&mut self, transaction: Transaction) {
        if self.current.is_none() {
            return;
        }
        let view_id = self.view.id;
        let doc = self.doc_mut().expect("current buffer");
        // `Document::apply` also incrementally reparses the syntax tree.
        doc.apply(&transaction, view_id);
    }

    pub fn insert_char(&mut self, c: char) {
        self.insert_str(&c.to_string());
    }

    pub fn insert_str(&mut self, s: &str) {
        if self.current.is_none() {
            return;
        }
        let transaction = Transaction::insert(
            self.doc().text(),
            self.doc().selection(self.view.id),
            s.into(),
        );
        self.apply(transaction);
    }

    pub fn insert_newline(&mut self) {
        let le = self.doc().line_ending.as_str().to_owned();
        self.insert_str(&le);
    }

    /// Insert a blank line below (`o`). The caller enters insert mode after.
    pub fn open_line_below(&mut self) {
        self.open_line_at_offset(1);
    }

    /// Insert a blank line above (`O`). The caller enters insert mode after.
    pub fn open_line_above(&mut self) {
        self.open_line_at_offset(0);
    }

    fn open_line_at_offset(&mut self, offset: usize) {
        if self.current.is_none() {
            return;
        }
        let (line, _) = self.cursor();
        let target_line = line + offset;
        let insert_at = self
            .doc()
            .text()
            .line_to_char(target_line)
            .min(self.doc().text().len_chars());
        let le = self.doc().line_ending.as_str().to_owned();
        let transaction = Transaction::change(
            self.doc().text(),
            [(insert_at, insert_at, Some(le.into()))].into_iter(),
        );
        self.apply(transaction);
        // The new blank line starts where the line boundary landed after the edit.
        let start = self
            .doc()
            .text()
            .line_to_char(target_line.min(self.line_count() - 1));
        self.set_cursor(start);
    }

    pub fn delete_backward(&mut self) {
        if self.current.is_none() {
            return;
        }
        let text = self.doc().text().slice(..);
        let pos = self.cursor_char_idx();
        if pos == 0 {
            return;
        }
        let from = prev_grapheme_boundary(text, pos);
        self.apply(Transaction::delete(
            self.doc().text(),
            [(from, pos)].into_iter(),
        ));
    }

    /// Delete the grapheme under the cursor (`x` in normal mode).
    /// Never deletes the line ending.
    pub fn delete_char_at_cursor(&mut self) {
        if self.current.is_none() {
            return;
        }
        let text = self.doc().text().slice(..);
        let pos = self.cursor_char_idx();
        if text.get_char(pos).is_none_or(|c| c == '\n') {
            return;
        }
        let to = next_grapheme_boundary(text, pos);
        self.apply(Transaction::delete(
            self.doc().text(),
            [(pos, to)].into_iter(),
        ));
        self.clamp_cursor_off_line_ending();
        // Standalone normal-mode edit: its own undo revision.
        self.commit_history();
    }

    /// Normal mode: the block cursor never rests on a line ending unless the
    /// line is empty.
    fn clamp_cursor_off_line_ending(&mut self) {
        let (line, col) = self.cursor();
        let len = self.line_char_len(line);
        if len > 0 && col >= len {
            self.set_cursor_on_line(line, len - 1);
        }
    }

    // ---- persistence ----

    pub fn save(&mut self) -> Result<()> {
        if self.current.is_none() {
            bail!("no buffer to save");
        }
        // Commit pending changes so the saved revision is a history revision
        // (is_modified compares against it).
        self.commit_history();
        let doc = self.doc_mut().expect("current buffer");
        let future = doc.save::<PathBuf>(None, false)?;
        let event = self
            .backend
            .runtime
            .block_on(future)
            .context("save failed")?;
        let doc = self.doc_mut().expect("current buffer");
        doc.set_last_saved_revision(event.revision, event.save_time);
        Ok(())
    }

    // ---- introspection for status line ----

    /// Current buffer name; `None` when no buffer is open.
    pub fn display_name(&self) -> Option<String> {
        let doc = self.doc_opt()?;
        Some(
            doc.path()
                .map(relative_display)
                .unwrap_or_else(|| "untitled".to_owned()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::SyntaxScope;

    #[test]
    fn relative_display_strips_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let inside = cwd.join("src/editor.rs");
        assert_eq!(relative_display(&inside), "src/editor.rs");
        // Outside the cwd: full path.
        assert_eq!(
            relative_display(std::path::Path::new("/elsewhere/f.rs")),
            "/elsewhere/f.rs"
        );
        // Already relative: as-is.
        assert_eq!(
            relative_display(std::path::Path::new("README.md")),
            "README.md"
        );
    }

    fn text_of(editor: &Editor) -> String {
        (0..editor.line_count())
            .map(|line| {
                editor
                    .highlighted_line(line)
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Scratch doc, typed `content` in insert mode, back to normal mode.
    fn editor_with(content: &str) -> Editor {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str(content);
        ed.enter_normal();
        ed
    }

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("eggplant-{}-{name}", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn insert_and_cursor() {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str("hello");
        assert_eq!(text_of(&ed), "hello\n");
        assert_eq!(ed.cursor(), (0, 5)); // insert bar after "hello"
        assert!(ed.is_modified());
    }

    #[test]
    fn esc_steps_cursor_back() {
        let ed = editor_with("abcd");
        assert_eq!(ed.cursor(), (0, 3)); // block on 'd'
    }

    #[test]
    fn horizontal_moves_stop_at_line_boundaries() {
        let mut ed = editor_with("ab\ncd");
        assert_eq!(ed.cursor(), (1, 1));
        ed.move_left(10);
        assert_eq!(ed.cursor(), (1, 0)); // stops at line start
        ed.move_right(10);
        assert_eq!(ed.cursor(), (1, 1)); // stops at last char, not the newline
    }

    #[test]
    fn word_motions() {
        let mut ed = editor_with("foo bar  baz");
        ed.move_line_start();
        ed.move_word_forward(1);
        assert_eq!(ed.cursor(), (0, 4)); // "bar"
        ed.move_word_forward(1);
        assert_eq!(ed.cursor(), (0, 9)); // "baz"
        ed.move_word_backward(1);
        assert_eq!(ed.cursor(), (0, 4));
        ed.move_word_end(1);
        assert_eq!(ed.cursor(), (0, 6)); // end of "bar"
    }

    #[test]
    fn delete_char_at_cursor_skips_newline() {
        let mut ed = editor_with("ab\ncd");
        ed.delete_char_at_cursor(); // on 'd'
        assert_eq!(text_of(&ed), "ab\nc\n");
        assert_eq!(ed.cursor(), (1, 0)); // clamped back onto 'c'
        ed.delete_char_at_cursor(); // on 'c'
        assert_eq!(text_of(&ed), "ab\n\n");
        assert_eq!(ed.cursor(), (1, 0)); // empty line: may rest on the newline
        ed.delete_char_at_cursor(); // on the newline: no-op
        assert_eq!(text_of(&ed), "ab\n\n");
    }

    #[test]
    fn open_line_below_and_above() {
        let mut ed = editor_with("one\nthree");
        ed.open_line_above();
        ed.enter_insert();
        ed.insert_str("two");
        assert_eq!(text_of(&ed), "one\ntwo\nthree\n");
        ed.enter_normal();
        ed.move_up(10);
        ed.open_line_below();
        ed.enter_insert();
        ed.insert_str("1.5");
        assert_eq!(text_of(&ed), "one\n1.5\ntwo\nthree\n");
    }

    #[test]
    fn insert_mode_arrows_then_type() {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str("ab\ncd");
        ed.move_up(1); // insert bar at (0, 2)
        ed.insert_str("X");
        assert_eq!(text_of(&ed), "abX\ncd\n");
    }

    #[test]
    fn append_enters_after_cursor() {
        let mut ed = editor_with("abc");
        ed.move_line_start();
        ed.enter_append();
        ed.insert_str("X");
        assert_eq!(text_of(&ed), "aXbc\n");
    }

    #[test]
    fn line_end_on_final_line_without_newline() {
        // Regression: line_char_len must not eat the last char when the
        // final line has no trailing newline.
        let path = temp_file("nonl", "ab\ncde");
        let mut ed = Editor::open(&path).unwrap();
        ed.move_last_line();
        ed.move_line_end();
        assert_eq!(ed.cursor(), (1, 2)); // on 'e', not 'd'
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn save_roundtrip() {
        let path = std::env::temp_dir().join(format!("eggplant-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut ed = Editor::open(&path).unwrap();
            ed.enter_insert();
            ed.insert_str("saved content");
            assert!(ed.is_modified());
            ed.save().unwrap();
            assert!(!ed.is_modified());
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved content\n");
        std::fs::remove_file(&path).unwrap();
    }

    // ---- buffers ----

    #[test]
    fn open_buffer_adds_and_switches() {
        let path_a = temp_file("buf-a", "aaa");
        let path_b = temp_file("buf-b", "bbb");
        let mut ed = Editor::open(&path_a).unwrap();
        assert_eq!(ed.buffer_count(), 1);

        ed.open_buffer(&path_b).unwrap();
        assert_eq!(ed.buffer_count(), 2);
        assert_eq!(ed.current_buffer(), Some(1));
        assert_eq!(text_of(&ed), "bbb");

        // Reopening the same path switches instead of duplicating.
        ed.open_buffer(&path_a).unwrap();
        assert_eq!(ed.buffer_count(), 2);
        assert_eq!(ed.current_buffer(), Some(0));

        std::fs::remove_file(&path_a).unwrap();
        std::fs::remove_file(&path_b).unwrap();
    }

    #[test]
    fn modified_flag_is_per_buffer() {
        let path_a = temp_file("mod-a", "aaa");
        let path_b = temp_file("mod-b", "bbb");
        let mut ed = Editor::open(&path_a).unwrap();
        ed.open_buffer(&path_b).unwrap();

        ed.enter_insert();
        ed.insert_str("dirty");
        assert!(ed.is_modified());
        assert!(ed.any_modified());

        ed.switch_buffer(0).unwrap();
        assert!(!ed.is_modified());
        assert!(ed.any_modified()); // buffer B is still dirty

        std::fs::remove_file(&path_a).unwrap();
        std::fs::remove_file(&path_b).unwrap();
    }

    #[test]
    fn close_buffer_guards_unsaved_and_empties_on_last() {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str("dirty");
        ed.enter_normal();

        // Refuses with unsaved changes.
        assert!(ed.close_current_buffer(false).is_err());
        // Force closes; closing the last buffer leaves the editor empty.
        ed.close_current_buffer(true).unwrap();
        assert_eq!(ed.buffer_count(), 0);
        assert!(!ed.has_buffer());
        assert_eq!(ed.current_buffer(), None);
    }

    #[test]
    fn empty_editor_is_a_safe_no_op_surface() {
        let mut ed = Editor::empty().unwrap();
        assert!(!ed.has_buffer());
        assert_eq!(ed.buffer_count(), 0);
        assert_eq!(ed.cursor(), (0, 0));
        assert_eq!(ed.line_count(), 0);
        assert!(ed.highlighted_line(0).is_empty());
        assert_eq!(ed.display_name(), None);
        assert!(ed.save().is_err());
        assert!(ed.close_current_buffer(true).is_err());

        // Everything below must be a no-op, not a panic.
        ed.enter_insert();
        assert_eq!(ed.mode(), Mode::Normal); // stays normal without a buffer
        ed.insert_str("nope");
        ed.move_down(3);
        ed.move_word_forward(1);
        ed.delete_char_at_cursor();
        ed.next_buffer();
        ed.prev_buffer();

        // Opening a file into the empty editor works.
        let path = temp_file("empty-open", "hello");
        ed.open_buffer(&path).unwrap();
        assert!(ed.has_buffer());
        assert_eq!(text_of(&ed), "hello");
        std::fs::remove_file(&path).unwrap();
    }

    // ---- highlighting ----

    #[test]
    fn scratch_buffer_yields_plain_spans() {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str("hello world");
        let spans = ed.highlighted_line(0);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello world");
        assert_eq!(spans[0].scope, None);
    }

    #[test]
    fn rust_file_is_highlighted_when_runtime_available() {
        let path = temp_file("highlight.rs", "fn main() { let s = \"hi\"; }\n");
        let ed = Editor::open(&path).unwrap();
        let spans = ed.highlighted_line(0);
        std::fs::remove_file(&path).unwrap();

        if spans.iter().all(|s| s.scope.is_none()) {
            eprintln!("skipping: no tree-sitter runtime available");
            return;
        }
        let find = |needle: &str| {
            spans
                .iter()
                .find(|s| s.text == needle)
                .unwrap_or_else(|| panic!("span {needle:?} in {spans:?}"))
                .scope
        };
        assert_eq!(find("fn"), Some(SyntaxScope::Keyword));
        assert_eq!(find("main"), Some(SyntaxScope::Function));
        assert_eq!(find("\"hi\""), Some(SyntaxScope::String));
    }

    #[test]
    fn doc_comment_injection_does_not_duplicate_text() {
        let content =
            "//! Module docs with `code`.\n/// Doc comment with `SyntaxScope` inline.\nfn x() {}\n";
        let path = temp_file("inject.rs", content);
        let ed = Editor::open(&path).unwrap();
        for (line, expected) in content.lines().enumerate() {
            let spans = ed.highlighted_line(line);
            if spans.iter().all(|s| s.scope.is_none()) {
                eprintln!("skipping: no tree-sitter runtime available");
                break;
            }
            let rendered: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(
                rendered, expected,
                "line {line} spans overlap/gap: {spans:?}"
            );
        }
        std::fs::remove_file(&path).unwrap();
    }

    // ---- undo/redo ----

    #[test]
    fn undo_redo_roundtrip() {
        let mut ed = editor_with("hello"); // one insert session
        assert!(ed.undo());
        assert_eq!(text_of(&ed), "\n");
        assert!(ed.redo());
        assert_eq!(text_of(&ed), "hello\n");
    }

    #[test]
    fn undo_granularity_is_per_insert_session() {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str("one ");
        ed.enter_normal();
        ed.enter_append();
        ed.insert_str("two");
        ed.enter_normal();
        assert_eq!(text_of(&ed), "one two\n");

        // First undo reverts only the second session (vim convention).
        assert!(ed.undo());
        assert_eq!(text_of(&ed), "one \n");
        assert!(ed.undo());
        assert_eq!(text_of(&ed), "\n");
        assert!(!ed.undo(), "third undo: nothing left");
    }

    #[test]
    fn undo_at_oldest_change_returns_false() {
        let mut ed = Editor::scratch().unwrap();
        assert!(!ed.undo());
        assert!(!ed.redo());
    }

    #[test]
    fn modified_flag_follows_undo_revisions() {
        let mut ed = editor_with("dirty");
        assert!(ed.is_modified());
        assert!(ed.undo());
        assert!(!ed.is_modified(), "undo back to the saved state is clean");
        assert!(ed.redo());
        assert!(ed.is_modified());
    }

    #[test]
    fn x_delete_is_its_own_revision() {
        let mut ed = editor_with("ab"); // post-Esc cursor sits on 'b'
        ed.delete_char_at_cursor();
        assert_eq!(text_of(&ed), "a\n");
        assert!(ed.undo());
        assert_eq!(text_of(&ed), "ab\n");
    }

    #[test]
    fn next_prev_buffer_wraps() {
        let path_a = temp_file("wrap-a", "a");
        let path_b = temp_file("wrap-b", "b");
        let mut ed = Editor::open(&path_a).unwrap();
        ed.open_buffer(&path_b).unwrap();

        ed.next_buffer();
        assert_eq!(ed.current_buffer(), Some(0));
        ed.prev_buffer();
        assert_eq!(ed.current_buffer(), Some(1));

        std::fs::remove_file(&path_a).unwrap();
        std::fs::remove_file(&path_b).unwrap();
    }
}
