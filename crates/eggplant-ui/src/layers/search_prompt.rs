//! The search prompt container (`Space s b`): owns the input, runs the
//! search live on every keystroke (incsearch-style), delegates rendering to
//! the pure `components::prompt` view.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::App;
use crate::components::prompt::{self, PromptProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;

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
        match key.code {
            // Esc clears the highlight; Enter keeps it for n/N cycling.
            KeyCode::Esc => {
                app.editor.clear_search();
                KeyResult::Close
            }
            KeyCode::Enter => KeyResult::Close,
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
