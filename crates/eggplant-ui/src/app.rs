//! Shared application state.

use eggplant_core::Editor;

use crate::commands::{self, Registry};
use crate::editing::{EditorCtx, Keymaps, PendingState};
use crate::files::IgnoreRules;
use crate::layers::notification::{Notification, Notifications};
use crate::theme::Theme;

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

/// A leap-jump target: `label` shown at `(line, col)`; typing it jumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeapLabel {
    pub label: char,
    pub line: usize,
    pub col: usize,
}

/// Leap-jump state (`Space g c`): the 2-char pattern typed so far, then the
/// labeled matches. `Some` = a leap is in progress (editor renders dimmed).
#[derive(Debug, Default)]
pub struct Leap {
    pub pattern: String,
    pub labels: Vec<LeapLabel>,
}

pub struct App {
    pub editor: Editor,
    pub notifications: Notifications,
    /// The command registry: global keymap + palette contents.
    pub registry: Registry,
    /// Active color theme (components read it via props adapters).
    pub theme: Theme,
    /// Pending modal input (counts, armed operators) — the statusline's
    /// showcmd-style hint reads it; `editing::resolve` mutates it.
    pub pending: PendingState,
    /// Modal keymaps (compiled defaults + config overrides).
    pub keymaps: Keymaps,
    /// Workspace root (file picker/explorer scope).
    pub root: std::path::PathBuf,
    /// File ignore rules (defaults + config `[files] ignore`).
    pub file_ignores: IgnoreRules,
    /// Leap-jump in progress (Space g c).
    pub leap: Option<Leap>,
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
            theme: Theme::default(),
            lifecycle: Lifecycle::Running,
            pending: PendingState::default(),
            keymaps: Keymaps::default(),
            root: std::env::current_dir().unwrap_or_default(),
            file_ignores: IgnoreRules::new(&std::env::current_dir().unwrap_or_default(), &[]),
            leap: None,
            tick_count: 0,
        }
    }

    /// The pending-input hint for the statusline (vim `showcmd` style).
    pub fn pending_hint(&self) -> Option<String> {
        self.pending.hint()
    }

    /// Request application shutdown (the event loop observes and exits).
    pub fn request_quit(&mut self) {
        self.lifecycle = Lifecycle::Quitting;
    }

    pub fn is_quitting(&self) -> bool {
        self.lifecycle == Lifecycle::Quitting
    }
}

impl EditorCtx for App {
    fn editor(&mut self) -> &mut Editor {
        &mut self.editor
    }

    fn notify(&mut self, message: &str) {
        self.notifications.push(Notification::info(message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_hint_formats_showcmd_style() {
        let mut app = App::new(Editor::scratch().unwrap());
        assert_eq!(app.pending_hint(), None);

        use crate::editing::PendingKey;

        app.pending.count = Some(5);
        assert_eq!(app.pending_hint().as_deref(), Some("5"));

        app.pending.key = Some((PendingKey::Delete, 1));
        app.pending.count = None;
        assert_eq!(app.pending_hint().as_deref(), Some("d"));

        app.pending.key = Some((PendingKey::Delete, 2));
        assert_eq!(app.pending_hint().as_deref(), Some("d2"));

        // Digits typed after the operator append: `d` then `3`.
        app.pending.key = Some((PendingKey::Yank, 1));
        app.pending.count = Some(3);
        assert_eq!(app.pending_hint().as_deref(), Some("y3"));
    }
}
