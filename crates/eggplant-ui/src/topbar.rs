//! Topbar chrome adapter: projects open buffers into tab props and
//! delegates to the pure `components::topbar` view (Rule 5).

use ratatui::layout::Rect;

use crate::app::App;
use crate::components::topbar::{self, BufferTab};
use crate::element::Element;

pub fn view(app: &App, area: Rect) -> Element {
    let buffers = app.editor.buffers_info();
    // Hide pristine scratch buffers (they're noise next to real files) —
    // unless the scratch is the only buffer, so the bar is never
    // mysteriously empty.
    let tabs: Vec<BufferTab> = buffers
        .into_iter()
        .filter(|b| !b.scratch || b.modified || app.editor.buffer_count() == 1)
        .map(|b| BufferTab {
            name: b.name,
            modified: b.modified,
            current: b.current,
        })
        .collect();
    topbar::view(&tabs, area, &app.theme.sheet())
}
