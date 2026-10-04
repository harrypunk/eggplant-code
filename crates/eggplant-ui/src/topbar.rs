//! Topbar chrome adapter: projects open buffers into tab props and
//! delegates to the pure `components::topbar` view (Rule 5).

use ratatui::layout::Rect;

use crate::app::App;
use crate::components::topbar::{self, BufferTab};
use crate::element::Element;

pub fn view(app: &App, area: Rect) -> Element {
    let tabs: Vec<BufferTab> = app
        .editor
        .buffers_info()
        .into_iter()
        .map(|b| BufferTab {
            name: b.name,
            modified: b.modified,
            current: b.current,
        })
        .collect();
    topbar::view(&tabs, area, &app.theme)
}
