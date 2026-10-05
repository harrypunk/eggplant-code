//! The compositor: z-ordered layers, focus management, declarative layout.
//!
//! - `LayerKind::Base` fills whatever the docked panels leave (exactly one, at index 0).
//! - `LayerKind::Panel { side, size }` docks against the body area, shrinking the base.
//! - `LayerKind::Float` overlays the body area (positions itself, e.g. centered).
//! - The bottom row is global chrome: the statusline (not a layer).
//! - Windows are equals (neovim-style): focus moves directionally (`C-h`/`C-l`),
//!   never cycles. Floats are modal: while one is open it holds key focus, and
//!   closing it returns focus to the previously focused window.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::App;
use crate::commands::{Command, CommandKind};
use crate::element::{self, Element};
use crate::statusline;
use crate::topbar;

/// Result of dispatching a key to a layer.
///
/// Structural results (`Close`, `Unfocus`, `Push`) are handled by the
/// compositor itself; `Execute` is an effect the compositor runs on the
/// layer's behalf (layers can't touch the compositor directly).
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

/// A renderable, focusable UI layer — the *container* half of Rule 5.
///
/// Containers own state and handle events (effects); their `view` maps
/// state to props and delegates to a pure component in `crate::components`.
pub trait Layer {
    /// Describe this layer's UI as an element tree. Pure: no mutation, no
    /// painting — the compositor paints the returned tree (Rule 5).
    fn view(&self, area: Rect, app: &App, focused: bool) -> Element;

    /// Handle a key.
    fn handle_key(&mut self, _key: KeyEvent, _app: &mut App) -> KeyResult {
        KeyResult::Ignored
    }

    /// Lifecycle hook called by the compositor each frame, before rendering,
    /// with the layer's resolved area. Update viewport-dependent state here
    /// (scroll windows, page sizes) — `render` itself must stay pure (Rule 5).
    fn resize(&mut self, _area: Rect, _app: &App) {}

    /// How this layer participates in layout.
    fn kind(&self) -> LayerKind;

    /// Stable identifier (used for toggles and status hints).
    fn id(&self) -> &'static str;

    /// Whether this layer may receive key focus.
    fn focusable(&self) -> bool {
        true
    }
}

/// Screen areas for one frame: one per layer plus the chrome strips.
#[derive(Debug)]
pub struct LayoutSolution {
    pub layer_areas: Vec<Rect>,
    pub topbar: Rect,
    pub statusline: Rect,
}

/// Pure layout: derive every layer's area from the layer kinds.
///
/// Chrome: statusline on the last row; the buffer topbar sits on the first
/// row *above the editor window only* (panels span the full height beside
/// it — the vscode layout). Panels dock in z-order; the base fills the
/// remainder minus the topbar row; floats get the whole main area.
pub fn compute_layout(kinds: &[LayerKind], area: Rect) -> LayoutSolution {
    let [main, statusline] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    let mut remaining = main;
    let mut layer_areas = vec![main; kinds.len()];
    let mut topbar = Rect { height: 1, ..main };

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
            LayerKind::Base => {
                // Topbar aligns with the base window's left border.
                topbar = Rect {
                    height: 1,
                    ..remaining
                };
                layer_areas[i] = Rect {
                    y: remaining.y + 1,
                    height: remaining.height.saturating_sub(1),
                    ..remaining
                };
            }
            LayerKind::Float => layer_areas[i] = main,
            LayerKind::Panel { .. } => {}
        }
    }

    LayoutSolution {
        layer_areas,
        topbar,
        statusline,
    }
}

/// Direction for `Compositor::focus_direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
}

#[derive(Default)]
pub struct Compositor {
    layers: Vec<Box<dyn Layer>>,
    /// Focused window (index into `layers`); 0 = the base editor window.
    window_focus: usize,
    /// Focused float while one is open (floats are modal).
    float_focus: Option<usize>,
}

impl Compositor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, layer: Box<dyn Layer>) {
        let focusable = layer.focusable();
        let is_float = matches!(layer.kind(), LayerKind::Float);
        self.layers.push(layer);
        if !focusable {
            return;
        }
        let index = self.layers.len() - 1;
        if is_float {
            self.float_focus = Some(index);
        } else {
            self.window_focus = index;
        }
    }

    /// Remove the layer at `index` (never the base layer). Focus falls back
    /// to the base window / no float; stored indices shift down past `index`.
    pub fn remove(&mut self, index: usize) {
        if index == 0 || index >= self.layers.len() {
            return;
        }
        self.layers.remove(index);
        if self.window_focus == index {
            self.window_focus = 0;
        } else if self.window_focus > index {
            self.window_focus -= 1;
        }
        match self.float_focus {
            Some(f) if f == index => self.float_focus = None,
            Some(f) if f > index => self.float_focus = Some(f - 1),
            _ => {}
        }
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

    /// Index of the layer that currently holds key focus: the modal float
    /// while one is open, else the focused window.
    pub fn focused_index(&self) -> usize {
        if let Some(f) = self
            .float_focus
            .filter(|&f| self.layers.get(f).is_some_and(|l| l.focusable()))
        {
            return f;
        }
        self.window_focus.min(self.layers.len().saturating_sub(1))
    }

    /// Window indices in visual left→right order: left panels, base, right
    /// panels (matches `compute_layout`'s docking order).
    fn window_order(&self) -> Vec<usize> {
        let mut left = Vec::new();
        let mut base = None;
        let mut right = Vec::new();
        for (i, layer) in self.layers.iter().enumerate() {
            match layer.kind() {
                LayerKind::Base => base = Some(i),
                LayerKind::Panel {
                    side: Side::Left, ..
                } => left.push(i),
                LayerKind::Panel {
                    side: Side::Right, ..
                } => right.push(i),
                LayerKind::Float => {}
            }
        }
        left.extend(base);
        left.extend(right);
        left
    }

    /// Move window focus left/right (`C-h`/`C-l`). No-op at the edges.
    pub fn focus_direction(&mut self, direction: FocusDirection) {
        let order = self.window_order();
        let Some(pos) = order.iter().position(|&i| i == self.window_focus) else {
            return;
        };
        let next = match direction {
            FocusDirection::Left => pos.checked_sub(1),
            FocusDirection::Right => (pos + 1 < order.len()).then_some(pos + 1),
        };
        if let Some(next) = next {
            self.window_focus = order[next];
        }
    }

    /// Move focus back to the base editor window.
    pub fn unfocus(&mut self) {
        self.window_focus = 0;
    }

    /// Send a key to the focused layer, applying any structural request or
    /// effect (close/unfocus/push/execute/ex) it returns.
    /// Execute a command: the single dispatch path for palette, which-key,
    /// global keymap — and modal actions (`Edit` funnels into
    /// `editing::interpret`, the same path modal keys use).
    pub fn execute(&mut self, command: Command, app: &mut App) {
        match command.kind {
            CommandKind::App(f) => f(app, self),
            CommandKind::Edit(action) => crate::editing::interpret(action, 1, app),
        }
    }

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
                self.execute(command, app);
                KeyResult::Consumed
            }
            other => other,
        }
    }

    /// Pre-render lifecycle: give every layer its resolved area so it can
    /// update viewport-dependent state outside of `render` (Rule 5).
    pub fn resize(&mut self, area: Rect, app: &App) {
        let kinds: Vec<LayerKind> = self.layers.iter().map(|layer| layer.kind()).collect();
        let solution = compute_layout(&kinds, area);
        for (layer, area) in self.layers.iter_mut().zip(solution.layer_areas) {
            layer.resize(area, app);
        }
    }

    /// Paint the whole screen: layer views bottom-up, then chrome. Pure with
    /// respect to state — this only interprets element trees (Rule 5).
    pub fn render(&self, frame: &mut Frame, area: Rect, app: &App) {
        let kinds: Vec<LayerKind> = self.layers.iter().map(|layer| layer.kind()).collect();
        let solution = compute_layout(&kinds, area);
        let focused = self.focused_index();
        let focused_id = self.layers.get(focused).map(|layer| layer.id());

        let tree = Element::Stack(
            self.layers
                .iter()
                .enumerate()
                .map(|(i, layer)| {
                    Element::fixed(
                        solution.layer_areas[i],
                        layer.view(solution.layer_areas[i], app, i == focused),
                    )
                })
                .chain([
                    // Components fill their area; positioning is the parent's
                    // job — wrap chrome in Fixed so it lands in its strip.
                    Element::fixed(solution.topbar, topbar::view(app, solution.topbar)),
                    Element::fixed(
                        solution.statusline,
                        statusline::view(app, focused_id, solution.statusline),
                    ),
                    app.notifications.view(area, &app.theme),
                ])
                .collect(),
        );
        element::paint(frame, tree, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal stub layer for focus/layout tests.
    struct Stub {
        kind: LayerKind,
        id: &'static str,
    }

    impl Layer for Stub {
        fn view(&self, _area: Rect, _app: &App, _focused: bool) -> Element {
            Element::Empty
        }

        fn kind(&self) -> LayerKind {
            self.kind
        }

        fn id(&self) -> &'static str {
            self.id
        }
    }

    fn window(side: Side) -> Stub {
        Stub {
            kind: LayerKind::Panel { side, size: 10 },
            id: "panel",
        }
    }

    fn float() -> Stub {
        Stub {
            kind: LayerKind::Float,
            id: "float",
        }
    }

    fn compositor_with_base() -> Compositor {
        let mut compositor = Compositor::new();
        compositor.push(Box::new(Stub {
            kind: LayerKind::Base,
            id: "editor",
        }));
        compositor
    }

    #[test]
    fn windows_are_focused_on_push_and_floats_are_modal() {
        let mut c = compositor_with_base();
        assert_eq!(c.focused_index(), 0);

        c.push(Box::new(window(Side::Left))); // panel takes window focus
        assert_eq!(c.focused_index(), 1);

        c.push(Box::new(float())); // float is modal
        assert_eq!(c.focused_index(), 2);

        c.remove(2); // closing the float restores the window focus
        assert_eq!(c.focused_index(), 1);
    }

    #[test]
    fn directional_focus_walks_left_to_right_and_stops_at_edges() {
        let mut c = compositor_with_base();
        c.push(Box::new(window(Side::Left))); // order: panel(1), editor(0)

        c.focus_direction(FocusDirection::Right); // panel → editor
        assert_eq!(c.focused_index(), 0);
        c.focus_direction(FocusDirection::Right); // right edge: no-op
        assert_eq!(c.focused_index(), 0);
        c.focus_direction(FocusDirection::Left); // editor → panel
        assert_eq!(c.focused_index(), 1);
        c.focus_direction(FocusDirection::Left); // left edge: no-op
        assert_eq!(c.focused_index(), 1);
    }

    #[test]
    fn removing_the_focused_window_falls_back_to_base() {
        let mut c = compositor_with_base();
        c.push(Box::new(window(Side::Left)));
        assert_eq!(c.focused_index(), 1);
        c.remove(1);
        assert_eq!(c.focused_index(), 0);
    }

    fn area() -> Rect {
        Rect::new(0, 0, 100, 30)
    }

    #[test]
    fn statusline_and_topbar_paint_on_the_chrome_rows() {
        // Regression: an unwrapped statusline Element painted into the full
        // screen area, landing on row 0 and tinting the whole frame.
        use eggplant_core::Editor;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        use crate::layers::editor::EditorSurface;

        let app = App::new(Editor::scratch().unwrap());
        let mut compositor = Compositor::new();
        compositor.push(Box::new(EditorSurface::new()));

        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                compositor.render(frame, area, &app);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        // Mode pill " NORMAL " on the last row (y=4), not the first.
        assert_eq!(buffer[(1, 4)].symbol(), "N");
        assert_ne!(buffer[(1, 0)].symbol(), "N");
        // Buffer topbar on the first row: the sole scratch buffer shows as
        // " untitled " starting at x=1.
        assert_eq!(buffer[(1, 0)].symbol(), "u");
    }

    #[test]
    fn base_only_gets_body_minus_chrome() {
        let solution = compute_layout(&[LayerKind::Base], area());
        assert_eq!(solution.topbar, Rect::new(0, 0, 100, 1));
        assert_eq!(solution.statusline, Rect::new(0, 29, 100, 1));
        assert_eq!(solution.layer_areas[0], Rect::new(0, 1, 100, 28));
    }

    #[test]
    fn left_panel_spans_full_height_and_topbar_aligns_to_base() {
        let kinds = [
            LayerKind::Base,
            LayerKind::Panel {
                side: Side::Left,
                size: 30,
            },
        ];
        let solution = compute_layout(&kinds, area());
        // Panel spans the full main height (y=0), like vscode's sidebar.
        assert_eq!(solution.layer_areas[1], Rect::new(0, 0, 30, 29));
        // Topbar starts at the editor window's left border (x=30).
        assert_eq!(solution.topbar, Rect::new(30, 0, 70, 1));
        assert_eq!(solution.layer_areas[0], Rect::new(30, 1, 70, 28));
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
        assert_eq!(solution.layer_areas[0], Rect::new(30, 1, 50, 28));
        assert_eq!(solution.topbar, Rect::new(30, 0, 50, 1));
    }

    #[test]
    fn float_overlays_full_main_area() {
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
        assert_eq!(solution.layer_areas[0], Rect::new(30, 1, 70, 28));
    }
}
