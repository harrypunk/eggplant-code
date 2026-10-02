//! Shared application state.

use eggplant_core::Editor;

use crate::commands::{self, Registry};
use crate::layers::notification::Notifications;

/// Application lifecycle status. Not a `bool`: quitting is a state
/// transition, and this is where future states land (e.g. quit reasons,
/// restart-into-file) without becoming a pile of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifecycle {
    /// Normal operation.
    #[default]
    Running,
    /// Teardown requested; the event loop exits after the current event.
    Quitting,
}

pub struct App {
    pub editor: Editor,
    pub notifications: Notifications,
    /// The command registry: global keymap + palette contents.
    pub registry: Registry,
    lifecycle: Lifecycle,
    /// Demo counter for the `F3` notification-spam key (until real producers exist).
    pub tick_count: u32,
}

impl App {
    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            notifications: Notifications::new(),
            registry: commands::default_registry(),
            lifecycle: Lifecycle::Running,
            tick_count: 0,
        }
    }

    /// Request application shutdown (the event loop observes and exits).
    pub fn request_quit(&mut self) {
        self.lifecycle = Lifecycle::Quitting;
    }

    pub fn is_quitting(&self) -> bool {
        self.lifecycle == Lifecycle::Quitting
    }
}
