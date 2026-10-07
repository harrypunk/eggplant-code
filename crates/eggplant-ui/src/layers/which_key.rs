//! The which-key container (`Space` prefix menu): walks the static
//! `KeyNode` tree in `commands.rs`, showing available keys per level.
//! Leaf keys execute registry commands; group keys descend. `Esc` closes.

use eggplant_core::input::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

use crate::app::App;
use crate::commands::{KeyNode, WHICH_KEY_ROOT};
use crate::components::which_key::{self, KeyHint, WhichKeyProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
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
                key: node.key(),
                description: node.description(),
            })
            .collect();
        let path = std::iter::once("SPC".to_owned())
            .chain(self.path.iter().map(char::to_string))
            .collect::<Vec<_>>()
            .join(" ");
        which_key::view(&WhichKeyProps { path, hints }, area, &app.theme.current)
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let KeyCode::Char(c) = key.code else {
            // Modal-ish: Esc closes, everything else is swallowed.
            return if key.code == KeyCode::Esc {
                KeyResult::Close
            } else {
                KeyResult::Consumed
            };
        };
        match self.node.iter().find(|node| node.key() == c) {
            Some(KeyNode::Leaf { command, .. }) => match app.input.registry.by_id(command) {
                Some(command) => KeyResult::Execute(command),
                None => KeyResult::Close,
            },
            Some(KeyNode::Group { children, .. }) => {
                self.path.push(c);
                self.node = children;
                KeyResult::Consumed
            }
            None => KeyResult::Close, // unknown key: dismiss quietly
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "which-key"
    }
}
