//! Floating dialog layers: a generic message dialog and a yes/no confirm dialog.

use eggplant_core::input::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::App;
use crate::commands::KeyStroke;
use crate::components::dialog;
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;

/// Simple modal message dialog (demo of the float layer kind).
pub struct Dialog {
    title: String,
    body: String,
}

impl Dialog {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
        }
    }
}

impl Layer for Dialog {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        dialog::dialog_view(&self.title, &self.body, area, &app.theme.current)
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            // Modal: Esc or the toggle key closes it, everything else is
            // swallowed so it can't leak to layers below.
            KeyCode::Esc | KeyCode::F(2) => KeyResult::Close,
            _ => KeyResult::Consumed,
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "dialog"
    }
}

/// Callback run when a `ConfirmDialog` is accepted.
type ConfirmAction = Box<dyn FnOnce(&mut App)>;

/// Modal yes/no confirmation; runs `on_confirm` when accepted.
/// The confirm dialog's closed action set (config: `[keys.dialog]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    Confirm,
    Cancel,
}

impl DialogAction {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "confirm" => Self::Confirm,
            "cancel" => Self::Cancel,
            _ => return None,
        })
    }
}

/// Default dialog bindings.
pub const DEFAULT_KEYS: &[(KeyStroke, DialogAction)] = &[
    (KeyStroke::char('y'), DialogAction::Confirm),
    (
        KeyStroke::new(KeyCode::Enter, KeyModifiers::NONE),
        DialogAction::Confirm,
    ),
    (KeyStroke::char('n'), DialogAction::Cancel),
    (
        KeyStroke::new(KeyCode::Esc, KeyModifiers::NONE),
        DialogAction::Cancel,
    ),
];

pub struct ConfirmDialog {
    title: String,
    message: String,
    on_confirm: Option<ConfirmAction>,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl FnOnce(&mut App) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            on_confirm: Some(Box::new(on_confirm)),
        }
    }
}

impl Layer for ConfirmDialog {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        dialog::confirm_view(&self.title, &self.message, area, &app.theme.current)
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        match eggplant_core::editing::lookup(&app.input.layer_keys.dialog, &key) {
            Some(DialogAction::Confirm) => {
                if let Some(on_confirm) = self.on_confirm.take() {
                    on_confirm(app);
                }
                KeyResult::Close
            }
            Some(DialogAction::Cancel) => KeyResult::Close,
            None => KeyResult::Consumed, // modal
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "confirm"
    }
}
