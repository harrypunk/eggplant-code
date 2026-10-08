//! The which-key container (`Space` prefix menu): walks the static
//! `KeyNode` tree in `commands.rs`, showing available keys per level.
//! Leaf keys execute registry commands; group keys descend. `Esc` closes.

use eggplant_core::input::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

use crate::action::{AppAction, Handled};
use crate::app::App;
use crate::commands::{KeyNode, WHICH_KEY_ROOT};
use crate::components::which_key::{self, KeyHint, WhichKeyProps};
use crate::compositor::{Layer, LayerKind};
use crate::element::Element;

pub struct WhichKey {
    /// Keys pressed so far (below the `Space` root), for the hint bar.
    path: Vec<char>,
    /// Current subtree.
    node: &'static [KeyNode],
}

impl WhichKey {
    pub fn root() -> Self {
        Self {
            path: Vec::new(),
            node: WHICH_KEY_ROOT,
        }
    }
}

impl Layer for WhichKey {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let hints = self
            .node
            .iter()
            .map(|node| KeyHint {
                // Digit ranges label as "0-9" — one row, not ten.
                key: node
                    .key()
                    .map_or_else(|| "0-9".to_owned(), |c| c.to_string()),
                description: node.description(),
            })
            .collect();
        let path = std::iter::once("SPC".to_owned())
            .chain(self.path.iter().map(char::to_string))
            .collect::<Vec<_>>()
            .join(" ");
        which_key::view(&WhichKeyProps { path, hints }, area, &app.theme.sheet())
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> Handled {
        let KeyCode::Char(c) = key.code else {
            // Modal-ish: Esc closes, everything else is swallowed.
            return if key.code == KeyCode::Esc {
                Handled::one(AppAction::CloseSelf)
            } else {
                Handled::quiet()
            };
        };
        // Digit leaves: any digit resolves through the parameterized node.
        if c.is_ascii_digit()
            && let Some(act) = self.node.iter().find_map(KeyNode::digit_action)
        {
            return Handled::Acted(vec![
                act(c.to_digit(10).unwrap() as usize),
                AppAction::CloseSelf,
            ]);
        }
        match self.node.iter().find(|node| node.key() == Some(c)) {
            // Run translates + dispatches the command; then this menu closes.
            Some(KeyNode::Leaf { command, .. }) => match app.input.registry.by_id(command) {
                Some(command) => {
                    Handled::Acted(vec![AppAction::Run(command), AppAction::CloseSelf])
                }
                None => Handled::one(AppAction::CloseSelf),
            },
            Some(KeyNode::Group { children, .. }) => {
                self.path.push(c);
                self.node = children;
                Handled::quiet()
            }
            None => Handled::one(AppAction::CloseSelf), // unknown key: dismiss quietly
            // DigitLeaves have no fixed key — digits are handled above.
            Some(KeyNode::DigitLeaves { .. }) => Handled::quiet(),
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "which-key"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggplant_core::input::KeyModifiers;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn digits_under_buffer_group_emit_switch_actions() {
        let app = App::new(eggplant_core::Editor::scratch().unwrap());
        let mut layer = WhichKey::root();
        // Descend: Space b (the +buffer group).
        layer.handle_key(key('b'), &app);
        // Then a digit: parsed to an index, parameterized action emitted.
        let Handled::Acted(actions) = layer.handle_key(key('3'), &app) else {
            panic!("digit must act");
        };
        assert!(
            matches!(actions[0], AppAction::SwitchBuffer(3)),
            "digit → SwitchBuffer(3)"
        );
        assert!(matches!(actions[1], AppAction::CloseSelf));
    }

    #[test]
    fn digits_elsewhere_dismiss_quietly() {
        let app = App::new(eggplant_core::Editor::scratch().unwrap());
        let mut layer = WhichKey::root();
        // Root has no digit leaves: '3' is unknown → dismiss.
        let Handled::Acted(actions) = layer.handle_key(key('3'), &app) else {
            panic!("must act");
        };
        assert!(matches!(actions.as_slice(), [AppAction::CloseSelf]));
    }
}
