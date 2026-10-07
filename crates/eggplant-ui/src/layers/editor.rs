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
    /// Last area from the compositor's `resize` hook — text width feeds
    /// viewport sync (horizontal scroll, wrap height).
    area: Rect,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    fn sync_viewport(&mut self, app: &App) {
        self.viewport.sync(
            app.editor.generation(),
            app.editor.cursor(),
            app.wrap,
            self.text_width(app),
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
        let width = self.text_width(app);
        let line_count = app.editor.display_line_count();
        let rows = self
            .viewport
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
