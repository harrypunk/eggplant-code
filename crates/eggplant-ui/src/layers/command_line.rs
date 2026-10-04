//! The `:` command line container — input state + keys; view is pure
//! (see `components::command_line`).
//!
//! `Enter` runs the input as an ex command (see `crate::ex_commands`),
//! `Esc` cancels. Append-only editing (chars + backspace) for now.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::App;
use crate::components::command_line;
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;

#[derive(Default)]
pub struct CommandLine {
    input: String,
}

impl CommandLine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Layer for CommandLine {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        command_line::view(&self.input, area, &app.theme)
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Esc => KeyResult::Close,
            KeyCode::Enter => KeyResult::RunEx(std::mem::take(&mut self.input)),
            KeyCode::Backspace => {
                self.input.pop();
                KeyResult::Consumed
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed, // modal-ish: swallow everything else
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "command-line"
    }
}
