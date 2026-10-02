//! The compositor: z-ordered layers, focus management, declarative layout.
//!
//! - `LayerKind::Base` fills whatever the docked panels leave (exactly one, at index 0).
//! - `LayerKind::Panel { side, size }` docks against the body area, shrinking the base.
//! - `LayerKind::Float` overlays the body area (positions itself, e.g. centered).
//! - The bottom row is global chrome: the statusline (not a layer).
//! - Key focus defaults to the topmost focusable layer; `focus_next` cycles.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::App;
use crate::commands::Command;
use crate::statusline;

/// Result of dispatching a key to a layer.
///
/// Structural results (`Close`, `Unfocus`, `Push`) are handled by the
/// compositor itself; `Execute`/`RunEx` are effects the compositor runs on
/// the layer's behalf (layers can't touch the compositor directly).
pub enum KeyResult {
    /// The layer handled the key; stop propagation.
    Consumed,
    /// The layer didn't handle the key; pass it on (e.g. to global keys).
    Ignored,
    /// The layer asks the compositor to close (remove) it.
    Close,
    /// The layer asks the compositor to move focus back to the base layer.
    Unfocus,
    /// The layer asks the compositor to push a new layer (takes focus).
    Push(Box<dyn Layer>),
    /// Close this layer, then run a registry command.
    Execute(Command),
    /// Close this layer, then run a `:` command line input.
    RunEx(String),
}

/// Which side a panel docks against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// How a layer participates in layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// The single bottom layer (editor surface); fills space left by panels.
    Base,
    /// Docked panel of `size` columns; togglable, focusable.
    Panel { side: Side, size: u16 },
    /// Overlay above the base; positions itself inside the body area.
    Float,
}

/// A renderable, focusable UI layer.
pub trait Layer {
    /// Render this layer into `area`. `focused` is hint for border/highlight styles.
    fn render(&self, frame: &mut Frame, area: Rect, app: &App, focused: bool);

    /// Handle a key.
    fn handle_key(&mut self, _key: KeyEvent, _app: &mut App) -> KeyResult {
        KeyResult::Ignored
    }

    /// How this layer participates in layout.
    fn kind(&self) -> LayerKind;

    /// Stable identifier (used for toggles and status hints).
    fn id(&self) -> &'static str;

    /// Whether this layer may receive key focus.
    fn focusable(&self) -> bool {
        true
    }
}

/// Screen areas for one frame: one per layer plus the statusline strip.
#[derive(Debug)]
pub struct LayoutSolution {
    pub layer_areas: Vec<Rect>,
    pub statusline: Rect,
}

/// Pure layout: derive every layer's area from the layer kinds.
/// Panels dock in z-order; the base fills the remainder; floats get the body.
pub fn compute_layout(kinds: &[LayerKind], area: Rect) -> LayoutSolution {
    let [body, statusline] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    let mut remaining = body;
    let mut layer_areas = vec![body; kinds.len()];

    for (i, kind) in kinds.iter().enumerate() {
        if let LayerKind::Panel { side, size } = *kind {
            let (dock, rest) = match side {
                Side::Left => {
                    let [dock, rest] =
                        Layout::horizontal([Constraint::Length(size), Constraint::Min(1)])
                            .areas(remaining);
                    (dock, rest)
                }
                Side::Right => {
                    let [rest, dock] =
                        Layout::horizontal([Constraint::Min(1), Constraint::Length(size)])
                            .areas(remaining);
                    (dock, rest)
                }
            };
            layer_areas[i] = dock;
            remaining = rest;
        }
    }

    for (i, kind) in kinds.iter().enumerate() {
        match kind {
            LayerKind::Base => layer_areas[i] = remaining,
            LayerKind::Float => layer_areas[i] = body,
            LayerKind::Panel { .. } => {}
        }
    }

    LayoutSolution {
        layer_areas,
        statusline,
    }
}

#[derive(Default)]
pub struct Compositor {
    layers: Vec<Box<dyn Layer>>,
    /// Explicit focus (index into `layers`); `None` = topmost focusable.
    focus: Option<usize>,
}

impl Compositor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, layer: Box<dyn Layer>) {
        let focusable = layer.focusable();
        self.layers.push(layer);
        if focusable {
            self.focus = Some(self.layers.len() - 1);
        }
    }

    /// Remove the layer at `index` (never the base layer).
    pub fn remove(&mut self, index: usize) {
        if index == 0 || index >= self.layers.len() {
            return;
        }
        self.layers.remove(index);
        self.focus = None; // fall back to topmost focusable
    }

    pub fn remove_by_id(&mut self, id: &str) {
        if let Some(index) = self.find(id) {
            self.remove(index);
        }
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id() == id)
    }

    pub fn has(&self, id: &str) -> bool {
        self.find(id).is_some()
    }

    /// Index of the layer that currently holds key focus.
    pub fn focused_index(&self) -> usize {
        let valid = self
            .focus
            .filter(|&i| self.layers.get(i).is_some_and(|l| l.focusable()));
        valid.unwrap_or_else(|| {
            self.layers
                .iter()
                .rposition(|layer| layer.focusable())
                .unwrap_or(0)
        })
    }

    /// Cycle focus through focusable layers (`Ctrl-W`).
    pub fn focus_next(&mut self) {
        let focusable: Vec<usize> = self
            .layers
            .iter()
            .enumerate()
            .filter_map(|(i, layer)| layer.focusable().then_some(i))
            .collect();
        if focusable.len() < 2 {
            return;
        }
        let current = self.focused_index();
        let next = focusable
            .iter()
            .cycle()
            .find(|&&i| i > current)
            .unwrap_or(&focusable[0]);
        self.focus = Some(*next);
    }

    /// Move focus back to the base layer.
    pub fn unfocus(&mut self) {
        self.focus = Some(0);
    }

    /// Send a key to the focused layer, applying any structural request or
    /// effect (close/unfocus/push/execute/ex) it returns.
    pub fn dispatch_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let index = self.focused_index();
        let Some(layer) = self.layers.get_mut(index) else {
            return KeyResult::Ignored;
        };
        let result = layer.handle_key(key, app);
        match result {
            KeyResult::Close => {
                self.remove(index);
                KeyResult::Consumed
            }
            KeyResult::Unfocus => {
                self.unfocus();
                KeyResult::Consumed
            }
            KeyResult::Push(layer) => {
                self.push(layer);
                KeyResult::Consumed
            }
            KeyResult::Execute(command) => {
                self.remove(index);
                (command.execute)(app, self);
                KeyResult::Consumed
            }
            KeyResult::RunEx(input) => {
                self.remove(index);
                crate::ex_commands::execute(app, self, &input);
                KeyResult::Consumed
            }
            other => other,
        }
    }

    /// Render all layers bottom-up, then the statusline chrome.
    pub fn render(&self, frame: &mut Frame, area: Rect, app: &App) {
        let kinds: Vec<LayerKind> = self.layers.iter().map(|layer| layer.kind()).collect();
        let solution = compute_layout(&kinds, area);
        let focused = self.focused_index();

        for (i, layer) in self.layers.iter().enumerate() {
            layer.render(frame, solution.layer_areas[i], app, i == focused);
        }

        let focused_id = self.layers.get(focused).map(|layer| layer.id());
        statusline::render(frame, solution.statusline, app, focused_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 100, 30)
    }

    #[test]
    fn base_only_gets_body_minus_statusline() {
        let solution = compute_layout(&[LayerKind::Base], area());
        assert_eq!(solution.statusline, Rect::new(0, 29, 100, 1));
        assert_eq!(solution.layer_areas[0], Rect::new(0, 0, 100, 29));
    }

    #[test]
    fn left_panel_docks_and_shrinks_base() {
        let kinds = [
            LayerKind::Base,
            LayerKind::Panel {
                side: Side::Left,
                size: 30,
            },
        ];
        let solution = compute_layout(&kinds, area());
        assert_eq!(solution.layer_areas[1], Rect::new(0, 0, 30, 29));
        assert_eq!(solution.layer_areas[0], Rect::new(30, 0, 70, 29));
    }

    #[test]
    fn left_and_right_panels_dock_in_z_order() {
        let kinds = [
            LayerKind::Base,
            LayerKind::Panel {
                side: Side::Left,
                size: 30,
            },
            LayerKind::Panel {
                side: Side::Right,
                size: 20,
            },
        ];
        let solution = compute_layout(&kinds, area());
        assert_eq!(solution.layer_areas[1], Rect::new(0, 0, 30, 29));
        assert_eq!(solution.layer_areas[2], Rect::new(80, 0, 20, 29));
        assert_eq!(solution.layer_areas[0], Rect::new(30, 0, 50, 29));
    }

    #[test]
    fn float_overlays_full_body() {
        let kinds = [
            LayerKind::Base,
            LayerKind::Panel {
                side: Side::Left,
                size: 30,
            },
            LayerKind::Float,
        ];
        let solution = compute_layout(&kinds, area());
        assert_eq!(solution.layer_areas[2], Rect::new(0, 0, 100, 29));
        assert_eq!(solution.layer_areas[0], Rect::new(30, 0, 70, 29));
    }
}
