//! Application commands + the global keymap.
//!
//! A `Command` is a named, invocable action (the M3 command palette will list
//! these). A `Keymap` is a declarative table from key strokes to commands, so
//! key handling is a lookup — not an if-else chain.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::compositor::Compositor;
use crate::layers::dialog::{ConfirmDialog, Dialog};
use crate::layers::files_panel::{self, FilesPanel};
use crate::layers::notification::Notification;

/// A key + modifier combination that can trigger a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyStroke {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn ctrl(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    pub const fn function(n: u8) -> Self {
        Self::new(KeyCode::F(n), KeyModifiers::NONE)
    }

    fn matches(&self, key: &KeyEvent) -> bool {
        self.code == key.code && key.modifiers.contains(self.modifiers)
    }
}

/// A named application action, invocable by key (and later by palette).
pub struct Command {
    pub id: &'static str,
    pub description: &'static str,
    pub execute: fn(&mut App, &mut Compositor),
}

/// Declarative key bindings: first match wins.
#[derive(Default)]
pub struct Keymap {
    bindings: Vec<(KeyStroke, Command)>,
}

impl Keymap {
    pub fn new(bindings: Vec<(KeyStroke, Command)>) -> Self {
        Self { bindings }
    }

    pub fn lookup(&self, key: &KeyEvent) -> Option<&Command> {
        self.bindings
            .iter()
            .find(|(stroke, _)| stroke.matches(key))
            .map(|(_, command)| command)
    }
}

/// The global keymap — consulted only when the focused layer ignored the key.
pub fn global_keymap() -> Keymap {
    Keymap::new(vec![
        (
            KeyStroke::ctrl('c'),
            Command {
                id: "app.force-quit",
                description: "Quit immediately without saving",
                execute: force_quit,
            },
        ),
        (
            KeyStroke::ctrl('q'),
            Command {
                id: "app.quit",
                description: "Quit (confirms on unsaved changes)",
                execute: quit,
            },
        ),
        (
            KeyStroke::ctrl('e'),
            Command {
                id: "panel.files.toggle",
                description: "Toggle file explorer",
                execute: toggle_files_panel,
            },
        ),
        (
            KeyStroke::ctrl('w'),
            Command {
                id: "focus.next",
                description: "Focus next layer",
                execute: focus_next,
            },
        ),
        (
            KeyStroke::function(2),
            Command {
                id: "demo.dialog",
                description: "Toggle demo floating dialog",
                execute: toggle_demo_dialog,
            },
        ),
        (
            KeyStroke::function(3),
            Command {
                id: "demo.notification",
                description: "Spawn a demo notification",
                execute: demo_notification,
            },
        ),
    ])
}

// ---- command implementations ----

fn force_quit(app: &mut App, _: &mut Compositor) {
    app.should_quit = true;
}

fn quit(app: &mut App, compositor: &mut Compositor) {
    if app.editor.is_modified() {
        compositor.push(Box::new(ConfirmDialog::new(
            "unsaved changes",
            "Quit without saving?",
            |app: &mut App| app.should_quit = true,
        )));
    } else {
        app.should_quit = true;
    }
}

fn toggle_files_panel(app: &mut App, compositor: &mut Compositor) {
    if compositor.has(files_panel::PANEL_ID) {
        compositor.remove_by_id(files_panel::PANEL_ID);
        return;
    }
    match FilesPanel::new(std::env::current_dir().unwrap_or_default()) {
        Ok(panel) => compositor.push(Box::new(panel)),
        Err(err) => app
            .notifications
            .push(Notification::error(format!("files panel: {err}"))),
    }
}

fn focus_next(_: &mut App, compositor: &mut Compositor) {
    compositor.focus_next();
}

fn toggle_demo_dialog(_: &mut App, compositor: &mut Compositor) {
    if compositor.has("dialog") {
        compositor.remove_by_id("dialog");
    } else {
        compositor.push(Box::new(Dialog::new(
            "dialog",
            "floating layers work.\n\n`F2` or `Esc` closes me.",
        )));
    }
}

fn demo_notification(app: &mut App, _: &mut Compositor) {
    app.tick_count += 1;
    app.notifications.push(Notification::info(format!(
        "notification #{}",
        app.tick_count
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_matches_code_and_modifiers() {
        let keymap = global_keymap();
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(keymap.lookup(&ctrl_q).map(|c| c.id), Some("app.quit"));

        let plain_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(keymap.lookup(&plain_q).is_none());
    }

    #[test]
    fn first_match_wins() {
        let keymap = Keymap::new(vec![
            (
                KeyStroke::ctrl('x'),
                Command {
                    id: "first",
                    description: "",
                    execute: |_, _| {},
                },
            ),
            (
                KeyStroke::ctrl('x'),
                Command {
                    id: "second",
                    description: "",
                    execute: |_, _| {},
                },
            ),
        ]);
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(keymap.lookup(&key).map(|c| c.id), Some("first"));
    }
}
