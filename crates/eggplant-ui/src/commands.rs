//! Application commands + the command registry.
//!
//! A `Command` is a named, invocable action (the palette lists them, the
//! global keymap triggers them). The `Registry` owns every command once;
//! key bindings are a declarative table of references into it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::compositor::{Compositor, FocusDirection};
use crate::layers::dialog::{ConfirmDialog, Dialog};
use crate::layers::files_panel::{self, FilesPanel};
use crate::layers::notification::Notification;
use crate::layers::palette::Palette;
use crate::layers::which_key::WhichKey;
use crate::theme::Theme;

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

    pub const fn ctrl_shift(c: char) -> Self {
        Self::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL.union(KeyModifiers::SHIFT),
        )
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

/// One node in the which-key tree (`Space` prefix menu).
pub enum KeyNode {
    /// A command leaf: pressing `key` executes the registry command.
    Leaf {
        key: char,
        description: &'static str,
        command: &'static str,
    },
    /// A submenu: pressing `key` descends.
    Group {
        key: char,
        description: &'static str,
        children: &'static [KeyNode],
    },
}

impl KeyNode {
    pub fn key(&self) -> char {
        match self {
            KeyNode::Leaf { key, .. } | KeyNode::Group { key, .. } => *key,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            KeyNode::Leaf { description, .. } | KeyNode::Group { description, .. } => description,
        }
    }
}

/// The which-key root (`Space`). Common commands; the full list lives in the
/// palette (`C-S-p`).
pub static WHICH_KEY_ROOT: &[KeyNode] = &[
    KeyNode::Group {
        key: 'f',
        description: "+file",
        children: &[
            KeyNode::Leaf {
                key: 's',
                description: "save",
                command: "file.save",
            },
            KeyNode::Leaf {
                key: 'q',
                description: "save & quit",
                command: "file.save-quit",
            },
            KeyNode::Leaf {
                key: 'e',
                description: "explorer",
                command: "panel.files.toggle",
            },
        ],
    },
    KeyNode::Group {
        key: 'b',
        description: "+buffer",
        children: &[
            KeyNode::Leaf {
                key: 'n',
                description: "next",
                command: "buffer.next",
            },
            KeyNode::Leaf {
                key: 'p',
                description: "prev",
                command: "buffer.prev",
            },
            KeyNode::Leaf {
                key: 'd',
                description: "close",
                command: "buffer.close",
            },
        ],
    },
    KeyNode::Leaf {
        key: 'q',
        description: "quit",
        command: "app.quit",
    },
    KeyNode::Leaf {
        key: 't',
        description: "cycle theme",
        command: "theme.cycle",
    },
    KeyNode::Leaf {
        key: 'p',
        description: "command palette",
        command: "palette.open",
    },
];

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
            id: "window.focus-left",
            description: "Focus window to the left",
            execute: focus_left,
        },
        Command {
            id: "window.focus-right",
            description: "Focus window to the right",
            execute: focus_right,
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
            id: "buffer.close",
            description: "Close the current buffer (fails on unsaved changes)",
            execute: |app, _| {
                if let Err(err) = app.editor.close_current_buffer(false) {
                    app.notifications
                        .push(Notification::error(format!("{err:#}")));
                }
            },
        },
        Command {
            id: "file.save-quit",
            description: "Save the current buffer, then quit",
            execute: |app, compositor| match app.editor.save() {
                Ok(()) => quit(app, compositor),
                Err(err) => app
                    .notifications
                    .push(Notification::error(format!("save failed: {err:#}"))),
            },
        },
        Command {
            id: "palette.open",
            description: "Open the command palette",
            execute: |app, compositor| {
                compositor.push(Box::new(Palette::new(app.registry.commands().to_vec())));
            },
        },
        Command {
            id: "which-key.open",
            description: "Open the key-hints menu (Space prefix)",
            execute: |_, compositor| compositor.push(Box::new(WhichKey::root())),
        },
        Command {
            id: "theme.cycle",
            description: "Cycle to the next color theme",
            execute: |app, _| {
                app.theme = Theme::next_after(app.theme.name);
                app.notifications
                    .push(Notification::info(format!("theme: {}", app.theme.name)));
            },
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
    // Command indices (order of the vec above):
    //   0 app.quit  1 app.force-quit  2 file.save  3 panel.files.toggle
    //   4 window.focus-left  5 window.focus-right  6 buffer.next  7 buffer.prev
    //   8 buffer.close  9 file.save-quit  10 palette.open  11 which-key.open
    //   12 theme.cycle  13 demo.dialog  14 demo.notification
    let keymap = vec![
        (KeyStroke::ctrl('c'), 1),        // app.force-quit
        (KeyStroke::ctrl('q'), 0),        // app.quit
        (KeyStroke::ctrl('s'), 2),        // file.save
        (KeyStroke::ctrl('e'), 3),        // panel.files.toggle
        (KeyStroke::ctrl('h'), 4),        // window.focus-left
        (KeyStroke::ctrl('l'), 5),        // window.focus-right
        (KeyStroke::ctrl_shift('p'), 10), // palette.open
        (KeyStroke::ctrl_shift('P'), 10), // (terminal casing varies)
        (KeyStroke::ctrl('p'), 10),       // palette.open (fallback: no kitty protocol)
        (KeyStroke::char(' '), 11),       // which-key.open (prefix menu)
        (KeyStroke::function(2), 13),     // demo.dialog
        (KeyStroke::function(3), 14),     // demo.notification
    ];
    Registry::new(commands, keymap)
}

// ---- command implementations (shared with the `:` ex commands) ----

/// Save the current buffer, reporting the outcome as a notification.
pub(crate) fn save_with_notification(app: &mut App, _: &mut Compositor) {
    let notification = match app.editor.save() {
        Ok(()) => Notification::info(format!(
            "wrote {}",
            app.editor.display_name().unwrap_or_default()
        )),
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

fn focus_left(_: &mut App, compositor: &mut Compositor) {
    compositor.focus_direction(FocusDirection::Left);
}

fn focus_right(_: &mut App, compositor: &mut Compositor) {
    compositor.focus_direction(FocusDirection::Right);
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
