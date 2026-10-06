//! The base editor surface: document text + gutter + cursor.
//! (The statusline is global chrome — see `crate::statusline`.)
//!
//! A pure *container* (Rule 5): it routes keys through `crate::editing`
//! (keys → actions → semantics), delegates scroll policy to
//! `crate::viewport`, moves the cursor through the `Editor` facade, and
//! maps state to props. It owns no logic of its own.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;

use crate::app::App;
use crate::components::editor::{self, EditorLine, EditorProps};
use crate::components::welcome::{self, WelcomeProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::editing::{self, Resolved, ViewAction};
use crate::element::Element;
use crate::viewport::Viewport;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct EditorSurface {
    viewport: Viewport,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    fn sync_viewport(&mut self, app: &App) {
        self.viewport
            .sync(app.editor.generation(), app.editor.cursor().0);
    }

    /// Viewport intents: scroll policy is the viewport's; cursor movement
    /// goes through the facade (buffer state).
    fn apply_view(&mut self, view: ViewAction, app: &mut App) {
        let (cursor_line, _) = app.editor.cursor();
        match view {
            ViewAction::CenterCursor => self.viewport.center_on(cursor_line),
            ViewAction::PageDown => {
                let last = app.editor.display_line_count().saturating_sub(1);
                let target = self.viewport.page_down(cursor_line, last);
                app.editor.move_to_line(target); // facade clamps
            }
            ViewAction::PageUp => {
                let target = self.viewport.page_up(cursor_line);
                app.editor.move_to_line(target);
            }
        }
    }
}

impl Layer for EditorSurface {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        if !app.editor.has_buffer() {
            return welcome::view(&WelcomeProps { version: VERSION }, area, &app.theme);
        }
        let leap = app.leap.as_ref();
        let first = self.viewport.first_visible();
        // Full viewport: rows past the file's end render as `~` (vim-style)
        // so `zz` can center even the last line. The gutter numbers only
        // user-counted lines (display_line_count).
        editor::view(
            &EditorProps {
                lines: (first..first + area.height as usize)
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
                scroll: first,
                line_count: app.editor.display_line_count(),
                cursor: app.editor.cursor(),
                // Dim once labels are up (leap's second phase).
                dim: leap.is_some_and(|leap| !leap.labels.is_empty()),
            },
            area,
            &app.theme,
        )
    }

    fn resize(&mut self, area: Rect, app: &App) {
        self.viewport.resize(area.height as usize);
        self.sync_viewport(app);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let result = match editing::resolve(&mut app.pending, app.editor.mode(), key, &app.keymaps)
        {
            Resolved::Swallowed => KeyResult::Consumed,
            Resolved::Ignored => KeyResult::Ignored,
            // Viewport intents belong to this layer (the scroll owner).
            Resolved::View(view) => {
                self.apply_view(view, app);
                KeyResult::Consumed
            }
            resolved => {
                editing::interpret_resolved(resolved, app);
                KeyResult::Consumed
            }
        };
        self.sync_viewport(app);
        result
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Base
    }

    fn id(&self) -> &'static str {
        "editor"
    }
}
