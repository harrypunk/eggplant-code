//! Shared application state.
//!
//! `App` is a *composition of cohesive slices*, not a flat bag: each
//! slice owns its data and behavior (`ThemeState` the probe cadence,
//! `InputState` the key-resolution pipeline, `Notifications` the toast
//! queue, `Workspace` the project scope). `App` itself holds only the
//! composition, the lifecycle, and cross-slice selectors. Layers receive
//! `App` uniformly (heterogeneous dispatch), but touch only their slice —
//! and express cross-slice writes as intents (see `layers::picker::Select`)
//! rather than reaching across.

use eggplant_core::Editor;

use crate::commands::{self, Registry};
use crate::layers::notification::{Notification, Notifications};
use crate::theme::Theme;
use eggplant_core::editing::{EditorCtx, Keymaps, PendingState};
use eggplant_core::files::Workspace;

/// Application lifecycle status. Not a `bool`: quitting is a state
/// transition, and this is where future states land (e.g. quit reasons,
/// restart-into-file) without becoming a pile of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifecycle {
    /// Normal operation.
    #[default]
    Running,
    /// Teardown requested; the event loop exits after the current event.
    Quitting,
}

/// A leap-jump target: `label` shown at `(line, col)`; typing it jumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeapLabel {
    pub label: char,
    pub line: usize,
    pub col: usize,
}

/// Leap-jump state (`Space g c`): the 2-char pattern typed so far, then the
/// labeled matches. `Some` = a leap is in progress (editor renders dimmed).
#[derive(Debug, Default, Clone)]
pub struct Leap {
    pub pattern: String,
    pub labels: Vec<LeapLabel>,
}

/// Theme state: the active theme, which source it follows (fixed vs. live
/// ghostty switch), and the re-probe cadence. Owns the probe timing so the
/// runner just asks "is a probe due?" / "did the theme flip?".
pub struct ThemeState {
    /// The active color theme (components read it via props adapters).
    pub current: Theme,
    follow: crate::theme::resolve::Follow,
    /// Event-loop ticks (250ms each) since startup; drives the probe
    /// cadence.
    ticks: u32,
}

impl ThemeState {
    /// A probe every ~3s (250ms ticks) — cheap (one OSC 11 round-trip),
    /// catches a theme flip shortly after it happens.
    const PROBE_EVERY_TICKS: u32 = 12;

    pub fn new(theme: Theme, follow: crate::theme::resolve::Follow) -> Self {
        Self {
            current: theme,
            follow,
            ticks: 0,
        }
    }

    /// Record one event-loop tick; `true` when a theme probe is due.
    pub fn tick(&mut self) -> bool {
        self.ticks = self.ticks.wrapping_add(1);
        self.ticks.is_multiple_of(Self::PROBE_EVERY_TICKS)
    }

    /// Raw tick count (the demo notification shows it).
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// Re-derive the theme if the terminal flipped light/dark; `true` when
    /// the active theme changed.
    pub fn refresh(&mut self, probe: &mut impl crate::theme::probe::DarknessProbe) -> bool {
        match crate::theme::resolve::refresh(&mut self.follow, probe) {
            Some(theme) => {
                self.current = theme;
                true
            }
            None => false,
        }
    }

    /// The components' styling handle (class → style; colors unreachable).
    pub fn sheet(&self) -> crate::stylesheet::Stylesheet<'_> {
        crate::stylesheet::Stylesheet::new(&self.current)
    }

    /// Cycle to the next builtin theme; returns the new theme's name.
    pub fn cycle(&mut self) -> &'static str {
        self.current = Theme::next_after(self.current.name);
        self.current.name
    }
}

/// The key-resolution pipeline: the global command registry, the modal
/// keymaps, the layer-local keymaps (config, compiled at startup), plus
/// the transient pending modal input (counts, armed operators).
pub struct InputState {
    /// The command registry: global keymap + palette contents.
    pub registry: Registry,
    /// Modal keymaps (compiled defaults + config overrides).
    pub keymaps: Keymaps,
    /// Layer-local keymaps (explorer, picker, prompt, dialog, leap).
    pub layer_keys: crate::keymaps::LayerKeymaps,
    /// Pending modal input — the statusline's showcmd-style hint reads
    /// it; written by dispatch from `editing::resolve`'s pure output.
    pub pending: PendingState,
}

impl InputState {
    /// The pending-input hint for the statusline (vim `showcmd` style).
    pub fn pending_hint(&self) -> Option<String> {
        self.pending.hint()
    }
}

pub struct App {
    /// Editing state: the core facade (buffers, cursor, modes, history).
    pub editor: Editor,
    /// The workspace: root + ignore rules (file picker/explorer scope).
    pub workspace: Workspace,
    /// Toast queue.
    pub notifications: Notifications,
    /// Theme slice: active theme + follow source + probe cadence.
    pub theme: ThemeState,
    /// Input slice: keymaps + pending modal input.
    pub input: InputState,
    /// Line fitting: soft-wrap when true, horizontal scroll when false
    /// (see docs/design/line-fitting.md). Toggled by `Space u w`.
    pub wrap: bool,
    /// Leap-jump in progress (Space g c).
    pub leap: Option<Leap>,
    lifecycle: Lifecycle,
}

impl App {
    // ---- overlay selectors: how active features decorate the editor ----
    // Narrow, stable queries; new overlay features (flash, multi-cursor…)
    // extend these arms — the editor surface and component never change.

    /// Editor-line decorations from active overlays (leap today):
    /// `(col, label)` chips.
    pub fn line_labels(&self, line: usize) -> Vec<(usize, char)> {
        self.leap
            .as_ref()
            .map(|leap| leap.labels_on_line(line))
            .unwrap_or_default()
    }

    /// Whether an active overlay wants the buffer text dimmed.
    pub fn dims_editor_text(&self) -> bool {
        self.leap.as_ref().is_some_and(Leap::dims_text)
    }

    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            workspace: Workspace::new(std::env::current_dir().unwrap_or_default()),
            notifications: Notifications::new(),
            theme: ThemeState::new(Theme::default(), crate::theme::resolve::Follow::Fixed),
            input: InputState {
                registry: commands::default_registry(),
                keymaps: Keymaps::default(),
                layer_keys: crate::keymaps::LayerKeymaps::default(),
                pending: PendingState::default(),
            },
            wrap: false,
            leap: None,
            lifecycle: Lifecycle::Running,
        }
    }

    /// The pending-input hint for the statusline (vim `showcmd` style).
    pub fn pending_hint(&self) -> Option<String> {
        self.input.pending_hint()
    }

    /// Request application shutdown (the event loop observes and exits).
    pub fn request_quit(&mut self) {
        self.lifecycle = Lifecycle::Quitting;
    }

    pub fn is_quitting(&self) -> bool {
        self.lifecycle == Lifecycle::Quitting
    }
}

impl EditorCtx for App {
    fn editor(&mut self) -> &mut Editor {
        &mut self.editor
    }

    fn notify(&mut self, message: &str) {
        self.notifications.push(Notification::info(message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_hint_formats_showcmd_style() {
        let mut app = App::new(Editor::scratch().unwrap());
        assert_eq!(app.pending_hint(), None);

        use eggplant_core::editing::PendingKey;

        app.input.pending.count = Some(5);
        assert_eq!(app.pending_hint().as_deref(), Some("5"));

        app.input.pending.key = Some((PendingKey::Delete, 1));
        app.input.pending.count = None;
        assert_eq!(app.pending_hint().as_deref(), Some("d"));

        app.input.pending.key = Some((PendingKey::Delete, 2));
        assert_eq!(app.pending_hint().as_deref(), Some("d2"));

        // Digits typed after the operator append: `d` then `3`.
        app.input.pending.key = Some((PendingKey::Yank, 1));
        app.input.pending.count = Some(3);
        assert_eq!(app.pending_hint().as_deref(), Some("y3"));
    }

    #[test]
    fn theme_probe_cadence_fires_every_twelve_ticks() {
        let mut theme = ThemeState::new(Theme::default(), crate::theme::resolve::Follow::Fixed);
        let dues: Vec<bool> = (0..25).map(|_| theme.tick()).collect();
        assert_eq!(theme.ticks(), 25);
        assert_eq!(
            dues.iter()
                .enumerate()
                .filter(|(_, due)| **due)
                .map(|(i, _)| i)
                .collect::<Vec<_>>(),
            vec![11, 23],
            "probe due on the 12th and 24th tick"
        );
    }
}
