//! Statusline chrome adapter: projects `App` state into statusline props and
//! delegates to the pure `components::statusline` view (Rule 5).

use ratatui::layout::Rect;

use crate::app::App;
use crate::components::statusline::{self, StatuslineProps};
use crate::element::Element;

/// Describe the statusline. `focused_layer` is the focused layer's id, shown
/// as a tag when it isn't the base editor.
pub fn view(app: &App, focused_layer: Option<&'static str>, area: Rect) -> Element {
    statusline::view(
        &StatuslineProps {
            mode: app.editor.mode(),
            buffer_name: app.editor.display_name(),
            modified: app.editor.is_modified(),
            buffers: (app.editor.current_buffer(), app.editor.buffer_count()),
            cursor: app.editor.cursor(),
            line_count: app.editor.line_count(),
            focused_layer,
        },
        area,
    )
}
