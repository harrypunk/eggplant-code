//! Shared application state.

use eggplant_core::Editor;

use crate::layers::notification::Notifications;

pub struct App {
    pub editor: Editor,
    pub notifications: Notifications,
    /// Set by layers (e.g. confirm-quit dialog) to request app shutdown.
    pub should_quit: bool,
    /// Demo counter for the `F3` notification-spam key (until real producers exist).
    pub tick_count: u32,
}

impl App {
    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            notifications: Notifications::new(),
            should_quit: false,
            tick_count: 0,
        }
    }
}
