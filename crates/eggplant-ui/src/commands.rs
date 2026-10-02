//! Application commands + the command registry.
//!
//! A `Command` is a named, invocable action (the palette lists them, the
//! global keymap triggers them). The `Registry` owns every command once;
//! key bindings are a declarative table of references into it.

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

    pub const fn char(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub const fn ctrl(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    pub const fn function(n: u8) -> Self {
        Self::new(KeyCode::F(n), KeyModifiers::NONE)
    }

    pub fn matches(&self, key: &KeyEvent) -> bool {
        if self.code != key.code {
            return false;
        }
        if self.modifiers == key.modifiers {
            return true;
        }
        // Shift alone doesn't change which char a plain binding means —
        // crossterm already reports the shifted char (e.g. 'G', '$').
        self.modifiers.is_empty() && key.modifiers == KeyModifiers::SHIFT
    }
}

/// A named application action, invocable by key, palette, or `:` alias.
#[derive(Clone, Copy)]
pub struct Command {
    pub id: &'static str,
    pub description: &'static str,
    pub execute: fn(&mut App, &mut Compositor),
}

/// Every command, plus the global keymap as (stroke → command index).
#[derive(Default)]
pub struct Registry {
    commands: Vec<Command>,
    keymap: Vec<(KeyStroke, usize)>,
}

impl Registry {
    pub fn new(commands: Vec<Command>, keymap: Vec<(KeyStroke, usize)>) -> Self {
        Self { commands, keymap }
    }

    /// The command bound to `key` in the global keymap, if any.
    pub fn lookup_key(&self, key: &KeyEvent) -> Option<Command> {
        self.keymap
            .iter()
            .find(|(stroke, _)| stroke.matches(key))
            .map(|(_, index)| self.commands[*index])
    }

    pub fn by_id(&self, id: &str) -> Option<Command> {
        self.commands.iter().copied().find(|c| c.id == id)
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
}

/// The default registry: all global commands + their key bindings.
pub fn default_registry() -> Registry {
    let commands = vec![
        Command {
            id: "app.quit",
            description: "Quit (confirms on unsaved changes)",
            execute: quit,
        },
        Command {
            id: "app.force-quit",
            description: "Quit immediately without saving",
            execute: force_quit,
        },
        Command {
            id: "file.save",
            description: "Save the current buffer",
            execute: save_with_notification,
        },
        Command {
            id: "panel.files.toggle",
            description: "Toggle file explorer",
            execute: toggle_files_panel,
        },
        Command {
            id: "focus.next",
            description: "Focus next layer",
            execute: focus_next,
        },
        Command {
            id: "buffer.next",
            description: "Switch to next buffer",
            execute: |app, _| app.editor.next_buffer(),
        },
        Command {
            id: "buffer.prev",
            description: "Switch to previous buffer",
            execute: |app, _| app.editor.prev_buffer(),
        },
        Command {
            id: "demo.dialog",
            description: "Toggle demo floating dialog",
            execute: toggle_demo_dialog,
        },
        Command {
            id: "demo.notification",
            description: "Spawn a demo notification",
            execute: demo_notification,
        },
    ];
    let keymap = vec![
        (KeyStroke::ctrl('c'), 1),   // app.force-quit
        (KeyStroke::ctrl('q'), 0),   // app.quit
        (KeyStroke::ctrl('s'), 2),   // file.save
        (KeyStroke::ctrl('e'), 3),   // panel.files.toggle
        (KeyStroke::ctrl('w'), 4),   // focus.next
        (KeyStroke::function(2), 7), // demo.dialog
        (KeyStroke::function(3), 8), // demo.notification
    ];
    Registry::new(commands, keymap)
}

// ---- command implementations (shared with the `:` ex commands) ----

/// Save the current buffer, reporting the outcome as a notification.
pub(crate) fn save_with_notification(app: &mut App, _: &mut Compositor) {
    let notification = match app.editor.save() {
        Ok(()) => Notification::info(format!("wrote {}", app.editor.display_name())),
        Err(err) => Notification::error(format!("save failed: {err:#}")),
    };
    app.notifications.push(notification);
}

/// Quit with a confirm dialog when any buffer has unsaved changes.
pub(crate) fn quit(app: &mut App, compositor: &mut Compositor) {
    if app.editor.any_modified() {
        compositor.push(Box::new(ConfirmDialog::new(
            "unsaved changes",
            "Some buffers have unsaved changes. Quit anyway?",
            |app: &mut App| app.request_quit(),
        )));
    } else {
        app.request_quit();
    }
}

fn force_quit(app: &mut App, _: &mut Compositor) {
    app.request_quit();
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
        let registry = default_registry();
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(registry.lookup_key(&ctrl_q).map(|c| c.id), Some("app.quit"));

        let plain_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(registry.lookup_key(&plain_q).is_none());
    }

    #[test]
    fn keystroke_plain_char_ignores_shift_but_not_ctrl() {
        let stroke = KeyStroke::char('G');
        let shifted = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert!(stroke.matches(&shifted));

        let stroke = KeyStroke::char('h');
        let ctrl_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert!(!stroke.matches(&ctrl_h));
    }
}
