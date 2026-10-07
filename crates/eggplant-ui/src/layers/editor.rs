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
    /// Per-buffer view memory: one viewport per buffer slot (slots are
    /// stable, monotonic ids). A slot's viewport survives switching away
    /// and back — the cursor does too (the facade restores it).
    viewports: std::collections::HashMap<usize, Viewport>,
    /// Last area from the compositor's `resize` hook — text width feeds
    /// viewport sync (horizontal scroll, wrap height).
    area: Rect,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current buffer's viewport (created on first sight).
    fn viewport(&mut self, app: &App) -> Option<&mut Viewport> {
        let slot = app.editor.current_slot()?;
        let viewport = self.viewports.entry(slot).or_default();
        viewport.resize(self.area.height as usize);
        Some(viewport)
    }

    fn sync_viewport(&mut self, app: &App) {
        let slot = app.editor.current_slot();
        let wrap = app.wrap;
        let width = self.text_width(app);
        let Some(viewport) = self.viewport(app) else {
            return; // no buffer: nothing to sync
        };
        // The slot IS the identity of what this viewport shows (one slot,
        // one document, ever) — no generation reset on switching.
        viewport.sync(
            slot.unwrap_or(0),
            app.editor.cursor(),
            wrap,
            width,
            &|line| app.editor.line_char_len(line),
        );
    }

    /// Text columns available after the gutter.
    fn text_width(&self, app: &App) -> usize {
        (self.area.width as usize)
            .saturating_sub(editor::gutter_width(app.editor.display_line_count()))
    }

    /// Viewport intents: scroll policy is the viewport's; cursor movement
    /// goes through the facade (buffer state).
    fn apply_view(&mut self, view: ViewAction, app: &mut App) {
        let (cursor_line, _) = app.editor.cursor();
        let Some(viewport) = self.viewport(app) else {
            return;
        };
        match view {
            ViewAction::CenterCursor => viewport.center_on(cursor_line),
            ViewAction::PageDown => {
                let last = app.editor.display_line_count().saturating_sub(1);
                let target = viewport.page_down(cursor_line, last);
                app.editor.move_to_line(target); // facade clamps
            }
            ViewAction::PageUp => {
                let target = viewport.page_up(cursor_line);
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
        let width = self.text_width(app);
        let line_count = app.editor.display_line_count();
        let viewport = app
            .editor
            .current_slot()
            .and_then(|slot| self.viewports.get(&slot).copied())
            .unwrap_or_default();
        let rows = viewport
            .layout_rows(app.wrap, width, &|line| app.editor.line_char_len(line))
            .into_iter()
            .map(|row| {
                let gutter = if row.line >= line_count {
                    editor::GutterMark::PastEnd
                } else if row.start_col == 0 {
                    editor::GutterMark::Number(row.line)
                } else {
                    editor::GutterMark::Continuation
                };
                editor::RowProps {
                    line: EditorLine {
                        spans: app.editor.highlighted_line(row.line),
                        selection: app.editor.visual_selection_on_line(row.line),
                        search_marks: app.editor.search_marks_on_line(row.line),
                        labels: app.line_labels(row.line),
                    },
                    doc_line: row.line,
                    start_col: row.start_col,
                    gutter,
                }
            })
            .collect();
        editor::view(
            &EditorProps {
                rows,
                line_count,
                cursor: app.editor.cursor(),
                dim: app.dims_editor_text(),
            },
            area,
            &app.theme,
        )
    }

    fn resize(&mut self, area: Rect, app: &App) {
        self.area = area;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    fn two_buffer_app() -> App {
        let dir = std::env::temp_dir().join(format!("eggplant-vm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lines = |n: usize| (1..=n).map(|i| format!("line {i}\n")).collect::<String>();
        std::fs::write(dir.join("a.txt"), lines(200)).unwrap();
        std::fs::write(dir.join("b.txt"), lines(200)).unwrap();
        let editor = eggplant_core::Editor::open(dir.join("a.txt")).unwrap();
        let mut app = App::new(editor);
        app.editor.open_buffer(dir.join("b.txt")).unwrap();
        app.editor.switch_buffer(0).unwrap(); // start on A
        app
    }

    #[test]
    fn each_buffer_keeps_its_own_viewport() {
        let mut app = two_buffer_app();
        let mut surface = EditorSurface::new();
        let area = Rect::new(0, 0, 40, 10);
        let sync = |surface: &mut EditorSurface, app: &App| {
            surface.resize(area, app); // resize drives sync
        };

        // Deep into A: the viewport scrolls to follow the cursor.
        app.editor.move_to_line(150);
        sync(&mut surface, &app);
        let slot_a = app.editor.current_slot().unwrap();
        let scrolled = surface.viewports[&slot_a].first_visible();
        assert!(scrolled > 100, "A scrolled deep, got {scrolled}");

        // Switch to B: fresh viewport at the top.
        app.editor.switch_buffer(1).unwrap();
        sync(&mut surface, &app);
        let slot_b = app.editor.current_slot().unwrap();
        assert_ne!(slot_a, slot_b);
        assert_eq!(surface.viewports[&slot_b].first_visible(), 0);
        assert_eq!(
            surface.viewports[&slot_a].first_visible(),
            scrolled,
            "A's viewport is stashed, not destroyed"
        );

        // Back to A: the deep scroll is restored.
        app.editor.switch_buffer(0).unwrap();
        sync(&mut surface, &app);
        assert_eq!(surface.viewports[&slot_a].first_visible(), scrolled);

        std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("eggplant-vm-{}", std::process::id())),
        )
        .ok();
    }
}
