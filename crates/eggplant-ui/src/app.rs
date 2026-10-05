//! Shared application state.

use eggplant_core::Editor;

use crate::commands::{self, Registry};
use crate::layers::notification::Notifications;
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

/// Normal-mode keys that wait for a second key: the `d`/`y` operators and
/// the `g` prefix (`gg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKey {
    Delete,
    Yank,
    Goto,
}

impl PendingKey {
    /// The key that arms it (for the pending hint).
    pub fn key(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Yank => 'y',
            Self::Goto => 'g',
        }
    }
}

pub struct App {
    pub editor: Editor,
    pub notifications: Notifications,
    /// The command registry: global keymap + palette contents.
    pub registry: Registry,
    /// Active color theme (components read it via props adapters).
    pub theme: Theme,
    /// Pending normal-mode input (single source of truth for the statusline's
    /// showcmd-style hint): count prefix and armed operator + its count.
    pub pending_count: Option<usize>,
    pub pending_key: Option<(PendingKey, usize)>,
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
            pending_count: None,
            pending_key: None,
            leap: None,
            tick_count: 0,
        }
    }

    /// The pending-input hint for the statusline (vim `showcmd` style):
    /// `"5"` for a bare count, `"d"` / `"d2"` for an armed operator.
    pub fn pending_hint(&self) -> Option<String> {
        let mut hint = String::new();
        if let Some((operator, count)) = self.pending_key {
            hint.push(operator.key());
            if count > 1 {
                hint.push_str(&count.to_string());
            }
        }
        if let Some(count) = self.pending_count {
            hint.push_str(&count.to_string());
        }
        (!hint.is_empty()).then_some(hint)
    }

    /// Request application shutdown (the event loop observes and exits).
    pub fn request_quit(&mut self) {
        self.lifecycle = Lifecycle::Quitting;
    }

    pub fn is_quitting(&self) -> bool {
        self.lifecycle == Lifecycle::Quitting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_hint_formats_showcmd_style() {
        let mut app = App::new(Editor::scratch().unwrap());
        assert_eq!(app.pending_hint(), None);

        app.pending_count = Some(5);
        assert_eq!(app.pending_hint().as_deref(), Some("5"));

        app.pending_key = Some((PendingKey::Delete, 1));
        app.pending_count = None;
        assert_eq!(app.pending_hint().as_deref(), Some("d"));

        app.pending_key = Some((PendingKey::Delete, 2));
        assert_eq!(app.pending_hint().as_deref(), Some("d2"));

        // Digits typed after the operator append: `d` then `3`.
        app.pending_key = Some((PendingKey::Yank, 1));
        app.pending_count = Some(3);
        assert_eq!(app.pending_hint().as_deref(), Some("y3"));
    }
}
