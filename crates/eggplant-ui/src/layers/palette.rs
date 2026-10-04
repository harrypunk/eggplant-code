//! The command palette container (`Space` in normal mode) — owns the input
//! and selection, derives the filtered list (selector), delegates rendering
//! to the pure `components::palette` view.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::App;
use crate::commands::Command;
use crate::components::palette::{self, PaletteItem, PaletteProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;
use crate::fuzzy;

pub struct Palette {
    input: String,
    /// Snapshot of the registry's commands (palette executes by value).
    commands: Vec<Command>,
    selected: usize,
}

impl Palette {
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            input: String::new(),
            commands,
            selected: 0,
        }
    }

    /// Selector: commands matching the current input, best first, capped.
    fn filtered(&self) -> Vec<Command> {
        fuzzy::filter(&self.input, &self.commands, |c| c.id)
            .into_iter()
            .take(palette::MAX_ROWS as usize)
            .map(|(_, command)| *command)
            .collect()
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.filtered().len();
        if count == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
    }
}

impl Layer for Palette {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let items = self
            .filtered()
            .iter()
            .map(|command| PaletteItem {
                id: command.id,
                description: command.description,
            })
            .collect();
        palette::view(
            &PaletteProps {
                input: self.input.clone(),
                items,
                selected: self.selected,
            },
            area,
            &app.theme,
        )
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Esc => KeyResult::Close,
            KeyCode::Enter => match self.filtered().get(self.selected) {
                Some(command) => KeyResult::Execute(*command),
                None => KeyResult::Close,
            },
            KeyCode::Up => {
                self.move_selection(-1);
                KeyResult::Consumed
            }
            KeyCode::Down => {
                self.move_selection(1);
                KeyResult::Consumed
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.selected = 0;
                KeyResult::Consumed
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                self.selected = 0;
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed, // modal-ish
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "palette"
    }
}
