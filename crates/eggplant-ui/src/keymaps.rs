//! Layer-local keymaps: each focusable layer's keys are **data**, like the
//! modal keymaps in `editing.rs` — a closed action enum, a default table,
//! and config overrides (`[keys.explorer]`…) that prepend/shadow.
//!
//! Layers look keys up and apply the action; they never match `KeyCode`
//! literals for bindings. Text-entry keys (typed chars, Backspace) are
//! text-field behavior, not bindings, and stay in the layers.

use crate::commands::KeyStroke;
use crate::layers::dialog::DialogAction;
use crate::layers::files_panel::ExplorerAction;
use crate::layers::leap::LeapAction;
use crate::layers::picker::PickerAction;
use crate::layers::search_prompt::PromptAction;

/// Every layer's effective keymap (defaults + config overrides). Layers
/// read it from `App` at key time — rebinding needs no layer rebuild.
pub struct LayerKeymaps {
    pub explorer: Vec<(KeyStroke, ExplorerAction)>,
    pub picker: Vec<(KeyStroke, PickerAction)>,
    pub prompt: Vec<(KeyStroke, PromptAction)>,
    pub dialog: Vec<(KeyStroke, DialogAction)>,
    pub leap: Vec<(KeyStroke, LeapAction)>,
}

impl Default for LayerKeymaps {
    fn default() -> Self {
        Self {
            explorer: crate::layers::files_panel::DEFAULT_KEYS.to_vec(),
            picker: crate::layers::picker::DEFAULT_KEYS.to_vec(),
            prompt: crate::layers::search_prompt::DEFAULT_KEYS.to_vec(),
            dialog: crate::layers::dialog::DEFAULT_KEYS.to_vec(),
            leap: crate::layers::leap::DEFAULT_KEYS.to_vec(),
        }
    }
}

/// Build a layer keymap: defaults with user binds prepended (shadowing).
pub fn with_overrides<T: Copy>(
    defaults: &[(KeyStroke, T)],
    overrides: Vec<(KeyStroke, T)>,
) -> Vec<(KeyStroke, T)> {
    overrides
        .into_iter()
        .chain(defaults.iter().copied())
        .collect()
}
