//! User configuration: one file, `~/.config/eggplant/config.toml`
//! (ghostty/alacritty-style — few domains, one place).
//!
//! ```toml
//! theme = "classic"
//!
//! [keys.global]
//! "C-x" = "app.quit"
//!
//! [keys.normal]
//! ";" = "edit.enter-insert"
//! ```
//!
//! Semantics: user bindings *shadow* defaults for the same stroke, new
//! strokes extend. Errors never crash: they become startup notifications
//! and the affected entry falls back to defaults.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eggplant_core::Mode;
use serde::Deserialize;

use crate::app::App;
use crate::commands::KeyStroke;
use crate::layers::dialog::DialogAction;
use crate::layers::files_panel::ExplorerAction;
use crate::layers::leap::LeapAction;
use crate::layers::notification::Notification;
use crate::layers::picker::PickerAction;
use crate::layers::search_prompt::PromptAction;
use eggplant_core::editing::EditorAction;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub theme: Option<String>,
    pub keys: Keys,
    pub files: Files,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Files {
    /// Gitignore-syntax patterns, appended after the built-in defaults
    /// (rust `target/`, `node_modules/`, python `__pycache__/`/`.venv/`…).
    /// `!pattern` re-includes — removing a default is a negation away.
    pub ignore: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Keys {
    /// Mode-independent bindings: stroke → command id.
    pub global: BTreeMap<String, String>,
    pub normal: BTreeMap<String, String>,
    pub visual: BTreeMap<String, String>,
    pub insert: BTreeMap<String, String>,
    /// Layer-local bindings: stroke → action id (see each layer's action
    /// enum: `ExplorerAction`, `PickerAction`, …).
    pub explorer: BTreeMap<String, String>,
    pub picker: BTreeMap<String, String>,
    pub prompt: BTreeMap<String, String>,
    pub dialog: BTreeMap<String, String>,
    pub leap: BTreeMap<String, String>,
}

impl Config {
    /// `$XDG_CONFIG_HOME/eggplant/config.toml` or `~/.config/eggplant/…`.
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|home| home.join(".config")))?;
        Some(base.join("eggplant").join("config.toml"))
    }

    /// Read the config file. `Ok(None)` when absent; `Err` on unreadable or
    /// malformed files (the caller notifies and boots with defaults).
    pub fn load() -> std::io::Result<Option<Self>> {
        let Some(path) = Self::path() else {
            return Ok(None);
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        toml::from_str(&text)
            .map(Some)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }

    /// Apply onto a freshly built app; problems become notifications.
    pub fn apply(self, app: &mut App) {
        // Theme resolution is NOT here: it lives in theme::resolve
        // (explicit name → ghostty → default), driven by startup::boot.
        let mut warnings = Vec::new();
        self.keys.apply(app, &mut warnings);
        if !self.files.ignore.is_empty() {
            app.workspace.set_ignore_patterns(&self.files.ignore);
        }
        for warning in warnings {
            app.notifications
                .push(Notification::warn(format!("config: {warning}")));
        }
    }
}

/// Apply one layer's `[keys.<scope>]` table: parse strokes, resolve action
/// ids, prepend (user binds shadow defaults), warn on anything unknown.
fn apply_layer<T: Copy>(
    scope: &str,
    table: &BTreeMap<String, String>,
    from_id: fn(&str) -> Option<T>,
    target: &mut Vec<(KeyStroke, T)>,
    warnings: &mut Vec<String>,
) {
    let entries = table
        .iter()
        .filter_map(
            |(stroke, id)| match (KeyStroke::parse(stroke), from_id(id)) {
                (Some(stroke), Some(action)) => Some((stroke, action)),
                (None, _) => {
                    warnings.push(format!("keys.{scope}: bad stroke '{stroke}'"));
                    None
                }
                (_, None) => {
                    warnings.push(format!("keys.{scope}: unknown action '{id}'"));
                    None
                }
            },
        )
        .collect::<Vec<_>>();
    target.splice(0..0, entries);
}

impl Keys {
    fn apply(self, app: &mut App, warnings: &mut Vec<String>) {
        for (stroke, id) in &self.global {
            match KeyStroke::parse(stroke) {
                Some(stroke) if app.registry.by_id(id).is_some() => app.registry.bind(stroke, id),
                Some(_) => warnings.push(format!("keys.global: unknown command '{id}'")),
                None => warnings.push(format!("keys.global: bad stroke '{stroke}'")),
            }
        }
        for (table, mode) in [
            (&self.normal, Mode::Normal),
            (&self.visual, Mode::Visual),
            (&self.insert, Mode::Insert),
        ] {
            let entries = table
                .iter()
                .filter_map(
                    |(stroke, id)| match (KeyStroke::parse(stroke), action_by_id(id)) {
                        (Some(stroke), Some(action)) => Some((stroke, action)),
                        (None, _) => {
                            warnings.push(format!("keys.{mode:?}: bad stroke '{stroke}'"));
                            None
                        }
                        (_, None) => {
                            warnings.push(format!("keys.{mode:?}: unknown action '{id}'"));
                            None
                        }
                    },
                )
                .collect::<Vec<_>>();
            app.keymaps.override_keys(mode, entries);
        }
        // Layer-local keymaps: same shadowing, typed per layer.
        apply_layer(
            "explorer",
            &self.explorer,
            ExplorerAction::from_id,
            &mut app.layer_keys.explorer,
            warnings,
        );
        apply_layer(
            "picker",
            &self.picker,
            PickerAction::from_id,
            &mut app.layer_keys.picker,
            warnings,
        );
        apply_layer(
            "prompt",
            &self.prompt,
            PromptAction::from_id,
            &mut app.layer_keys.prompt,
            warnings,
        );
        apply_layer(
            "dialog",
            &self.dialog,
            DialogAction::from_id,
            &mut app.layer_keys.dialog,
            warnings,
        );
        apply_layer(
            "leap",
            &self.leap,
            LeapAction::from_id,
            &mut app.layer_keys.leap,
            warnings,
        );
    }
}

/// Reverse lookup: action id → action (ids come from `EditorAction::ALL`).
fn action_by_id(id: &str) -> Option<EditorAction> {
    EditorAction::ALL
        .iter()
        .copied()
        .find(|action| action.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggplant_core::editing::Keymaps;

    #[test]
    fn keystroke_parses_modifiers_names_and_chars() {
        assert_eq!(KeyStroke::parse("C-S-p"), Some(KeyStroke::ctrl_shift('p')));
        assert_eq!(KeyStroke::parse("Space"), Some(KeyStroke::char(' ')));
        assert_eq!(KeyStroke::parse("g"), Some(KeyStroke::char('g')));
        assert_eq!(KeyStroke::parse("F2"), Some(KeyStroke::function(2)));
        assert_eq!(
            KeyStroke::parse("left"),
            Some(KeyStroke::new(
                eggplant_core::input::KeyCode::Left,
                eggplant_core::input::KeyModifiers::NONE
            ))
        );
        assert_eq!(KeyStroke::parse("C-q"), Some(KeyStroke::ctrl('q')));
        assert_eq!(KeyStroke::parse("-"), Some(KeyStroke::char('-')));
        assert!(KeyStroke::parse("X-q").is_none(), "unknown modifier");
        assert!(KeyStroke::parse("nope").is_none());
    }

    #[test]
    fn user_binding_shadows_the_default() {
        let mut keymaps = Keymaps::default();
        keymaps.override_keys(
            Mode::Normal,
            vec![(KeyStroke::char(';'), EditorAction::EnterInsert)],
        );
        let key = eggplant_core::input::KeyEvent::new(
            eggplant_core::input::KeyCode::Char(';'),
            eggplant_core::input::KeyModifiers::NONE,
        );
        assert_eq!(
            eggplant_core::editing::resolve(&mut Default::default(), Mode::Normal, key, &keymaps),
            eggplant_core::editing::Resolved::Act(EditorAction::EnterInsert, 1)
        );
    }

    #[test]
    fn layer_keymaps_override_and_warn() {
        let config: Config = toml::from_str(
            r#"
                [keys.explorer]
                "u" = "up"
                "j" = "bogus"
                [keys.picker]
                "C-j" = "down"
            "#,
        )
        .unwrap();
        let mut app = App::new(eggplant_core::Editor::scratch().unwrap());
        config.apply(&mut app);

        let key = |code, mods| eggplant_core::input::KeyEvent::new(code, mods);
        use eggplant_core::input::{KeyCode, KeyModifiers};
        // 'u' now means up in the explorer…
        assert_eq!(
            eggplant_core::editing::lookup(
                &app.layer_keys.explorer,
                &key(KeyCode::Char('u'), KeyModifiers::NONE)
            ),
            Some(ExplorerAction::MoveUp)
        );
        // …'j' still means down (bogus id warned, default intact)…
        assert_eq!(
            eggplant_core::editing::lookup(
                &app.layer_keys.explorer,
                &key(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(ExplorerAction::MoveDown)
        );
        // …and the picker gained C-j.
        assert_eq!(
            eggplant_core::editing::lookup(
                &app.layer_keys.picker,
                &key(KeyCode::Char('j'), KeyModifiers::CONTROL)
            ),
            Some(PickerAction::MoveDown)
        );
        assert!(
            app.notifications
                .iter()
                .any(|n| n.message().contains("unknown action 'bogus'"))
        );
    }

    #[test]
    fn unknown_config_entries_warn_and_fall_back() {
        let config: Config = toml::from_str(
            r#"
                theme = "nope"
                [keys.normal]
                ";" = "not.an.action"
            "#,
        )
        .unwrap();
        let mut app = App::new(eggplant_core::Editor::scratch().unwrap());
        config.apply(&mut app);
        // unknown theme names are handled by theme::resolve, not apply;
        // the keymap falls back to defaults and a warning is raised
        assert_eq!(
            eggplant_core::editing::resolve(
                &mut Default::default(),
                Mode::Normal,
                eggplant_core::input::KeyEvent::new(
                    eggplant_core::input::KeyCode::Char(';'),
                    eggplant_core::input::KeyModifiers::NONE
                ),
                &app.keymaps
            ),
            eggplant_core::editing::Resolved::Ignored
        );
    }
}
