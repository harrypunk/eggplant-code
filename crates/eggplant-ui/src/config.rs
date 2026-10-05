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
use crate::editing::EditorAction;
use crate::layers::notification::Notification;
use crate::theme::Theme;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub theme: Option<String>,
    pub keys: Keys,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Keys {
    /// Mode-independent bindings: stroke → command id.
    pub global: BTreeMap<String, String>,
    pub normal: BTreeMap<String, String>,
    pub visual: BTreeMap<String, String>,
    pub insert: BTreeMap<String, String>,
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
        let mut warnings = Vec::new();
        if let Some(theme) = &self.theme {
            match Theme::by_name(theme) {
                Some(theme) => app.theme = theme,
                None => warnings.push(format!("unknown theme '{theme}'")),
            }
        }
        self.keys.apply(app, &mut warnings);
        for warning in warnings {
            app.notifications
                .push(Notification::warn(format!("config: {warning}")));
        }
    }
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
    use crate::editing::Keymaps;

    #[test]
    fn keystroke_parses_modifiers_names_and_chars() {
        assert_eq!(KeyStroke::parse("C-S-p"), Some(KeyStroke::ctrl_shift('p')));
        assert_eq!(KeyStroke::parse("Space"), Some(KeyStroke::char(' ')));
        assert_eq!(KeyStroke::parse("g"), Some(KeyStroke::char('g')));
        assert_eq!(KeyStroke::parse("F2"), Some(KeyStroke::function(2)));
        assert_eq!(
            KeyStroke::parse("left"),
            Some(KeyStroke::new(
                crossterm::event::KeyCode::Left,
                crossterm::event::KeyModifiers::NONE
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
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(';'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            crate::editing::resolve(&mut Default::default(), Mode::Normal, key, &keymaps),
            crate::editing::Resolved::Act(EditorAction::EnterInsert, 1)
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
        // theme untouched, default keymap intact, two warnings raised
        assert_eq!(app.theme.name, Theme::default().name);
        assert_eq!(
            crate::editing::resolve(
                &mut Default::default(),
                Mode::Normal,
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(';'),
                    crossterm::event::KeyModifiers::NONE
                ),
                &app.keymaps
            ),
            crate::editing::Resolved::Ignored
        );
    }
}
