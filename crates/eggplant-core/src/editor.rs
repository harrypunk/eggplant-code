//! The editor facade: one document, one view, one cursor (for now).
//!
//! Wraps helix-view's `Document` + helix-core `Transaction`s behind a small,
//! UI-agnostic API. The UI never touches helix types directly, so the backend
//! can be swapped for our own core later.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use helix_core::graphemes::{
    next_grapheme_boundary, nth_next_grapheme_boundary, nth_prev_grapheme_boundary,
    prev_grapheme_boundary,
};
use helix_core::movement::{move_next_word_end, move_next_word_start, move_prev_word_start};
use helix_core::selection::Range;
use helix_core::syntax;
use helix_core::{Selection, Transaction};
use helix_view::ViewId;
use helix_view::document::Document;
use helix_view::editor::Config;

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
        Ok(Self {
            config: Arc::new(ArcSwap::from_pointee(Config::default())),
            // Empty loader: no language detection/highlighting until M5.
            syn_loader: Arc::new(ArcSwap::from_pointee(syntax::Loader::default())),
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime")?,
        })
    }
}

pub struct Editor {
    doc: Document,
    view_id: ViewId,
    mode: Mode,
    /// Edit transactions applied since the last save (our own modified flag,
    /// independent of helix internals so the facade stays backend-agnostic).
    edits_since_save: usize,
    /// Bumped whenever the underlying document is replaced (open_file), so
    /// the UI can drop per-document state (scroll offsets, ...).
    generation: usize,
    backend: Backend,
}

impl Editor {
    /// Open a file (or an empty document if it doesn't exist yet).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let backend = Backend::new()?;
        let doc = Document::open(
            path.as_ref(),
            None,
            false,
            backend.config.clone(),
            backend.syn_loader.clone(),
        )
        .with_context(|| format!("failed to open {}", path.as_ref().display()))?;
        Ok(Self::from_doc(doc, backend))
    }

    /// A new empty scratch document.
    pub fn scratch() -> Result<Self> {
        let backend = Backend::new()?;
        let doc = Document::default(backend.config.clone(), backend.syn_loader.clone());
        Ok(Self::from_doc(doc, backend))
    }

    /// Replace the current document with the file at `path`.
    ///
    /// NOTE: there are no buffers yet (M3+) — this discards the current
    /// document. Callers should check `is_modified()` first.
    pub fn open_file(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let doc = Document::open(
            path.as_ref(),
            None,
            false,
            self.backend.config.clone(),
            self.backend.syn_loader.clone(),
        )
        .with_context(|| format!("failed to open {}", path.as_ref().display()))?;
        self.set_doc(doc);
        Ok(())
    }

    fn set_doc(&mut self, doc: Document) {
        self.doc = doc;
        self.doc.set_selection(self.view_id, Selection::point(0));
        self.edits_since_save = 0;
        self.generation += 1;
    }

    /// Document replacement counter — the UI resets scroll etc. when it changes.
    pub fn generation(&self) -> usize {
        self.generation
    }

    fn from_doc(mut doc: Document, backend: Backend) -> Self {
        let view_id = ViewId::default();
        doc.set_selection(view_id, Selection::point(0));
        Self {
            doc,
            view_id,
            mode: Mode::Normal,
            edits_since_save: 0,
            generation: 0,
            backend,
        }
    }

    // ---- queries (read-only views for the UI) ----

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn path(&self) -> Option<&Path> {
        self.doc.path()
    }

    pub fn is_modified(&self) -> bool {
        self.edits_since_save > 0
    }

    pub fn line_count(&self) -> usize {
        self.doc.text().len_lines()
    }

    /// The document's lines in `range` (clamped), with line endings stripped.
    pub fn lines(&self, range: std::ops::Range<usize>) -> Vec<String> {
        let text = self.doc.text();
        let end = range.end.min(self.line_count());
        (range.start.min(end)..end)
            .map(|i| {
                let line = text.line(i).to_string();
                line.trim_end_matches(['\r', '\n']).to_owned()
            })
            .collect()
    }

    /// Primary cursor as `(line, column)` in char units.
    pub fn cursor(&self) -> (usize, usize) {
        let text = self.doc.text();
        let pos = self.cursor_char_idx();
        let line = text.char_to_line(pos);
        (line, pos - text.line_to_char(line))
    }

    fn cursor_char_idx(&self) -> usize {
        self.doc
            .selection(self.view_id)
            .primary()
            .cursor(self.doc.text().slice(..))
    }

    /// Raw head of the primary range — the insert position in insert mode.
    fn selection_head(&self) -> usize {
        self.doc.selection(self.view_id).primary().head
    }

    /// Put the cursor at `pos`, respecting helix's block-cursor selection
    /// model: selections are always >= 1 grapheme wide and the range
    /// *direction* encodes the mode —
    ///
    /// - normal: forward `(pos, pos+1)`, block cursor sits at `pos`
    /// - insert: backward `(pos+1, pos)`, insert bar sits at `pos`
    fn set_cursor(&mut self, pos: usize) {
        let len = self.doc.text().len_chars();
        let pos = pos.min(len);
        let range = match self.mode {
            Mode::Normal => Range::new(pos, (pos + 1).min(len)),
            Mode::Insert => Range::new((pos + 1).min(len), pos),
        };
        self.doc
            .set_selection(self.view_id, Selection::single(range.anchor, range.head));
    }

    // ---- mode ----

    /// `i` — insert before the block cursor.
    pub fn enter_insert(&mut self) {
        let pos = self.cursor_char_idx();
        self.mode = Mode::Insert;
        self.set_cursor(pos);
    }

    /// `a` — insert after the block cursor (append).
    pub fn enter_append(&mut self) {
        let text = self.doc.text().slice(..);
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
        if self.mode == Mode::Normal {
            return;
        }
        let head = self.selection_head();
        let line = self.doc.text().char_to_line(head);
        let line_start = self.doc.text().line_to_char(line);
        let line_len = self.line_char_len(line);
        let col = head - line_start;
        let target = if line_len == 0 {
            line_start
        } else {
            line_start + col.saturating_sub(1).min(line_len - 1)
        };
        self.mode = Mode::Normal;
        self.set_cursor(target);
    }

    // ---- movements ----

    pub fn move_left(&mut self, count: usize) {
        let text = self.doc.text().slice(..);
        let pos = self.cursor_char_idx();
        let line_start = text.line_to_char(text.char_to_line(pos));
        // Clamp at line start: horizontal moves never cross line boundaries.
        let new = nth_prev_grapheme_boundary(text, pos, count).max(line_start);
        self.set_cursor(new);
    }

    pub fn move_right(&mut self, count: usize) {
        let text = self.doc.text().slice(..);
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
        let (line, col) = self.cursor();
        let last = self.line_count().saturating_sub(1);
        let target = line.saturating_add_signed(delta).min(last);
        self.set_cursor_on_line(target, col);
    }

    /// Put the cursor on `line` at `col`, clamped to that line's content.
    /// Insert mode may rest one past the last char (the bar at end of line);
    /// normal mode stays on the last real char (unless the line is empty).
    fn set_cursor_on_line(&mut self, line: usize, col: usize) {
        let text = self.doc.text().slice(..);
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
        let text = self.doc.text();
        let start = text.line_to_char(line);
        let end = text.line_to_char(line + 1).max(start);
        let mut len = end.saturating_sub(start);
        while len > 0 && matches!(text.get_char(start + len - 1), Some('\r') | Some('\n')) {
            len -= 1;
        }
        len
    }

    pub fn move_line_start(&mut self) {
        let (line, _) = self.cursor();
        self.set_cursor_on_line(line, 0);
    }

    pub fn move_line_end(&mut self) {
        let (line, _) = self.cursor();
        let end = self.line_char_len(line).saturating_sub(1);
        self.set_cursor_on_line(line, end);
    }

    pub fn move_last_line(&mut self) {
        let (_, col) = self.cursor();
        let last = self.line_count().saturating_sub(1);
        self.set_cursor_on_line(last, col);
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
        let text = self.doc.text().slice(..);
        let range = self.doc.selection(self.view_id).primary();
        let new = motion(text, range, count);
        if new.head == range.head {
            return; // motion didn't move
        }
        self.set_cursor(target(text, new));
    }

    // ---- edits ----

    fn apply(&mut self, transaction: Transaction) {
        self.doc.apply(&transaction, self.view_id);
        self.edits_since_save += 1;
    }

    pub fn insert_char(&mut self, c: char) {
        self.insert_str(&c.to_string());
    }

    pub fn insert_str(&mut self, s: &str) {
        let transaction =
            Transaction::insert(self.doc.text(), self.doc.selection(self.view_id), s.into());
        self.apply(transaction);
    }

    pub fn insert_newline(&mut self) {
        let le = self.doc.line_ending.as_str().to_owned();
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
        let (line, _) = self.cursor();
        let target_line = line + offset;
        let insert_at = self
            .doc
            .text()
            .line_to_char(target_line)
            .min(self.doc.text().len_chars());
        let le = self.doc.line_ending.as_str().to_owned();
        let transaction = Transaction::change(
            self.doc.text(),
            [(insert_at, insert_at, Some(le.into()))].into_iter(),
        );
        self.apply(transaction);
        // The new blank line starts where the line boundary landed after the edit.
        let start = self
            .doc
            .text()
            .line_to_char(target_line.min(self.line_count() - 1));
        self.set_cursor(start);
    }

    pub fn delete_backward(&mut self) {
        let text = self.doc.text().slice(..);
        let pos = self.cursor_char_idx();
        if pos == 0 {
            return;
        }
        let from = prev_grapheme_boundary(text, pos);
        self.apply(Transaction::delete(
            self.doc.text(),
            [(from, pos)].into_iter(),
        ));
    }

    /// Delete the grapheme under the cursor (`x` in normal mode).
    /// Never deletes the line ending.
    pub fn delete_char_at_cursor(&mut self) {
        let text = self.doc.text().slice(..);
        let pos = self.cursor_char_idx();
        if text.get_char(pos).is_none_or(|c| c == '\n') {
            return;
        }
        let to = next_grapheme_boundary(text, pos);
        self.apply(Transaction::delete(
            self.doc.text(),
            [(pos, to)].into_iter(),
        ));
        self.clamp_cursor_off_line_ending();
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
        let future = self
            .doc
            .save::<PathBuf>(None, false)
            .context("failed to start save")?;
        let event = self
            .backend
            .runtime
            .block_on(future)
            .context("save failed")?;
        self.doc
            .set_last_saved_revision(event.revision, event.save_time);
        self.edits_since_save = 0;
        Ok(())
    }

    // ---- introspection for status line ----

    pub fn display_name(&self) -> String {
        self.doc
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[scratch]".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(editor: &Editor) -> String {
        editor.lines(0..editor.line_count()).join("\n")
    }

    /// Scratch doc, typed `content` in insert mode, back to normal mode.
    fn editor_with(content: &str) -> Editor {
        let mut ed = Editor::scratch().unwrap();
        ed.enter_insert();
        ed.insert_str(content);
        ed.enter_normal();
        ed
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
        let path = std::env::temp_dir().join(format!("eggplant-nonl-{}", std::process::id()));
        std::fs::write(&path, "ab\ncde").unwrap();
        let mut ed = Editor::open(&path).unwrap();
        ed.move_last_line();
        ed.move_line_end();
        assert_eq!(ed.cursor(), (1, 2)); // on 'e', not 'd'
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn open_file_replaces_document() {
        let dir = std::env::temp_dir();
        let path_a = dir.join(format!("eggplant-a-{}", std::process::id()));
        let path_b = dir.join(format!("eggplant-b-{}", std::process::id()));
        std::fs::write(&path_a, "aaa").unwrap();
        std::fs::write(&path_b, "bbb\nccc").unwrap();

        let mut ed = Editor::open(&path_a).unwrap();
        let generation = ed.generation();
        ed.enter_insert();
        ed.insert_str("dirty");
        assert!(ed.is_modified());

        ed.open_file(&path_b).unwrap();
        assert_eq!(text_of(&ed), "bbb\nccc");
        assert!(!ed.is_modified());
        assert!(ed.generation() > generation);
        assert_eq!(ed.cursor(), (0, 0));

        std::fs::remove_file(&path_a).unwrap();
        std::fs::remove_file(&path_b).unwrap();
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
}
