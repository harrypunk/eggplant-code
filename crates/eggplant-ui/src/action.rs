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
/// Events from background threads (validation, …). Data-only, `Send`;
/// the runner drains them and maps them onto actions in dispatch.
pub enum BgEvent {
    /// An auth key validation completed (agent.auth flow).
    AuthValidated {
        provider: String,
        key: String,
        base_url: String,
        outcome: Result<(), String>,
        /// Models the key can see (empty when the listing failed).
        models: Vec<String>,
    },
    /// A provider's model list was fetched (model picker).
    ModelsListed {
        provider: String,
        result: Result<Vec<String>, String>,
    },
}

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
    /// Jump straight to buffer `index` (`Space b 0-9`); out-of-range
    /// indices are ignored (quiet no-op).
    SwitchBuffer(usize),
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
    // ---- agent (docs/design/agent.md) ----
    /// Spawn the session if absent (chat views do this on open so a
    /// persisted transcript restores immediately). Resolve failures are
    /// quiet here — they surface on the first prompt instead.
    EnsureAgentSession,
    /// Send a prompt to the session (spawning it lazily).
    AgentPrompt(String),
    /// Clear transcript + persisted store (fresh conversation).
    AgentNewChat,
    /// Validate + persist an API key (agent auth flow).
    AuthSubmit {
        provider: String,
        key: String,
        base_url: String,
    },
    /// Validation completed (from a background thread via BgEvent).
    AuthResult {
        provider: String,
        key: String,
        base_url: String,
        /// Models the key can see (empty when the listing failed).
        models: Vec<String>,
        outcome: Result<(), String>,
    },
    /// Fetch model lists for all authenticated providers (model picker).
    FetchModels,
    /// A provider's model list arrived (from a background thread).
    ModelsListed {
        provider: String,
        result: Result<Vec<String>, String>,
    },
    /// Set + persist the default model for a provider.
    SetModel {
        provider: String,
        model: String,
    },
    /// Open (or refresh) the log viewer at a minimum level.
    OpenLogs {
        min: log::LevelFilter,
    },
    /// Reload the current buffer from disk (discards local edits —
    /// the command confirms first when dirty).
    ReloadBuffer,
    /// Reload all buffers from disk (`force` discards local edits —
    /// confirmed upstream).
    ReloadAllBuffers {
        force: bool,
    },
    /// Abort the current run.
    AgentAbort,
    /// One runtime event (streamed delta, tool lifecycle, run end).
    Agent(eggplant_agent::AgentEvent),

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
            | AppAction::SwitchBuffer(_)
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
        self.broadcast(event, app);
    }

    /// Broadcast a fact to every layer's `observe`. Dispatch uses it after
    /// applying; the agent host drain uses it directly when a host call
    /// mutated buffers (there is no AppAction for "the agent's tool
    /// edited this buffer" — the serving IS the mutation).
    pub fn broadcast(&mut self, event: ActionEvent, app: &App) {
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
            AppAction::SwitchBuffer(index) => {
                // Quick-choose: out-of-range is a quiet no-op, not an error.
                let _ = app.editor.switch_buffer(index);
            }
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

            AppAction::EnsureAgentSession => {
                let cwd = app.workspace.root.clone();
                let _ = app.agent.ensure_session(cwd);
            }
            AppAction::AuthSubmit {
                provider,
                key,
                base_url,
            } => {
                // Validate on a background thread; the result returns via
                // the effects channel (I/O never blocks the UI thread).
                let tx = app.bg_sender();
                std::thread::spawn(move || {
                    let outcome = eggplant_agent::validate_key(&base_url, &key);
                    // On success, discover the models immediately — login
                    // picks a default so the user never hits a stale one.
                    let models = if outcome.is_ok() {
                        eggplant_agent::list_models(&base_url, &key).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let _ = tx.send(BgEvent::AuthValidated {
                        provider,
                        key,
                        base_url,
                        outcome,
                        models,
                    });
                });
            }
            AppAction::AuthResult {
                provider,
                key,
                base_url,
                models,
                outcome,
            } => match outcome {
                Ok(()) => {
                    // Store the endpoint only when it overrides the preset
                    // default (keep auth.toml minimal).
                    let override_url = eggplant_agent::preset(&provider)
                        .filter(|p| p.base_url != base_url)
                        .map(|_| base_url.as_str());
                    // Login picks a working default model so the user
                    // never has to choose before the first chat.
                    let default_model = eggplant_agent::preset(&provider)
                        .and_then(|p| crate::agent::choose_default_model(p.default_model, &models))
                        .or_else(|| models.first().cloned());
                    let saved = eggplant_agent::AuthStore::load_default().map(|mut store| {
                        let saved = store.set(&provider, &key, override_url);
                        // The latest login becomes THE selected pair.
                        if saved.is_ok()
                            && let Some(model) = &default_model
                        {
                            let _ = store.set_current(&provider, model);
                        }
                        saved
                    });
                    match saved {
                        Some(Ok(())) => {
                            app.agent.auth_generation += 1;
                            if let Some(model) = &default_model {
                                app.agent.current = Some((provider.clone(), model.clone()));
                            }
                            app.agent.model_lists.insert(
                                provider.clone(),
                                crate::agent::ModelListState::Ready(models),
                            );
                            app.notifications.push(
                                crate::layers::notification::Notification::with_level(
                                    crate::layers::notification::Level::Info,
                                    match &default_model {
                                        Some(model) => {
                                            format!("{provider}: connected ✓ (model: {model})")
                                        }
                                        None => format!("{provider}: connected ✓"),
                                    },
                                ),
                            );
                        }
                        _ => {
                            app.notifications.push(
                                crate::layers::notification::Notification::error(format!(
                                    "{provider}: key valid but could not save auth.toml"
                                )),
                            );
                        }
                    }
                }
                Err(message) => {
                    app.notifications
                        .push(crate::layers::notification::Notification::error(format!(
                            "{provider}: {message}"
                        )));
                }
            },
            AppAction::AgentNewChat => {
                if let Some(session) = app.agent.session() {
                    session.new_chat();
                } else {
                    app.agent.transcript.clear();
                }
            }
            AppAction::AgentPrompt(text) => {
                let cwd = app.workspace.root.clone();
                match app.agent.ensure_session(cwd) {
                    Ok(session) => {
                        session.prompt(text.clone());
                        app.agent
                            .transcript
                            .push(crate::agent::ChatItem::User(text));
                    }
                    Err(message) => app
                        .notifications
                        .push(crate::layers::notification::Notification::error(message)),
                }
            }
            AppAction::FetchModels => {
                // One background thread per authenticated provider whose
                // list we don't already have.
                let store = eggplant_agent::AuthStore::load_default();
                for preset in eggplant_agent::provider::PRESETS {
                    let key = std::env::var(preset.api_key_env)
                        .ok()
                        .filter(|k| !k.is_empty())
                        .or_else(|| {
                            store
                                .as_ref()
                                .and_then(|s| s.get(preset.name).map(str::to_owned))
                        });
                    let Some(key) = key else { continue };
                    if matches!(
                        app.agent.model_lists.get(preset.name),
                        Some(crate::agent::ModelListState::Ready(_))
                            | Some(crate::agent::ModelListState::Loading)
                    ) {
                        continue;
                    }
                    app.agent.model_lists.insert(
                        preset.name.to_string(),
                        crate::agent::ModelListState::Loading,
                    );
                    let base_url = store
                        .as_ref()
                        .and_then(|s| s.url_for(preset.name).map(str::to_owned))
                        .unwrap_or_else(|| preset.base_url.to_string());
                    let provider = preset.name.to_string();
                    let tx = app.bg_sender();
                    std::thread::spawn(move || {
                        let result = eggplant_agent::list_models(&base_url, &key);
                        let _ = tx.send(BgEvent::ModelsListed { provider, result });
                    });
                }
            }
            AppAction::ModelsListed { provider, result } => {
                let state = match result {
                    Ok(models) => crate::agent::ModelListState::Ready(models),
                    Err(e) => crate::agent::ModelListState::Error(e),
                };
                app.agent.model_lists.insert(provider, state);
            }
            AppAction::SetModel { provider, model } => {
                let saved = eggplant_agent::AuthStore::load_default()
                    .map(|mut store| store.set_current(&provider, &model));
                match saved {
                    Some(Ok(())) => {
                        app.agent.current = Some((provider.clone(), model.clone()));
                        // The running session pinned the old model at
                        // spawn — drop it; the next prompt respawns with
                        // the new model, history preserved via the store.
                        app.agent.drop_session();
                        app.notifications.push(
                            crate::layers::notification::Notification::with_level(
                                crate::layers::notification::Level::Info,
                                format!("{provider}: model → {model}"),
                            ),
                        );
                    }
                    _ => {
                        app.notifications
                            .push(crate::layers::notification::Notification::error(format!(
                                "{provider}: could not save model"
                            )));
                    }
                }
            }
            AppAction::OpenLogs { min } => {
                app.logs_level = min;
                let Some(root) = eggplant_agent::store::data_root() else {
                    return;
                };
                let path = eggplant_core::logging::path(&root);
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let filtered = eggplant_core::logging::filter_level(&content, min);
                let header = format!("logs [{min}] — press L for the level menu — read-only\n\n");
                app.editor
                    .open_viewer("logs", &format!("{header}{filtered}"));
                self.broadcast(crate::action::ActionEvent::BufferChanged, app);
            }
            AppAction::ReloadBuffer => match app.editor.reload_current() {
                Ok(()) => {
                    let name = app.editor.display_name().unwrap_or_default();
                    app.notifications
                        .push(crate::layers::notification::Notification::with_level(
                            crate::layers::notification::Level::Info,
                            format!("reloaded {name}"),
                        ));
                    self.broadcast(crate::action::ActionEvent::BufferChanged, app);
                }
                Err(e) => {
                    app.notifications
                        .push(crate::layers::notification::Notification::error(format!(
                            "reload: {e}"
                        )));
                }
            },
            AppAction::ReloadAllBuffers { force } => {
                let outcome = app.editor.reload_all(force);
                let names: Vec<String> = outcome
                    .skipped
                    .iter()
                    .map(|p| {
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    })
                    .collect();
                let message = if names.is_empty() {
                    format!("reloaded {} buffer(s)", outcome.reloaded.len())
                } else {
                    format!(
                        "reloaded {} buffer(s), skipped dirty: {}",
                        outcome.reloaded.len(),
                        names.join(", ")
                    )
                };
                app.notifications
                    .push(crate::layers::notification::Notification::with_level(
                        crate::layers::notification::Level::Info,
                        message,
                    ));
                if !outcome.reloaded.is_empty() {
                    self.broadcast(crate::action::ActionEvent::BufferChanged, app);
                }
            }
            AppAction::AgentAbort => {
                if let Some(session) = app.agent.session() {
                    session.abort();
                }
            }
            AppAction::Agent(event) => app.agent.apply(&event),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn two_buffer_app() -> App {
        let dir = std::env::temp_dir().join(format!("eggplant-sw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        let mut app = App::new(eggplant_core::Editor::open(dir.join("a.txt")).unwrap());
        app.editor.open_buffer(dir.join("b.txt")).unwrap();
        app.editor.switch_buffer(0).unwrap();
        app
    }

    #[test]
    fn switch_buffer_jumps_in_range_and_ignores_out_of_range() {
        let mut app = two_buffer_app();
        let mut compositor = Compositor::default();

        compositor.dispatch(AppAction::SwitchBuffer(1), None, &mut app);
        assert_eq!(app.editor.current_buffer(), Some(1));

        compositor.dispatch(AppAction::SwitchBuffer(9), None, &mut app);
        assert_eq!(
            app.editor.current_buffer(),
            Some(1),
            "out-of-range index is a quiet no-op"
        );

        compositor.dispatch(AppAction::SwitchBuffer(0), None, &mut app);
        assert_eq!(app.editor.current_buffer(), Some(0));
    }
}
