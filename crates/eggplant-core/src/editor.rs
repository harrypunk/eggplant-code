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
    /// Unique identity: helix's `DocumentId::default` is always 1 (helix's
    /// own editor assigns unique ids; ours doesn't), so we track our own.
    slot: usize,
    doc: Document,
}

/// Live buffer-search state (`Space s b`, vim `/`-style): all matches as
/// sorted char ranges plus the index `n`/`N` cycle from.
#[derive(Debug, Clone)]
struct Search {
    /// Slot of the buffer the matches belong to (stale searches render
    /// nothing).
    buffer: usize,
    matches: Vec<std::ops::Range<usize>>,
    current: usize,
}

/// The yank register: text plus whether it was taken linewise (`yy`/`dd`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

/// Motions that operators (`d`/`y`) can consume, as char-index targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordForward,
    WordEnd,
    WordBackward,
    LineStart,
    LineEnd,
}

impl Motion {
    /// Inclusive motions include the target char. Only `$` needs the bump:
    /// helix's word-end head is already one past the last word char.
    fn inclusive(self) -> bool {
        matches!(self, Self::LineEnd)
    }
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
    fn new(doc: Document, slot: usize) -> Self {
        Self { slot, doc }
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
    /// Next unique buffer slot (monotonic; survives buffer closes).
    next_slot: usize,
    /// Index of the current buffer in `buffers`.
    /// Index of the current buffer; `None` when no buffer is open.
    current: Option<usize>,
    /// The single view (identity for selections + jumplist sync on undo/redo).
    view: View,
    mode: Mode,
    /// Bumped whenever the *identity* of the current document changes
    /// (open/switch/close), so the UI can drop per-document state.
    generation: usize,
    /// The yank register (shared across buffers, vim-style).
    register: Option<Register>,
    /// Live search matches (`None` = no active search).
    search: Option<Search>,
    backend: Backend,
}

impl Editor {
    /// Open a file (or an empty document if it doesn't exist yet).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let backend = Backend::new()?;
        let doc = backend.open_document(path.as_ref())?;
        Ok(Self::new(Buffer::new(doc, 0), backend))
    }

    /// A new empty scratch document.
    pub fn scratch() -> Result<Self> {
        let backend = Backend::new()?;
        let doc = backend.scratch_document();
        Ok(Self::new(Buffer::new(doc, 0), backend))
    }

    /// No buffers at all (directory startup: explorer + empty editor).
    pub fn empty() -> Result<Self> {
        Ok(Self {
            buffers: Vec::new(),
            next_slot: 0,
            current: None,
            view: View::new(DocumentId::default(), GutterConfig::default()),
            mode: Mode::Normal,
            generation: 0,
            register: None,
            search: None,
            backend: Backend::new()?,
        })
    }

    fn new(buffer: Buffer, backend: Backend) -> Self {
        let view = View::new(buffer.doc.id(), GutterConfig::default());
        let mut editor = Self {
            buffers: vec![buffer],
            next_slot: 1,
            current: Some(0),
            view,
            mode: Mode::Normal,
            generation: 0,
            register: None,
            search: None,
            backend,
        };
        editor.restore_cursor();
        editor
    }

    /// Arriving at a buffer: its Document remembers its own selection
    /// (helix stores selections per document) — re-assert normal-mode
    /// block-cursor invariants at the remembered position instead of
    /// resetting to the top. Per-buffer view memory relies on this.
    fn restore_cursor(&mut self) {
        self.mode = Mode::Normal;
        if self.current.is_none() {
            return; // closing the last buffer leaves the editor empty
        }
        // A document this view has never touched has no selection entry
        // yet (fresh from the backend) — start at the top.
        let head = match self.doc_opt() {
            Some(doc) if doc.selections().contains_key(&self.view.id) => self.cursor_char_idx(),
            _ => 0,
        };
        self.set_cursor(head);
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
        let slot = self.next_slot;
        self.next_slot += 1;
        self.buffers.push(Buffer::new(doc, slot));
        self.current = Some(self.buffers.len() - 1);
        self.restore_cursor();
        self.generation += 1;
        Ok(())
    }

    pub fn switch_buffer(&mut self, index: usize) -> Result<()> {
        if index >= self.buffers.len() {
            bail!("no buffer {}", index + 1);
        }
        if self.current != Some(index) {
            self.current = Some(index);
            self.restore_cursor();
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
        self.restore_cursor();
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

    /// Lines as a user counts them: excludes the phantom trailing empty
    /// line a file-final newline produces in the rope. Gutter numbering
    /// and viewport clamping use this, not `line_count`.
    pub fn display_line_count(&self) -> usize {
        if self.current.is_none() {
            return 0;
        }
        self.last_line() + 1
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

    /// Put the cursor at `pos`, respecting helix's block-cursor selection
    /// model: selections are always >= 1 grapheme wide and the range
    /// *direction* encodes the mode —
    ///
    /// - normal: forward `(pos, pos+1)`, block cursor sits at `pos`
    /// - insert: backward `(pos+1, pos)`, insert bar sits at `pos`
    /// - visual: the fixed end is preserved, the head follows `pos`
    ///   (flipping to a backward range when extending left past the anchor)
    fn set_cursor(&mut self, pos: usize) {
        let view_id = self.view.id;
        let len = self.doc().text().len_chars();
        let pos = pos.min(len);
        let range = match self.mode {
            Mode::Normal => Range::new(pos, (pos + 1).min(len)),
            Mode::Insert => Range::new((pos + 1).min(len), pos),
            Mode::Visual | Mode::VisualLine => {
                // The fixed end as an absolute char boundary: stored in
                // `anchor`, which is the start when forward but one past the
                // start when backward.
                let primary = self.doc().selection(view_id).primary();
                let fixed = if primary.head >= primary.anchor {
                    primary.anchor
                } else {
                    primary.anchor - 1
                };
                if pos >= fixed {
                    Range::new(fixed, (pos + 1).min(len))
                } else {
                    Range::new(fixed + 1, pos)
                }
            }
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
        // Insert mode backs off one char (the bar sits *between* chars);
        // visual mode keeps the cursor on the moving end.
        let was_insert = self.mode == Mode::Insert;
        let pos = self.cursor_char_idx();
        let line = self.doc().text().char_to_line(pos);
        let line_start = self.doc().text().line_to_char(line);
        let line_len = self.line_char_len(line);
        let col = pos - line_start;
        let target = if line_len == 0 {
            line_start
        } else {
            line_start
                + col
                    .saturating_sub(usize::from(was_insert))
                    .min(line_len - 1)
        };
        self.mode = Mode::Normal;
        self.set_cursor(target);
        // End of an insert session: commit it as one undo revision.
        self.commit_history();
    }

    /// Visual (charwise) mode: motions extend the selection, `d`/`y` act on
    /// it directly. The cursor char is included (vim convention).
    pub fn enter_visual(&mut self) {
        if self.current.is_none() || self.mode == Mode::Visual {
            return;
        }
        self.enter_visual_at_cursor(Mode::Visual);
    }

    /// Visual linewise mode (`V`): the selection always covers whole lines
    /// (including their newlines); vertical motions extend by lines.
    pub fn enter_visual_line(&mut self) {
        if self.current.is_none() || self.mode == Mode::VisualLine {
            return;
        }
        self.enter_visual_at_cursor(Mode::VisualLine);
    }

    fn enter_visual_at_cursor(&mut self, mode: Mode) {
        let pos = self.cursor_char_idx();
        let len = self.doc().text().len_chars();
        let view_id = self.view.id;
        self.mode = mode;
        self.doc_mut()
            .expect("current buffer")
            .set_selection(view_id, Selection::single(pos, (pos + 1).min(len)));
    }

    fn in_visual(&self) -> bool {
        matches!(self.mode, Mode::Visual | Mode::VisualLine)
    }

    /// The selected char range in charwise visual. Covers exactly the
    /// selected chars, including the one under the cursor.
    fn visual_char_range(&self) -> std::ops::Range<usize> {
        let primary = self.doc().selection(self.view.id).primary();
        primary.anchor.min(primary.head)..primary.anchor.max(primary.head)
    }

    /// Lines spanned by the visual selection (fixed end .. cursor line).
    fn visual_line_span(&self) -> (usize, usize) {
        let text = self.doc().text().slice(..);
        let primary = self.doc().selection(self.view.id).primary();
        let fixed = if primary.head >= primary.anchor {
            primary.anchor
        } else {
            primary.anchor.saturating_sub(1)
        };
        let cursor = self.cursor_char_idx();
        (
            text.char_to_line(fixed.min(cursor)),
            text.char_to_line(fixed.max(cursor)),
        )
    }

    /// The selected char range in linewise visual: whole lines including
    /// their newlines (so delete/yank/paste behave like `dd`/`yy`/`p`).
    fn visual_line_char_range(&self) -> std::ops::Range<usize> {
        let text = self.doc().text();
        let (lo, hi) = self.visual_line_span();
        let start = text.line_to_char(lo);
        let end = if hi + 1 < text.len_lines() {
            text.line_to_char(hi + 1)
        } else {
            text.len_chars()
        };
        start..end
    }

    /// The effective selection char range for the current visual mode.
    fn selection_range(&self) -> std::ops::Range<usize> {
        match self.mode {
            Mode::VisualLine => self.visual_line_char_range(),
            _ => self.visual_char_range(),
        }
    }

    /// Visual `d`/`x`: delete the selection (also yanks), back to normal.
    pub fn delete_selection(&mut self) {
        if !self.in_visual() || self.current.is_none() {
            return;
        }
        let linewise = self.mode == Mode::VisualLine;
        let range = self.selection_range();
        if range.is_empty() {
            return;
        }
        let text = self.doc().text().slice(range.clone()).to_string();
        self.register = Some(Register { text, linewise });
        self.mode = Mode::Normal; // before delete: cursor collapses normally
        self.apply_delete(range);
    }

    /// Visual `y`: yank the selection, back to normal (cursor at its start).
    pub fn yank_selection(&mut self) {
        if !self.in_visual() || self.current.is_none() {
            return;
        }
        let linewise = self.mode == Mode::VisualLine;
        let range = self.selection_range();
        let text = self.doc().text().slice(range.clone()).to_string();
        self.register = Some(Register { text, linewise });
        self.mode = Mode::Normal;
        self.set_cursor(range.start);
    }

    /// Visual selection intersected with `line`, as char columns
    /// `[start, end)`. `None` outside visual mode / off the selection.
    pub fn visual_selection_on_line(&self, line: usize) -> Option<(usize, usize)> {
        if !self.in_visual() || self.current.is_none() {
            return None;
        }
        let text = self.doc().text();
        if line >= text.len_lines() {
            return None;
        }
        if self.mode == Mode::VisualLine {
            let (lo, hi) = self.visual_line_span();
            let len = self.line_char_len(line);
            return ((lo..=hi).contains(&line) && len > 0).then_some((0, len));
        }
        let selection = self.visual_char_range();
        let line_start = text.line_to_char(line);
        let line_end = line_start + self.line_char_len(line);
        let start = selection.start.max(line_start);
        let end = selection.end.min(line_end);
        // Lazy: the subtractions are only valid inside the intersection.
        (start < end).then(|| (start - line_start, end - line_start))
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
        // Clamp to the last *real* line — the rope's phantom trailing line
        // is not a place the cursor may rest.
        let target = line.saturating_add_signed(delta).min(self.last_line());
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
            Mode::Normal | Mode::Visual | Mode::VisualLine => len.saturating_sub(1),
            Mode::Insert => len,
        };
        self.set_cursor(start + col.min(max_col));
    }

    /// Char length of a line's content, excluding the line ending (if any).
    /// Out-of-range lines (past the rope's end) report 0.
    pub fn line_char_len(&self, line: usize) -> usize {
        let text = self.doc().text();
        let line = line.min(text.len_lines().saturating_sub(1));
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
        self.set_cursor_on_line(self.last_line(), col);
    }

    /// The last line the cursor may sit on: a file ending in a newline has
    /// a phantom empty line after the content, which is not a jump target.
    fn last_line(&self) -> usize {
        let last = self.line_count().saturating_sub(1);
        if last > 0 && self.line_char_len(last) == 0 {
            last - 1
        } else {
            last
        }
    }

    /// Go to `line` (0-based, clamped), keeping the column where possible.
    pub fn move_to_line(&mut self, line: usize) {
        if self.current.is_none() {
            return;
        }
        let (_, col) = self.cursor();
        let target = line.min(self.last_line());
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

    /// `gg`: first line, or line `count` with a count (`5gg` → line 5).
    pub fn move_first_line(&mut self, count: usize) {
        self.move_to_line(count.saturating_sub(1));
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

    // ---- search ----

    /// Set/replace the search pattern (live, incsearch-style): find all
    /// matches, jump the cursor to the first one at/after it (wrapping),
    /// return the match count. An empty pattern clears the search.
    pub fn search(&mut self, pattern: &str) -> usize {
        if self.current.is_none() || pattern.is_empty() {
            self.search = None;
            return 0;
        }
        let rope = self.doc().text();
        let haystack = rope.slice(..).to_string();
        let matches: Vec<std::ops::Range<usize>> = haystack
            .match_indices(pattern)
            .map(|(byte, m)| {
                let start = rope.byte_to_char(byte);
                start..start + m.chars().count()
            })
            .collect();
        if matches.is_empty() {
            self.search = None;
            return 0;
        }
        let cursor = self.cursor_char_idx();
        let current = matches.iter().position(|m| m.start >= cursor).unwrap_or(0);
        let count = matches.len();
        self.search = Some(Search {
            buffer: self.buffers[self.current.expect("checked")].slot,
            matches,
            current,
        });
        self.jump_to_search_match();
        count
    }

    /// `n`: jump to the next match (wraps). No-op without an active search.
    pub fn next_search_match(&mut self) {
        self.cycle_search_match(1);
    }

    /// `N`: jump to the previous match (wraps).
    pub fn prev_search_match(&mut self) {
        self.cycle_search_match(-1);
    }

    fn cycle_search_match(&mut self, delta: isize) {
        if self.current.is_none() {
            return;
        }
        if self
            .search
            .as_ref()
            .is_some_and(|s| Some(s.buffer) != self.current_slot())
        {
            self.search = None; // stale: belongs to another buffer
            return;
        }
        let Some(search) = &mut self.search else {
            return;
        };
        let len = search.matches.len() as isize;
        let next = (search.current as isize + delta).rem_euclid(len) as usize;
        let pos = search.matches[next].start;
        search.current = next;
        self.set_cursor(pos);
        self.clamp_cursor_off_line_ending();
    }

    fn jump_to_search_match(&mut self) {
        let pos = self.search.as_ref().map(|s| s.matches[s.current].start);
        if let Some(pos) = pos {
            self.set_cursor(pos);
            self.clamp_cursor_off_line_ending();
        }
    }

    /// Slot of the current buffer (`None` when bufferless).
    /// Stable id of the current buffer (monotonic, survives closes) —
    /// the key for per-buffer UI state (view memory).
    pub fn current_slot(&self) -> Option<usize> {
        self.current.map(|i| self.buffers[i].slot)
    }

    /// Clear the search highlight (`Esc`).
    pub fn clear_search(&mut self) {
        self.search = None;
    }

    /// Whether a search is active on the current buffer.
    pub fn has_search(&self) -> bool {
        self.current.is_some()
            && self
                .search
                .as_ref()
                .is_some_and(|s| Some(s.buffer) == self.current_slot())
    }

    /// All buffer lines as plain text (picker source for buffer grep).
    /// Empty when bufferless.
    pub fn buffer_lines(&self) -> Vec<String> {
        self.doc_opt()
            .map(|doc| doc.text().lines().map(|line| line.to_string()).collect())
            .unwrap_or_default()
    }

    /// All occurrences of `pattern` as `(line, col)`, in document order.
    /// Leap-jump's label source (stateless, unlike `search`).
    pub fn find_matches(&self, pattern: &str) -> Vec<(usize, usize)> {
        let Some(doc) = self.doc_opt() else {
            return Vec::new();
        };
        if pattern.is_empty() {
            return Vec::new();
        }
        let text = doc.text();
        let haystack = text.slice(..).to_string();
        haystack
            .match_indices(pattern)
            .map(|(byte, _)| {
                let ch = text.byte_to_char(byte);
                let line = text.char_to_line(ch);
                (line, ch - text.line_to_char(line))
            })
            .collect()
    }

    /// Leap: move the cursor to `(line, col)`, clamped.
    pub fn jump_to(&mut self, line: usize, col: usize) {
        if self.current.is_none() {
            return;
        }
        let line = line.min(self.last_line());
        self.set_cursor_on_line(line, col);
        self.clamp_cursor_off_line_ending();
    }

    /// Search matches intersecting `line` as char columns
    /// `(start, end, is_current)`. Empty outside an active search.
    pub fn search_marks_on_line(&self, line: usize) -> Vec<(usize, usize, bool)> {
        if !self.has_search() {
            return Vec::new();
        }
        let text = self.doc().text();
        if line >= text.len_lines() {
            return Vec::new();
        }
        let line_start = text.line_to_char(line);
        let line_end = line_start + self.line_char_len(line);
        let search = self.search.as_ref().expect("has_search checked");
        search
            .matches
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let start = m.start.max(line_start);
                let end = m.end.min(line_end);
                // Lazy: the subtractions only hold inside the intersection.
                (start < end).then(|| (start - line_start, end - line_start, i == search.current))
            })
            .collect()
    }

    // ---- edits ----

    fn apply(&mut self, transaction: Transaction) {
        if self.current.is_none() {
            return;
        }
        self.search = None; // edits invalidate match positions
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

    // ---- operators (delete / yank / paste) ----

    /// The char range an operator covers for `motion` with `count`
    /// (inclusive motions include the target char). Empty when no buffer.
    pub fn operator_range(&self, motion: Motion, count: usize) -> std::ops::Range<usize> {
        let Some(_) = self.current else { return 0..0 };
        let text = self.doc().text().slice(..);
        let range = self.doc().selection(self.view.id).primary();
        let cursor = self.cursor_char_idx();
        let (line, col) = self.cursor();
        let line_start = text.line_to_char(line);
        let line_len = self.line_char_len(line);
        let target = match motion {
            Motion::Left => line_start + col.saturating_sub(count),
            Motion::Right => line_start + (col + count).min(line_len.saturating_sub(1)),
            Motion::WordForward => move_next_word_start(text, range, count).head,
            Motion::WordEnd => move_next_word_end(text, range, count).head,
            Motion::WordBackward => move_prev_word_start(text, range, count).head,
            Motion::LineStart => line_start,
            Motion::LineEnd => line_start + line_len.saturating_sub(1),
        };
        let start = cursor.min(target);
        let mut end = cursor.max(target);
        if motion.inclusive() && end < text.len_chars() {
            end = next_grapheme_boundary(text, end);
        }
        start..end
    }

    /// Yank a char range into the register (charwise). Empty range: no-op.
    pub fn yank_range(&mut self, range: std::ops::Range<usize>) {
        if self.current.is_none() || range.is_empty() {
            return;
        }
        let text = self.doc().text().slice(range.start..range.end).to_string();
        self.register = Some(Register {
            text,
            linewise: false,
        });
    }

    /// Delete a char range; the cursor lands on the range start.
    pub fn delete_range(&mut self, range: std::ops::Range<usize>) {
        if self.current.is_none() || range.is_empty() {
            return;
        }
        self.yank_range(range.clone()); // vim convention: delete also yanks
        self.apply_delete(range);
    }

    /// Delete `range` (already yanked): cursor to the range start, one undo
    /// revision.
    fn apply_delete(&mut self, range: std::ops::Range<usize>) {
        self.apply(Transaction::delete(
            self.doc().text(),
            [(range.start, range.end)].into_iter(),
        ));
        self.set_cursor(
            range
                .start
                .min(self.doc().text().len_chars().saturating_sub(1)),
        );
        self.clamp_cursor_off_line_ending();
        self.commit_history();
    }

    /// Yank the current line including its newline (`yy`).
    pub fn yank_line(&mut self) {
        let Some(range) = self.current_line_range() else {
            return;
        };
        let text = self.doc().text().slice(range).to_string();
        self.register = Some(Register {
            text,
            linewise: true,
        });
    }

    /// Delete the current line including its newline (`dd`).
    pub fn delete_line(&mut self) {
        let Some(range) = self.current_line_range() else {
            return;
        };
        if range.is_empty() {
            return; // phantom empty last line: nothing to delete
        }
        self.yank_line();
        self.apply_delete(range);
    }

    /// Char range of the current line including its line ending.
    fn current_line_range(&self) -> Option<std::ops::Range<usize>> {
        self.current?;
        let text = self.doc().text();
        let (line, _) = self.cursor();
        let start = text.line_to_char(line);
        let end = text.line_to_char((line + 1).min(text.len_lines() - 1));
        let end = if line + 1 < text.len_lines() {
            end
        } else {
            text.len_chars()
        };
        Some(start..end)
    }

    /// Paste the register after the cursor (`p`): charwise after the cursor
    /// char; linewise on the line below. No-op with an empty register.
    pub fn paste_after(&mut self) {
        let Some(register) = self.register.clone() else {
            return;
        };
        let Some(_) = self.current else { return };
        let insert_at = if register.linewise {
            let text = self.doc().text();
            let (line, _) = self.cursor();
            text.line_to_char((line + 1).min(text.len_lines() - 1))
        } else {
            let text = self.doc().text().slice(..);
            let (line, _) = self.cursor();
            let line_start = text.line_to_char(line);
            let line_end = line_start + self.line_char_len(line);
            next_grapheme_boundary(text, self.cursor_char_idx()).min(line_end)
        };
        self.apply(Transaction::insert(
            self.doc().text(),
            &Selection::point(insert_at),
            register.text.into(),
        ));
        // Land the cursor at the start of the pasted text.
        self.set_cursor(insert_at);
        self.clamp_cursor_off_line_ending();
        self.commit_history();
    }

    /// Current register contents (for tests/status display).
    pub fn register(&self) -> Option<&Register> {
        self.register.as_ref()
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
    fn switching_buffers_restores_the_remembered_cursor() {
        let a = temp_file("view-mem-a", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
        let b = temp_file("view-mem-b", "x\ny\nz\n");
        let mut ed = Editor::open(&a).unwrap();
        ed.open_buffer(&b).unwrap();
        ed.switch_buffer(0).unwrap();
        ed.move_to_line(7); // deep in file A
        assert_eq!(ed.cursor().0, 7);

        ed.switch_buffer(1).unwrap();
        assert_eq!(ed.cursor(), (0, 0), "B was never visited: top");

        ed.move_to_line(2);
        ed.switch_buffer(0).unwrap();
        assert_eq!(ed.cursor().0, 7, "A remembers its cursor");

        ed.switch_buffer(1).unwrap();
        assert_eq!(ed.cursor().0, 2, "B remembers too");
        assert_eq!(ed.mode(), Mode::Normal, "mode is normalized on arrival");

        std::fs::remove_file(a).unwrap();
        std::fs::remove_file(b).unwrap();
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

    // ---- operators: d / y / p ----

    #[test]
    fn dw_deletes_to_next_word_start() {
        let mut ed = editor_with("foo bar baz");
        ed.move_to_line(0);
        ed.move_line_start();
        let range = ed.operator_range(Motion::WordForward, 1);
        ed.delete_range(range);
        assert_eq!(text_of(&ed), "bar baz\n");
        assert_eq!(ed.register().unwrap().text, "foo "); // delete also yanks
        assert_eq!(ed.cursor(), (0, 0));
    }

    #[test]
    fn de_is_inclusive_of_word_end() {
        let mut ed = editor_with("foo bar");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.delete_range(ed.operator_range(Motion::WordEnd, 1));
        assert_eq!(text_of(&ed), " bar\n");
    }

    #[test]
    fn d_dollar_keeps_the_newline() {
        let mut ed = editor_with("foo bar\nnext");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.delete_range(ed.operator_range(Motion::LineEnd, 1));
        assert_eq!(text_of(&ed), "\nnext\n");
    }

    #[test]
    fn db_from_word_start_deletes_the_previous_word() {
        let mut ed = editor_with("foo bar");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.move_right(4); // on 'b' of "bar"
        ed.delete_range(ed.operator_range(Motion::WordBackward, 1));
        assert_eq!(text_of(&ed), "bar\n");
    }

    #[test]
    fn db_from_word_middle_deletes_to_word_start() {
        let mut ed = editor_with("foo bar");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.move_right(5); // on 'a' of "bar"
        ed.delete_range(ed.operator_range(Motion::WordBackward, 1));
        assert_eq!(text_of(&ed), "foo ar\n", "cursor char survives");
    }

    #[test]
    fn yy_p_duplicates_the_line_below() {
        let mut ed = editor_with("abc\ndef");
        ed.move_to_line(0);
        ed.yank_line();
        assert!(ed.register().unwrap().linewise);
        ed.paste_after();
        assert_eq!(text_of(&ed), "abc\nabc\ndef\n");
        assert_eq!(ed.cursor().0, 1, "cursor lands on the pasted line");
    }

    #[test]
    fn dd_deletes_line_and_p_restores_it_below() {
        let mut ed = editor_with("one\ntwo\nthree");
        ed.move_to_line(0);
        ed.delete_line();
        assert_eq!(text_of(&ed), "two\nthree\n");
        assert_eq!(ed.cursor(), (0, 0));
        ed.paste_after(); // paste "one" below "two"
        assert_eq!(text_of(&ed), "two\none\nthree\n");
    }

    #[test]
    fn charwise_yank_and_paste_after_cursor() {
        let mut ed = editor_with("foo bar");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.yank_range(ed.operator_range(Motion::WordEnd, 1)); // "foo"
        assert!(!ed.register().unwrap().linewise);
        ed.paste_after(); // after the 'f'
        assert_eq!(text_of(&ed), "ffoooo bar\n");
    }

    #[test]
    fn dd_is_undoable_as_one_revision() {
        let mut ed = editor_with("one\ntwo");
        ed.move_to_line(0);
        ed.delete_line();
        assert_eq!(text_of(&ed), "two\n");
        assert!(ed.undo());
        assert_eq!(text_of(&ed), "one\ntwo\n");
    }

    // ---- visual mode ----

    #[test]
    fn visual_extend_right_then_delete() {
        let mut ed = editor_with("hello world");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.enter_visual();
        assert_eq!(ed.mode(), Mode::Visual);
        ed.move_right(4); // select "hello"
        assert_eq!(ed.visual_selection_on_line(0), Some((0, 5)));
        ed.delete_selection();
        assert_eq!(ed.mode(), Mode::Normal);
        assert_eq!(text_of(&ed), " world\n");
        assert_eq!(ed.register().unwrap().text, "hello");
        assert!(ed.undo(), "one revision");
        assert_eq!(text_of(&ed), "hello world\n");
    }

    #[test]
    fn visual_extend_left_past_anchor() {
        let mut ed = editor_with("hello");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.move_right(4); // cursor on 'o'
        ed.enter_visual();
        ed.move_left(2); // select "llo" (anchor char included)
        assert_eq!(ed.visual_selection_on_line(0), Some((2, 5)));
        assert_eq!(ed.cursor(), (0, 2), "cursor rides the moving end");
        ed.yank_selection();
        assert_eq!(ed.register().unwrap().text, "llo");
        assert_eq!(ed.mode(), Mode::Normal);
        assert_eq!(ed.cursor(), (0, 2), "cursor at selection start");
    }

    #[test]
    fn visual_shrinks_back_to_anchor() {
        let mut ed = editor_with("hello");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.enter_visual();
        ed.move_right(2);
        assert_eq!(ed.visual_selection_on_line(0), Some((0, 3)));
        ed.move_left(2);
        assert_eq!(ed.visual_selection_on_line(0), Some((0, 1)));
    }

    #[test]
    fn visual_word_motion_extends() {
        let mut ed = editor_with("foo bar baz");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.enter_visual();
        ed.move_word_forward(1);
        // vim convention: visual `w` includes the next word's first char.
        assert_eq!(ed.visual_selection_on_line(0), Some((0, 5)));
    }

    #[test]
    fn visual_multiline_selection_reports_per_line_cols() {
        let mut ed = editor_with("ab\ncd\nef");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.move_right(1); // on 'b'
        ed.enter_visual();
        ed.move_down(1); // onto line 1 col 1
        assert_eq!(ed.visual_selection_on_line(0), Some((1, 2)));
        assert_eq!(ed.visual_selection_on_line(1), Some((0, 2)));
        assert_eq!(ed.visual_selection_on_line(2), None);
    }

    #[test]
    fn esc_exits_visual_keeping_cursor() {
        let mut ed = editor_with("hello");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.move_right(2);
        ed.enter_visual();
        ed.move_right(1);
        ed.enter_normal();
        assert_eq!(ed.mode(), Mode::Normal);
        assert_eq!(ed.cursor(), (0, 3));
        assert_eq!(ed.visual_selection_on_line(0), None);
    }

    // ---- visual line mode ----

    #[test]
    fn visual_line_covers_whole_lines() {
        let mut ed = editor_with("abc\ndef\nghi");
        ed.move_to_line(1);
        ed.move_right(1); // mid-line
        ed.enter_visual_line();
        assert_eq!(ed.mode(), Mode::VisualLine);
        assert_eq!(ed.visual_selection_on_line(1), Some((0, 3)));
        assert_eq!(ed.visual_selection_on_line(0), None);

        ed.move_down(1); // extend over "ghi"
        assert_eq!(ed.visual_selection_on_line(1), Some((0, 3)));
        assert_eq!(ed.visual_selection_on_line(2), Some((0, 3)));
    }

    #[test]
    fn visual_line_delete_removes_lines_linewise() {
        let mut ed = editor_with("one\ntwo\nthree");
        ed.move_to_line(0);
        ed.enter_visual_line();
        ed.move_down(1);
        ed.delete_selection();
        assert_eq!(text_of(&ed), "three\n");
        let register = ed.register().unwrap();
        assert!(register.linewise, "V-delete yanks linewise");
        assert_eq!(register.text, "one\ntwo\n");
        assert_eq!(ed.mode(), Mode::Normal);
        assert!(ed.undo());
        assert_eq!(text_of(&ed), "one\ntwo\nthree\n");
    }

    #[test]
    fn visual_line_yank_then_paste_duplicates_below() {
        let mut ed = editor_with("one\ntwo");
        ed.move_to_line(0);
        ed.enter_visual_line();
        ed.yank_selection();
        assert!(ed.register().unwrap().linewise);
        assert_eq!(ed.mode(), Mode::Normal);
        ed.paste_after();
        assert_eq!(text_of(&ed), "one\none\ntwo\n");
    }

    #[test]
    fn visual_line_extends_upwards() {
        let mut ed = editor_with("one\ntwo\nthree");
        ed.move_to_line(2);
        ed.enter_visual_line();
        ed.move_up(2);
        assert_eq!(ed.visual_selection_on_line(0), Some((0, 3)));
        assert_eq!(ed.visual_selection_on_line(2), Some((0, 5)));
        ed.delete_selection();
        assert_eq!(
            text_of(&ed),
            "",
            "deleting all lines leaves an empty buffer"
        );
    }

    // ---- key sequences (gg) ----

    #[test]
    fn gg_goes_to_first_line_and_counts() {
        let mut ed = editor_with("one\ntwo\nthree\nfour");
        ed.move_to_line(3);
        ed.move_first_line(1); // bare gg
        assert_eq!(ed.cursor().0, 0);
        ed.move_first_line(3); // 3gg → line 3
        assert_eq!(ed.cursor().0, 2);
    }

    #[test]
    fn gg_in_visual_extends_to_the_top() {
        let mut ed = editor_with("one\ntwo\nthree");
        ed.move_to_line(2); // cursor col 4 (post-Esc), clamped to 2 on jump
        ed.enter_visual();
        ed.move_first_line(1); // gg keeps the column, like G
        assert_eq!(ed.visual_selection_on_line(0), Some((2, 3)));
        assert_eq!(ed.visual_selection_on_line(2), Some((0, 5)));
    }

    // ---- search ----

    #[test]
    fn search_finds_all_matches_and_jumps_incsearch_style() {
        let mut ed = editor_with("foo bar foo\nbaz foo");
        ed.move_to_line(0);
        ed.move_line_start();
        assert_eq!(ed.search("foo"), 3);
        assert_eq!(ed.cursor(), (0, 0), "first match at/after cursor");
        assert!(ed.has_search());
        // line 0: "foo bar foo" → matches at cols 0..3 and 8..11
        assert_eq!(
            ed.search_marks_on_line(0),
            vec![(0, 3, true), (8, 11, false)]
        );
        assert_eq!(ed.search_marks_on_line(1), vec![(4, 7, false)]);
    }

    #[test]
    fn search_cycles_with_wraparound() {
        let mut ed = editor_with("a x\nx b\nc x");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.search("x");
        assert_eq!(ed.cursor(), (0, 2));
        ed.next_search_match();
        assert_eq!(ed.cursor(), (1, 0));
        ed.next_search_match();
        assert_eq!(ed.cursor(), (2, 2));
        ed.next_search_match();
        assert_eq!(ed.cursor(), (0, 2), "wraps to first");
        ed.prev_search_match();
        assert_eq!(ed.cursor(), (2, 2), "N wraps to last");
        // the current mark follows the cursor
        assert_eq!(ed.search_marks_on_line(2), vec![(2, 3, true)]);
    }

    #[test]
    fn search_starts_from_cursor_position() {
        let mut ed = editor_with("foo foo foo");
        ed.move_to_line(0);
        ed.move_line_start();
        ed.move_right(5); // past the second foo's start
        ed.search("foo");
        assert_eq!(ed.cursor(), (0, 8), "lands on next match, not first");
    }

    #[test]
    fn search_empty_or_missing_pattern_clears() {
        let mut ed = editor_with("hello");
        assert_eq!(ed.search("zzz"), 0);
        assert!(!ed.has_search());
        ed.search("ell");
        assert!(ed.has_search());
        ed.search("");
        assert!(!ed.has_search());
        assert_eq!(ed.search_marks_on_line(0), Vec::new());
    }

    #[test]
    fn search_is_invalidated_by_edits_and_cleared_on_esc() {
        let mut ed = editor_with("foo foo");
        ed.search("foo");
        assert!(ed.has_search());
        ed.enter_insert();
        ed.insert_char('x');
        ed.enter_normal();
        assert!(!ed.has_search(), "edits drop stale match positions");

        ed.search("foo");
        ed.clear_search();
        assert!(!ed.has_search());
    }

    #[test]
    fn search_marks_nothing_on_other_buffers() {
        let mut ed = editor_with("foo");
        ed.search("foo");
        let other = temp_file("other.txt", "foo foo");
        ed.open_buffer(&other).unwrap();
        assert!(!ed.has_search());
        assert_eq!(ed.search_marks_on_line(0), Vec::new());
    }

    #[test]
    fn dbg_search_buffer_ids() {
        let mut ed = editor_with("foo");
        ed.search("foo");
        let other = temp_file("other2.txt", "foo foo");
        ed.open_buffer(&other).unwrap();
        eprintln!("has_search={}", ed.has_search());
        eprintln!(
            "current={:?} buffers={}",
            ed.current_buffer(),
            ed.buffer_count()
        );
    }
    #[test]
    fn find_matches_returns_line_col_pairs() {
        let ed = editor_with("ab ab\nxab");
        assert_eq!(ed.find_matches("ab"), vec![(0, 0), (0, 3), (1, 1)]);
        assert_eq!(ed.find_matches(""), Vec::new());
        assert_eq!(ed.find_matches("zz"), Vec::new());
    }

    #[test]
    fn move_down_stops_at_the_last_real_line() {
        // A trailing-newline file has a phantom rope line below the last
        // real one; `j` must not step onto it.
        let path = temp_file("j-eof", "one\ntwo\n");
        let mut ed = Editor::open(&path).unwrap();
        ed.move_down(1);
        assert_eq!(ed.cursor().0, 1);
        ed.move_down(1);
        assert_eq!(ed.cursor().0, 1, "no cursor below the last real line");
        ed.move_down(99);
        assert_eq!(ed.cursor().0, 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn display_line_count_matches_user_counted_lines() {
        // Real file loads: a file-final newline is a terminator, not an
        // extra line. (Scratch docs can't probe this: they start with a
        // placeholder newline.)
        let cases = [("one\ntwo\n", 2), ("one\ntwo", 2), ("one\n", 1), ("", 1)];
        for (i, (content, want)) in cases.into_iter().enumerate() {
            let path = temp_file(&format!("dlc-{i}"), content);
            let ed = Editor::open(&path).unwrap();
            assert_eq!(ed.display_line_count(), want, "{content:?}");
            std::fs::remove_file(&path).ok();
        }
        assert_eq!(Editor::empty().unwrap().display_line_count(), 0);
    }

    #[test]
    fn jump_to_moves_and_clamps() {
        let mut ed = editor_with("one\ntwo\nthree");
        ed.jump_to(1, 1);
        assert_eq!(ed.cursor(), (1, 1));
        ed.jump_to(99, 99);
        assert_eq!(ed.cursor().0, 2, "line clamps to the last line");
        assert!(ed.cursor().1 <= 5, "col clamps into the line");
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
