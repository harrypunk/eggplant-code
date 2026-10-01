//! Shared application state.

use eggplant_core::Editor;

use crate::layers::notification::Notifications;

pub struct App {
    pub editor: Editor,
    pub notifications: Notifications,
    /// Demo counter for the `n` notification-spam key (until real producers exist).
    pub tick_count: u32,
}

impl App {
    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            notifications: Notifications::new(),
            tick_count: 0,
        }
    }
}
