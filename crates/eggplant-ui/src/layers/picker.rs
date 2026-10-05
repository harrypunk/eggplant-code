//! The generic picker container: owns the input and selection, derives the
//! filtered list (selector), delegates rendering to the pure
//! `components::picker` view. Concrete pickers (command palette, buffer
//! grep, …) are constructor functions over `PickerSpec`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::App;
use crate::components::picker::{self, PickerItem, PickerProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;
use crate::fuzzy;

/// What makes a picker concrete: its items plus three function pointers —
/// how to filter, how to display, what Enter does.
pub struct PickerSpec<T> {
    /// Frame title ("palette", "grep", …).
    pub title: &'static str,
    pub items: Vec<T>,
    /// Text the fuzzy filter matches against.
    pub text_of: fn(&T) -> &str,
    /// Display projection: (primary column, free-form text).
    pub project: fn(&T) -> (String, String),
    /// Enter on an item.
    pub on_select: fn(&T, &mut App) -> KeyResult,
}

pub struct Picker<T> {
    input: String,
    selected: usize,
    spec: PickerSpec<T>,
}

impl<T> Picker<T> {
    pub fn new(spec: PickerSpec<T>) -> Self {
        Self {
            input: String::new(),
            selected: 0,
            spec,
        }
    }

    /// Selector: items matching the current input, best first, capped.
    fn filtered(&self) -> Vec<&T> {
        fuzzy::filter(&self.input, &self.spec.items, self.spec.text_of)
            .into_iter()
            .take(picker::MAX_ROWS as usize)
            .map(|(_, item)| item)
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

impl<T: 'static> Layer for Picker<T> {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let items = self
            .filtered()
            .into_iter()
            .map(|item| {
                let (primary, secondary) = (self.spec.project)(item);
                PickerItem { primary, secondary }
            })
            .collect();
        picker::view(
            &PickerProps {
                title: self.spec.title,
                input: self.input.clone(),
                items,
                selected: self.selected,
            },
            area,
            &app.theme,
        )
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Esc => KeyResult::Close,
            KeyCode::Enter => match self.filtered().get(self.selected) {
                Some(item) => (self.spec.on_select)(item, app),
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
        self.spec.title
    }
}
