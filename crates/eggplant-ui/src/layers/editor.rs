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

impl EditorSurface {
    /// Viewport intents: `zz` centers the cursor line (no clamping against
    /// EOF — `~` rows fill past the end); `C-f`/`C-b` page cursor and
    /// scroll together, vim-style, clamped at the buffer ends.
    fn apply_view(&mut self, view: editing::ViewAction, app: &mut App) {
        let height = self.viewport_height.max(1);
        let (cursor_line, _) = app.editor.cursor();
        match view {
            editing::ViewAction::CenterCursor => {
                self.scroll = cursor_line.saturating_sub(height / 2);
            }
            editing::ViewAction::PageDown => {
                let last = app.editor.display_line_count().saturating_sub(1);
                app.editor.move_to_line(cursor_line + height); // facade clamps
                self.scroll = (self.scroll + height).min(last);
            }
            editing::ViewAction::PageUp => {
                app.editor.move_to_line(cursor_line.saturating_sub(height));
                self.scroll = self.scroll.saturating_sub(height);
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
        // Full viewport: rows past the file's end render as `~` (vim-style)
        // so `zz` can center even the last line. The gutter numbers only
        // user-counted lines (display_line_count).
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
        self.viewport_height = area.height as usize;
        self.ensure_cursor_visible(app);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_lines(n: usize) -> App {
        let mut app = App::new(eggplant_core::Editor::scratch().unwrap());
        app.editor.enter_insert();
        app.editor.insert_str(&"line\n".repeat(n));
        app.editor.enter_normal();
        app
    }

    #[test]
    fn zz_centers_the_cursor_line() {
        let mut app = app_with_lines(20);
        let mut surface = EditorSurface::new();
        surface.viewport_height = 5;

        app.editor.move_to_line(10);
        surface.apply_view(editing::ViewAction::CenterCursor, &mut app);
        assert_eq!(surface.scroll, 8, "cursor 10, half-viewport 2");

        // Near the top the scroll clamps at 0 (saturating).
        app.editor.move_to_line(1);
        surface.apply_view(editing::ViewAction::CenterCursor, &mut app);
        assert_eq!(surface.scroll, 0);

        // Near EOF it centers too — `~` rows fill past the end.
        app.editor.move_to_line(19);
        surface.apply_view(editing::ViewAction::CenterCursor, &mut app);
        assert_eq!(surface.scroll, 17);
    }

    #[test]
    fn ctrl_f_and_ctrl_b_page_cursor_and_scroll() {
        let mut app = app_with_lines(30);
        app.editor.move_to_line(0); // the fixture leaves the cursor at EOF
        let last = app.editor.display_line_count() - 1;
        let mut surface = EditorSurface::new();
        surface.viewport_height = 10;

        surface.apply_view(editing::ViewAction::PageDown, &mut app);
        assert_eq!(app.editor.cursor().0, 10);
        assert_eq!(surface.scroll, 10);

        // Clamps at the last line / scroll ceiling.
        surface.apply_view(editing::ViewAction::PageDown, &mut app);
        surface.apply_view(editing::ViewAction::PageDown, &mut app);
        assert_eq!(app.editor.cursor().0, last, "clamped at the last line");
        assert_eq!(surface.scroll, last);

        surface.apply_view(editing::ViewAction::PageUp, &mut app);
        assert_eq!(app.editor.cursor().0, last - 10);
        assert_eq!(surface.scroll, last - 10);

        // Clamps at the top.
        surface.apply_view(editing::ViewAction::PageUp, &mut app);
        surface.apply_view(editing::ViewAction::PageUp, &mut app);
        assert_eq!(app.editor.cursor().0, 0);
        assert_eq!(surface.scroll, 0);
    }
}
