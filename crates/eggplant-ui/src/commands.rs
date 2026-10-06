//! Application commands + the command registry.
//!
//! A `Command` is a named, invocable action (the palette lists them, the
//! global keymap triggers them). The `Registry` owns every command once;
//! key bindings are a declarative table of references into it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::app::Leap;
use crate::compositor::{Compositor, FocusDirection};
use crate::editing::EditorAction;
use crate::layers::dialog::{ConfirmDialog, Dialog};
use crate::layers::file_picker;
use crate::layers::files_panel::{self, FilesPanel};
use crate::layers::leap::LeapLayer;
use crate::layers::notification::Notification;
use crate::layers::search_prompt::SearchPrompt;
use crate::layers::which_key::WhichKey;
use crate::layers::{grep, palette};
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

    /// Parse a config-file stroke: `"C-S-p"`, `"Space"`, `"g"`, `"F2"`,
    /// `"left"`. Modifiers are `-`-prefixed (`C-`/`A-`/`S-`), key names are
    /// case-insensitive, a single char keeps its case (`G` ≠ `g`).
    pub fn parse(text: &str) -> Option<Self> {
        let (mods, key) = match text.rsplit_once('-') {
            Some((mods, key)) if !key.is_empty() => (mods, key),
            _ => ("", text),
        };
        let mut modifiers = KeyModifiers::NONE;
        for m in mods.split('-').filter(|m| !m.is_empty()) {
            modifiers |= match m.to_ascii_lowercase().as_str() {
                "c" | "ctrl" => KeyModifiers::CONTROL,
                "a" | "alt" => KeyModifiers::ALT,
                "s" | "shift" => KeyModifiers::SHIFT,
                _ => return None,
            };
        }
        let code = match key.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "esc" => KeyCode::Esc,
            "enter" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            f if f.len() >= 2
                && f.len() <= 3
                && f.starts_with('f')
                && f[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                KeyCode::F(f[1..].parse().ok()?)
            }
            _ if key.chars().count() == 1 => {
                KeyCode::Char(key.chars().next().expect("len checked"))
            }
            _ => return None,
        };
        Some(Self::new(code, modifiers))
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

/// How a command executes. Both kinds funnel through one dispatch
/// (`Compositor::execute`): the palette, the which-key tree, global keys,
/// and modal editing never call implementations directly.
#[derive(Clone, Copy)]
pub enum CommandKind {
    /// App-level: layers, windows, quit, save, … (runs with the compositor).
    App(fn(&mut App, &mut Compositor)),
    /// Modal edit action, interpreted via `editing::interpret` (the same
    /// funnel modal keys use).
    Edit(EditorAction),
}

/// A named application action, invocable by key, palette, or which-key leaf.
#[derive(Clone, Copy)]
pub struct Command {
    pub id: &'static str,
    pub description: &'static str,
    pub kind: CommandKind,
    /// Listed in the command palette? Low-level `edit.*` commands are
    /// registered (keybindable, single dispatch) but not listed — modal keys
    /// are their home.
    pub palette: bool,
}

impl Command {
    const fn app(
        id: &'static str,
        description: &'static str,
        f: fn(&mut App, &mut Compositor),
    ) -> Self {
        Self {
            id,
            description,
            kind: CommandKind::App(f),
            palette: true,
        }
    }

    /// Auto-registered edit action: id/description come from the action.
    fn edit(action: EditorAction) -> Self {
        Self {
            id: action.id(),
            description: action.description(),
            kind: CommandKind::Edit(action),
            palette: false,
        }
    }
}

/// Every command, plus the global keymap as (stroke → command id).
#[derive(Default)]
pub struct Registry {
    commands: Vec<Command>,
    keymap: Vec<(KeyStroke, String)>,
}

impl Registry {
    pub fn new(commands: Vec<Command>, keymap: Vec<(KeyStroke, &'static str)>) -> Self {
        Self {
            commands,
            keymap: keymap
                .into_iter()
                .map(|(stroke, id)| (stroke, id.to_owned()))
                .collect(),
        }
    }

    /// Bind (or shadow) a stroke → command id. Config overrides prepend, so
    /// they win over defaults.
    pub fn bind(&mut self, stroke: KeyStroke, id: &str) {
        self.keymap.insert(0, (stroke, id.to_owned()));
    }

    /// The command bound to `key` in the global keymap, if any.
    pub fn lookup_key(&self, key: &KeyEvent) -> Option<Command> {
        self.keymap
            .iter()
            .find(|(stroke, _)| stroke.matches(key))
            .and_then(|(_, id)| self.by_id(id))
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
///
/// Design rules (the full tree incl. planned groups is in README):
/// groups are nouns, leaves are verbs, every leaf names a registry command
/// id. Group letters are reserved up front (`s` search, `g` goto, `w`
/// window, `l` lsp, `a` ai) so future features never reshuffle bindings.
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
            KeyNode::Leaf {
                key: 'p',
                description: "picker",
                command: "file.open-picker",
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
    KeyNode::Group {
        key: 'g',
        description: "+goto",
        children: &[KeyNode::Leaf {
            key: 'c',
            description: "char (2-char leap)",
            command: "goto.char",
        }],
    },
    KeyNode::Group {
        key: 's',
        description: "+search",
        children: &[
            KeyNode::Leaf {
                key: 'b',
                description: "in buffer",
                command: "search.buffer",
            },
            KeyNode::Leaf {
                key: 'c',
                description: "grep lines",
                command: "search.lines",
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
        Command::app("app.quit", "Quit (confirms on unsaved changes)", quit),
        Command::app(
            "app.force-quit",
            "Quit immediately without saving",
            force_quit,
        ),
        Command::app(
            "file.save",
            "Save the current buffer",
            save_with_notification,
        ),
        Command::app(
            "file.open-picker",
            "Open a file (picker)",
            |app, compositor| {
                compositor.push(Box::new(file_picker::file_picker(app)));
            },
        ),
        Command::app(
            "panel.files.toggle",
            "Toggle file explorer",
            toggle_files_panel,
        ),
        Command::app("window.focus-left", "Focus window to the left", focus_left),
        Command::app(
            "window.focus-right",
            "Focus window to the right",
            focus_right,
        ),
        Command::app("buffer.next", "Switch to next buffer", |app, _| {
            app.editor.next_buffer()
        }),
        Command::app("buffer.prev", "Switch to previous buffer", |app, _| {
            app.editor.prev_buffer()
        }),
        Command::app(
            "buffer.close",
            "Close the current buffer (fails on unsaved changes)",
            |app, _| {
                if let Err(err) = app.editor.close_current_buffer(false) {
                    app.notifications
                        .push(Notification::error(format!("{err:#}")));
                }
            },
        ),
        Command::app(
            "file.save-quit",
            "Save the current buffer, then quit",
            |app, compositor| match app.editor.save() {
                Ok(()) => quit(app, compositor),
                Err(err) => app
                    .notifications
                    .push(Notification::error(format!("save failed: {err:#}"))),
            },
        ),
        Command::app(
            "palette.open",
            "Open the command palette",
            |app, compositor| {
                compositor.push(Box::new(palette::command_palette(
                    app.registry.commands().to_vec(),
                )));
            },
        ),
        Command::app(
            "search.buffer",
            "Search in buffer (live, n/N cycle)",
            |_app, compositor| compositor.push(Box::new(SearchPrompt::new())),
        ),
        Command::app(
            "goto.char",
            "Leap to a 2-char pattern",
            |app, compositor| {
                if app.editor.has_buffer() {
                    app.leap = Some(Leap::default());
                    compositor.push(Box::new(LeapLayer));
                }
            },
        ),
        Command::app(
            "search.lines",
            "Grep lines in buffer (live)",
            |app, compositor| {
                compositor.push(Box::new(grep::buffer_grep(app.editor.buffer_lines())));
            },
        ),
        Command::app(
            "which-key.open",
            "Open the key-hints menu (Space prefix)",
            |_, compositor| compositor.push(Box::new(WhichKey::root())),
        ),
        Command::app("theme.cycle", "Cycle to the next color theme", |app, _| {
            app.theme = Theme::next_after(app.theme.name);
            app.notifications
                .push(Notification::info(format!("theme: {}", app.theme.name)));
        }),
        Command::app(
            "demo.dialog",
            "Toggle demo floating dialog",
            toggle_demo_dialog,
        ),
        Command::app(
            "demo.notification",
            "Spawn a demo notification",
            demo_notification,
        ),
    ];
    // Every modal edit action registers as an `edit.*` command: one
    // dispatch path for palette / which-key / global keys / modal editing.
    let commands: Vec<Command> = commands
        .into_iter()
        .chain(EditorAction::ALL.iter().map(|a| Command::edit(*a)))
        .collect();
    let keymap = vec![
        (KeyStroke::ctrl('c'), "app.force-quit"),
        (KeyStroke::ctrl('q'), "app.quit"),
        (KeyStroke::ctrl('s'), "file.save"),
        (KeyStroke::ctrl('e'), "panel.files.toggle"),
        (KeyStroke::ctrl('h'), "window.focus-left"),
        (KeyStroke::ctrl('l'), "window.focus-right"),
        (KeyStroke::ctrl_shift('p'), "palette.open"),
        (KeyStroke::ctrl_shift('P'), "palette.open"), // terminal casing varies
        (KeyStroke::ctrl('p'), "palette.open"),       // fallback: no kitty protocol
        (KeyStroke::char(' '), "which-key.open"),     // prefix menu
        (KeyStroke::function(2), "demo.dialog"),
        (KeyStroke::function(3), "demo.notification"),
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
    app.notifications.push(Notification::info(format!(
        "notification (tick #{})",
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

    #[test]
    fn every_edit_action_is_registered_but_palette_hidden() {
        let registry = default_registry();
        for action in EditorAction::ALL {
            let command = registry
                .by_id(action.id())
                .unwrap_or_else(|| panic!("{} not registered", action.id()));
            assert_eq!(command.description, action.description());
            assert!(matches!(command.kind, CommandKind::Edit(a) if a == *action));
            assert!(!command.palette, "{} stays out of the palette", action.id());
        }
    }

    #[test]
    fn every_which_key_leaf_resolves_to_a_command() {
        fn walk(nodes: &[KeyNode], registry: &Registry) {
            for node in nodes {
                match node {
                    KeyNode::Leaf { command, .. } => assert!(
                        registry.by_id(command).is_some(),
                        "which-key leaf '{command}' has no command"
                    ),
                    KeyNode::Group { children, .. } => walk(children, registry),
                }
            }
        }
        walk(WHICH_KEY_ROOT, &default_registry());
    }
}
