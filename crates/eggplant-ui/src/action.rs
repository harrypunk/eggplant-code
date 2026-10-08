//! `AppAction`: every way SHARED state can change, as plain data
//! (docs/design/state-flow.md). Layers and command translators *return*
//! actions; they never perform them. The single interpreter is
//! `Compositor::dispatch` — the one place cross-slice mutation lives.
//!
//! The tier boundary: editor-internal mutations (cursor moves, edits —
//! high-frequency, domain-rich) keep the editing pipeline's own
//! vocabulary (`Resolved` / `EditorAction`) and ride inside
//! `AppAction::Modal` / `AppAction::Edit`. Layer-local state (picker
//! input, tree expansion, scroll offsets) is not here at all — local
//! state mutates locally.

use std::path::PathBuf;

use eggplant_core::editing::{EditorAction, PendingState, Resolved};

use crate::app::{App, Leap};
use crate::commands::Command;
use crate::compositor::{Compositor, FocusDirection, Layer};
use crate::layers::notification::Level;

/// One shared-state change. Plain data: constructible in tests, loggable,
/// replayable.
pub enum AppAction {
    // ---- notifications ----
    Notify {
        level: Level,
        message: String,
    },

    // ---- editing (the core pipeline's outputs, traveling as data) ----
    /// A modal key resolution: store the next pending state, then run the
    /// resolution through `editing::interpret_resolved`.
    Modal {
        pending: PendingState,
        resolved: Resolved,
    },
    /// Pending state changed without an edit (counts, armed/cancelled
    /// operators, view intents handled locally by the editor surface).
    SetPending(PendingState),
    /// A registry-level edit action (palette / global keys).
    Edit(EditorAction),

    // ---- buffers ----
    /// Open a file (optionally jumping to `(line, col)`). Open failures
    /// become error notifications inside dispatch.
    OpenBuffer {
        path: PathBuf,
        at: Option<(usize, usize)>,
    },
    NextBuffer,
    PrevBuffer,
    /// Close the current buffer; unsaved-changes errors notify.
    CloseCurrentBuffer,
    /// Move the cursor to a line (buffer grep, page scrolls).
    MoveToLine(usize),
    JumpTo {
        line: usize,
        col: usize,
    },
    Save,
    /// Save, then quit on success; notify on failure.
    SaveQuit,
    /// Incremental buffer search (the `/` prompt types these).
    Search(String),
    ClearSearch,

    // ---- theme & ui prefs ----
    CycleTheme,
    ToggleWrap,

    // ---- overlays ----
    SetLeap(Option<Leap>),

    // ---- layers & focus ----
    PushLayer(Box<dyn Layer>),
    /// Toggle a layer by id: removed if present, else built by `make`
    /// (construction may do I/O — event time — and may fail → notify).
    ToggleLayer {
        id: &'static str,
        make: fn(&App) -> Result<Box<dyn Layer>, String>,
    },
    /// Close the layer that emitted this action (identity by id, robust
    /// against index shifts from earlier actions in the same batch).
    CloseSelf,
    /// Move focus back to the base editor window.
    Unfocus,
    FocusWindow(FocusDirection),

    // ---- commands ----
    /// Run a registry command (translate it to actions, dispatch those).
    Run(Command),

    // ---- lifecycle ----
    Quit,
}

/// A fact about a dispatched action, broadcast to every layer after
/// dispatch (the subscription channel — redux middleware-style). Kept
/// minimal: payloads live in `App` state; observers read the truth from
/// there, so a failed `OpenBuffer` never misleads anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionEvent {
    /// The current buffer changed (opened, switched, closed).
    BufferChanged,
    /// Anything else — the default; observers ignore it.
    Other,
}

impl ActionEvent {
    /// Derive the broadcast fact from an action (before it's consumed).
    fn of(action: &AppAction) -> Self {
        match action {
            AppAction::OpenBuffer { .. }
            | AppAction::NextBuffer
            | AppAction::PrevBuffer
            | AppAction::CloseCurrentBuffer => Self::BufferChanged,
            _ => Self::Other,
        }
    }
}

/// Outcome of dispatching a key to a layer.
pub enum Handled {
    /// The layer didn't handle the key; pass it on (e.g. to the global
    /// keymap).
    Ignored,
    /// The layer handled the key; dispatch these actions in order.
    Acted(Vec<AppAction>),
}

impl Handled {
    /// Handled, with one action.
    pub fn one(action: AppAction) -> Self {
        Self::Acted(vec![action])
    }

    /// Handled, no shared-state effects (the layer only changed its own
    /// state).
    pub fn quiet() -> Self {
        Self::Acted(Vec::new())
    }
}

impl Compositor {
    /// The single interpreter: every shared-state mutation flows through
    /// here. `emitter` is the id of the layer that produced the action
    /// (`None` for commands from the global keymap) — `CloseSelf` resolves
    /// against it.
    pub fn dispatch(&mut self, action: AppAction, emitter: Option<&str>, app: &mut App) {
        let event = ActionEvent::of(&action);
        self.apply(action, emitter, app);
        // Broadcast the fact: observers (the explorer's buffer sync, …)
        // react to shared-state changes without dispatch knowing them.
        for layer in self.layers_mut() {
            layer.observe(event, app);
        }
    }

    /// Apply one action's effects. Public only for `action.rs`'s impl.
    fn apply(&mut self, action: AppAction, emitter: Option<&str>, app: &mut App) {
        match action {
            AppAction::Notify { level, message } => {
                app.notifications
                    .push(crate::layers::notification::Notification::with_level(
                        level, message,
                    ));
            }

            AppAction::Modal { pending, resolved } => {
                app.input.pending = pending;
                eggplant_core::editing::interpret_resolved(resolved, app);
            }
            AppAction::SetPending(pending) => app.input.pending = pending,
            AppAction::Edit(action) => eggplant_core::editing::interpret(action, 1, app),

            AppAction::OpenBuffer { path, at } => match app.editor.open_buffer(&path) {
                Ok(()) => {
                    if let Some((line, col)) = at {
                        app.editor.jump_to(line, col);
                    }
                }
                Err(err) => {
                    let display = path
                        .strip_prefix(&app.workspace.root)
                        .unwrap_or(&path)
                        .display();
                    app.notifications
                        .push(crate::layers::notification::Notification::error(format!(
                            "open {display}: {err:#}"
                        )));
                }
            },
            AppAction::NextBuffer => app.editor.next_buffer(),
            AppAction::PrevBuffer => app.editor.prev_buffer(),
            AppAction::CloseCurrentBuffer => {
                if let Err(err) = app.editor.close_current_buffer(false) {
                    app.notifications
                        .push(crate::layers::notification::Notification::error(format!(
                            "{err:#}"
                        )));
                }
            }
            AppAction::MoveToLine(line) => app.editor.move_to_line(line),
            AppAction::JumpTo { line, col } => app.editor.jump_to(line, col),
            AppAction::Save => save_and_notify(app),
            AppAction::SaveQuit => match app.editor.save() {
                Ok(()) => app.request_quit(),
                Err(err) => {
                    app.notifications
                        .push(crate::layers::notification::Notification::error(format!(
                            "save failed: {err:#}"
                        )));
                }
            },
            AppAction::Search(pattern) => {
                app.editor.search(&pattern);
            }
            AppAction::ClearSearch => app.editor.clear_search(),

            AppAction::CycleTheme => {
                let name = app.theme.cycle();
                app.notifications
                    .push(crate::layers::notification::Notification::info(format!(
                        "theme: {name}"
                    )));
            }
            AppAction::ToggleWrap => {
                app.wrap = !app.wrap;
                app.notifications
                    .push(crate::layers::notification::Notification::info(
                        if app.wrap {
                            "wrap on"
                        } else {
                            "wrap off (horizontal scroll)"
                        },
                    ));
            }

            AppAction::SetLeap(leap) => app.leap = leap,

            AppAction::PushLayer(layer) => self.push(layer),
            AppAction::ToggleLayer { id, make } => {
                if self.has(id) {
                    self.remove_by_id(id);
                } else {
                    match make(app) {
                        Ok(layer) => self.push(layer),
                        Err(err) => {
                            app.notifications
                                .push(crate::layers::notification::Notification::error(err));
                        }
                    }
                }
            }
            AppAction::CloseSelf => {
                if let Some(id) = emitter {
                    self.remove_by_id(id);
                }
            }
            AppAction::Unfocus => self.unfocus(),
            AppAction::FocusWindow(direction) => self.focus_direction(direction),

            AppAction::Run(command) => self.execute(command, app),

            AppAction::Quit => app.request_quit(),
        }
    }
}

/// Save the current buffer, reporting the outcome as a notification.
fn save_and_notify(app: &mut App) {
    let notification = match app.editor.save() {
        Ok(()) => crate::layers::notification::Notification::info(format!(
            "wrote {}",
            app.editor.display_name().unwrap_or_default()
        )),
        Err(err) => {
            crate::layers::notification::Notification::error(format!("save failed: {err:#}"))
        }
    };
    app.notifications.push(notification);
}
