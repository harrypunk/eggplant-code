//! The search prompt container (`Space s b`): owns the input, runs the
//! search live on every keystroke (incsearch-style), delegates rendering to
//! the pure `components::prompt` view.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::App;
use crate::commands::KeyStroke;
use crate::components::prompt::{self, PromptProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;

/// The search prompt's closed action set (config: `[keys.prompt]`).
/// Chars and Backspace edit the pattern — text-field behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptAction {
    /// Keep the highlight for `n`/`N` cycling.
    Confirm,
    /// Clear the highlight and close.
    Close,
}

impl PromptAction {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "confirm" => Self::Confirm,
            "close" => Self::Close,
            _ => return None,
        })
    }
}

/// Default prompt bindings.
pub const DEFAULT_KEYS: &[(KeyStroke, PromptAction)] = &[
    (
        KeyStroke::new(KeyCode::Enter, KeyModifiers::NONE),
        PromptAction::Confirm,
    ),
    (
        KeyStroke::new(KeyCode::Esc, KeyModifiers::NONE),
        PromptAction::Close,
    ),
];

#[derive(Default)]
pub struct SearchPrompt {
    input: String,
}

impl SearchPrompt {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Layer for SearchPrompt {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        prompt::view(
            &PromptProps {
                label: "/",
                input: self.input.clone(),
            },
            area,
            &app.theme,
        )
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        if let Some(action) = crate::editing::lookup(&app.layer_keys.prompt, &key) {
            return match action {
                // Close clears the highlight; Confirm keeps it for n/N.
                PromptAction::Close => {
                    app.editor.clear_search();
                    KeyResult::Close
                }
                PromptAction::Confirm => KeyResult::Close,
            };
        }
        match key.code {
            KeyCode::Backspace => {
                self.input.pop();
                app.editor.search(&self.input);
                KeyResult::Consumed
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                app.editor.search(&self.input);
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed, // modal-ish
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "search-prompt"
    }
}
