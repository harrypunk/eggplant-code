//! The base editor surface: document text + gutter + cursor.
//! (The statusline is global chrome — see `crate::statusline`.)
//!
//! All input logic lives in `crate::editing` (keys → actions → semantics);
//! this container only routes keys through it, keeps the cursor visible,
//! and maps state to props (Rule 5).

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

use crate::app::App;
use crate::components::editor::{self, EditorLine, EditorProps};
use crate::components::welcome::{self, WelcomeProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::editing::{self, Resolved};
use crate::element::Element;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct EditorSurface {
    /// First visible line (vertical scroll offset).
    scroll: usize,
    /// Document generation we last saw; a change resets the scroll.
    seen_generation: usize,
    /// Editor height from the compositor's `resize` hook, so key handling can
    /// keep the cursor visible. Updated outside `render` (Rule 5).
    viewport_height: usize,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_cursor_visible(&mut self, app: &App) {
        // Document replaced/switched? Drop per-document state.
        if app.editor.generation() != self.seen_generation {
            self.seen_generation = app.editor.generation();
            self.scroll = 0;
        }
        let cursor_line = app.editor.cursor().0;
        let height = self.viewport_height.max(1);
        if cursor_line < self.scroll {
            self.scroll = cursor_line;
        } else if cursor_line >= self.scroll + height {
            self.scroll = cursor_line + 1 - height;
        }
    }
}

impl Layer for EditorSurface {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        if !app.editor.has_buffer() {
            return welcome::view(&WelcomeProps { version: VERSION }, area, &app.theme);
        }
        let leap = app.leap.as_ref();
        editor::view(
            &EditorProps {
                lines: (self.scroll..self.scroll + area.height as usize)
                    .map(|line| EditorLine {
                        spans: app.editor.highlighted_line(line),
                        selection: app.editor.visual_selection_on_line(line),
                        search_marks: app.editor.search_marks_on_line(line),
                        labels: leap
                            .map(|leap| {
                                leap.labels
                                    .iter()
                                    .filter(|label| label.line == line)
                                    .map(|label| (label.col, label.label))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect(),
                scroll: self.scroll,
                line_count: app.editor.line_count(),
                cursor: app.editor.cursor(),
                // Dim once labels are up (leap's second phase).
                dim: leap.is_some_and(|leap| !leap.labels.is_empty()),
            },
            area,
            &app.theme,
        )
    }

    fn resize(&mut self, area: Rect, app: &App) {
        self.viewport_height = area.height as usize;
        self.ensure_cursor_visible(app);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let result = match editing::resolve(&mut app.pending, app.editor.mode(), key) {
            Resolved::Swallowed => KeyResult::Consumed,
            Resolved::Ignored => KeyResult::Ignored,
            resolved => {
                editing::interpret_resolved(resolved, app);
                KeyResult::Consumed
            }
        };
        self.ensure_cursor_visible(app);
        result
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Base
    }

    fn id(&self) -> &'static str {
        "editor"
    }
}
