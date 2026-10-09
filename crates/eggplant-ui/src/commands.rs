//! Application commands + the command registry.
//!
//! A `Command` is a named, invocable action (the palette lists them, the
//! global keymap triggers them). The `Registry` owns every command once;
//! key bindings are a declarative table of references into it.

use eggplant_core::input::KeyEvent;
#[cfg(test)]
use eggplant_core::input::{KeyCode, KeyModifiers};

use crate::action::{ActionEvent, AppAction};
use crate::app::App;
use crate::app::Leap;
use crate::compositor::{FocusDirection, Layer};
use crate::layers::dialog::{ConfirmDialog, Dialog};
use crate::layers::file_picker;
use crate::layers::files_panel::{self, FilesPanel};
use crate::layers::leap::LeapLayer;
use crate::layers::notification::Level;
use crate::layers::search_prompt::SearchPrompt;
use crate::layers::which_key::WhichKey;
use crate::layers::{grep, palette};
use eggplant_core::editing::EditorAction;
pub use eggplant_core::input::KeyStroke;

/// How a command executes. Both kinds are PURE translators to actions —
/// the single interpreter (`Compositor::dispatch`) performs the effects.
/// The palette, the which-key tree, global keys, and modal editing never
/// touch implementations directly.
#[derive(Clone, Copy)]
pub enum CommandKind {
    /// App-level: layers, windows, quit, save, … — translates (reading
    /// `&App` at event time) to the actions that will perform it.
    App(fn(&App) -> Vec<AppAction>),
    /// Modal edit action, dispatched via `editing::interpret` (the same
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
        f: fn(&App) -> Vec<AppAction>,
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
    /// Any digit 0-9 resolves to a parameterized action (buffer
    /// quick-choose) — one entry in the data instead of ten enumerated
    /// commands (registry commands are nullary; digits are parsed).
    DigitLeaves {
        description: &'static str,
        act: fn(usize) -> AppAction,
    },
}

impl KeyNode {
    /// The fixed key this node binds; digit leaves bind a RANGE (0-9),
    /// not one key — `None`.
    pub fn key(&self) -> Option<char> {
        match self {
            KeyNode::Leaf { key, .. } | KeyNode::Group { key, .. } => Some(*key),
            KeyNode::DigitLeaves { .. } => None,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            KeyNode::Leaf { description, .. }
            | KeyNode::Group { description, .. }
            | KeyNode::DigitLeaves { description, .. } => description,
        }
    }

    /// The digit-leaf action, when this node is one.
    pub fn digit_action(&self) -> Option<fn(usize) -> AppAction> {
        match self {
            KeyNode::DigitLeaves { act, .. } => Some(*act),
            _ => None,
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
                description: "save+quit",
                command: "file.save-quit",
            },
            KeyNode::Leaf {
                key: 'e',
                description: "explorer",
                command: "panel.files.toggle",
            },
            KeyNode::Leaf {
                key: 'p',
                description: "open",
                command: "file.open-picker",
            },
        ],
    },
    KeyNode::Group {
        key: 'a',
        description: "+agent",
        children: &[
            KeyNode::Leaf {
                key: 'i',
                description: "chat popup",
                command: "agent.chat",
            },
            KeyNode::Leaf {
                key: 'a',
                description: "auth providers",
                command: "agent.auth",
            },
            KeyNode::Leaf {
                key: 'm',
                description: "choose model",
                command: "agent.model",
            },
            KeyNode::Leaf {
                key: 't',
                description: "toggle chat window",
                command: "agent.toggle",
            },
            KeyNode::Leaf {
                key: 'n',
                description: "new session",
                command: "agent.new",
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
            // Quick-choose by index; out-of-range is ignored.
            KeyNode::DigitLeaves {
                description: "buffer n",
                act: |n| AppAction::SwitchBuffer(n),
            },
        ],
    },
    KeyNode::Group {
        key: 'g',
        description: "+goto",
        children: &[KeyNode::Leaf {
            key: 'c',
            description: "leap",
            command: "goto.char",
        }],
    },
    KeyNode::Group {
        key: 's',
        description: "+search",
        children: &[
            KeyNode::Leaf {
                key: 'b',
                description: "buffer",
                command: "search.buffer",
            },
            KeyNode::Leaf {
                key: 'c',
                description: "lines",
                command: "search.lines",
            },
            KeyNode::Leaf {
                key: 'p',
                description: "project",
                command: "search.project",
            },
        ],
    },
    KeyNode::Group {
        key: 'u',
        description: "+ui",
        children: &[KeyNode::Leaf {
            key: 'w',
            description: "wrap",
            command: "ui.toggle-wrap",
        }],
    },
    KeyNode::Leaf {
        key: 'q',
        description: "quit",
        command: "app.quit",
    },
    KeyNode::Leaf {
        key: 't',
        description: "theme",
        command: "theme.cycle",
    },
    KeyNode::Leaf {
        key: 'p',
        description: "palette",
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
        Command::app("file.open-picker", "Open a file (picker)", |app| {
            vec![AppAction::PushLayer(Box::new(file_picker::file_picker(
                app,
            )))]
        }),
        Command::app(
            "panel.files.toggle",
            "Toggle file explorer",
            toggle_files_panel,
        ),
        Command::app("window.focus-left", "Focus window to the left", focus_left),
        Command::app("ui.toggle-wrap", "Toggle soft-wrap", |_| {
            vec![AppAction::ToggleWrap]
        }),
        Command::app(
            "window.focus-right",
            "Focus window to the right",
            focus_right,
        ),
        Command::app("buffer.next", "Switch to next buffer", |_| {
            vec![AppAction::NextBuffer]
        }),
        Command::app("buffer.prev", "Switch to previous buffer", |_| {
            vec![AppAction::PrevBuffer]
        }),
        Command::app(
            "buffer.close",
            "Close the current buffer (fails on unsaved changes)",
            |_| vec![AppAction::CloseCurrentBuffer],
        ),
        Command::app(
            "file.save-quit",
            "Save the current buffer, then quit",
            |_| vec![AppAction::SaveQuit],
        ),
        Command::app("palette.open", "Open the command palette", |app| {
            vec![AppAction::PushLayer(Box::new(palette::command_palette(
                app.input.registry.commands().to_vec(),
            )))]
        }),
        Command::app(
            "search.buffer",
            "Search in buffer (live, n/N cycle)",
            |_| vec![AppAction::PushLayer(Box::new(SearchPrompt::new()))],
        ),
        Command::app("goto.char", "Leap to a 2-char pattern", |app| {
            if app.editor.has_buffer() {
                vec![
                    AppAction::SetLeap(Some(Leap::default())),
                    AppAction::PushLayer(Box::new(LeapLayer)),
                ]
            } else {
                Vec::new()
            }
        }),
        Command::app("search.lines", "Grep lines in buffer (live)", |app| {
            vec![AppAction::PushLayer(Box::new(grep::buffer_grep(
                app.editor.buffer_lines(),
            )))]
        }),
        Command::app("search.project", "Live grep across the workspace", |app| {
            vec![AppAction::PushLayer(Box::new(
                crate::layers::project_grep::project_grep(app),
            ))]
        }),
        Command::app(
            "which-key.open",
            "Open the key-hints menu (Space prefix)",
            |_| vec![AppAction::PushLayer(Box::new(WhichKey::root()))],
        ),
        Command::app("theme.cycle", "Cycle to the next color theme", |_| {
            vec![AppAction::CycleTheme]
        }),
        Command::app("agent.chat", "Chat with the agent (popup)", |_app| {
            vec![
                AppAction::EnsureAgentSession,
                AppAction::PushLayer(crate::layers::chat::ChatModal::new()),
            ]
        }),
        Command::app("agent.abort", "Abort the running agent", |_app| {
            vec![AppAction::AgentAbort]
        }),
        Command::app("agent.new", "Start a new agent session", |_app| {
            vec![AppAction::AgentNewChat]
        }),
        Command::app("agent.auth", "Authenticate providers", |_app| {
            vec![AppAction::PushLayer(crate::layers::auth::AuthLayer::new())]
        }),
        Command::app("agent.model", "Choose a model", |_app| {
            vec![
                AppAction::PushLayer(crate::layers::models::ModelPicker::new()),
                AppAction::FetchModels,
            ]
        }),
        Command::app("agent.toggle", "Toggle the agent chat window", |_app| {
            vec![
                AppAction::EnsureAgentSession,
                AppAction::ToggleLayer {
                    id: crate::layers::chat::PANEL_ID,
                    make: |_app| {
                        Ok(crate::layers::chat::ChatPanel::new()
                            as Box<dyn crate::compositor::Layer>)
                    },
                },
            ]
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
        (KeyStroke::ctrl('i'), "agent.chat"),
        // Dedicated interrupt key (Esc stays close/unfocus; C-c stays
        // force-quit globally). Works with no chat view open.
        (
            KeyStroke::new(
                eggplant_core::input::KeyCode::F(8),
                eggplant_core::input::KeyModifiers::CONTROL,
            ),
            "agent.abort",
        ),
        (KeyStroke::function(2), "demo.dialog"),
        (KeyStroke::function(3), "demo.notification"),
    ];
    Registry::new(commands, keymap)
}

// ---- command translators (pure: `&App` reads → actions as data) ----
// Shared with the `:` ex commands. None of these mutate anything.

/// Save the current buffer; dispatch reports the outcome.
pub(crate) fn save_with_notification(_: &App) -> Vec<AppAction> {
    vec![AppAction::Save]
}

/// Quit, confirming first when any buffer has unsaved changes.
pub(crate) fn quit(app: &App) -> Vec<AppAction> {
    if app.editor.any_modified() {
        vec![AppAction::PushLayer(Box::new(ConfirmDialog::new(
            "unsaved changes",
            "Some buffers have unsaved changes. Quit anyway?",
            vec![AppAction::Quit],
        )))]
    } else {
        vec![AppAction::Quit]
    }
}

fn force_quit(_: &App) -> Vec<AppAction> {
    vec![AppAction::Quit]
}

fn toggle_files_panel(_: &App) -> Vec<AppAction> {
    vec![AppAction::ToggleLayer {
        id: files_panel::PANEL_ID,
        make: |app| {
            FilesPanel::new(app.workspace.root.clone())
                .map(|mut panel| {
                    // Open already-synced: reveal the current buffer's file.
                    panel.observe(ActionEvent::BufferChanged, app);
                    Box::new(panel) as Box<dyn crate::compositor::Layer>
                })
                .map_err(|err| format!("files panel: {err}"))
        },
    }]
}

fn focus_left(_: &App) -> Vec<AppAction> {
    vec![AppAction::FocusWindow(FocusDirection::Left)]
}

fn focus_right(_: &App) -> Vec<AppAction> {
    vec![AppAction::FocusWindow(FocusDirection::Right)]
}

fn toggle_demo_dialog(_: &App) -> Vec<AppAction> {
    vec![AppAction::ToggleLayer {
        id: "dialog",
        make: |_| {
            Ok(Box::new(Dialog::new(
                "dialog",
                "floating layers work.\n\n`F2` or `Esc` closes me.",
            )) as Box<dyn crate::compositor::Layer>)
        },
    }]
}

fn demo_notification(app: &App) -> Vec<AppAction> {
    vec![AppAction::Notify {
        level: Level::Info,
        message: format!("notification (tick #{})", app.theme.ticks()),
    }]
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
                    // Digit leaves carry their action inline — no command id.
                    KeyNode::DigitLeaves { .. } => {}
                }
            }
        }
        walk(WHICH_KEY_ROOT, &default_registry());
    }
}
