//! The compositor: an ordered stack of layers.
//!
//! - Index 0 is the base layer (editor surface).
//! - Higher indices render on top and receive key focus first.
//! - Layers are z-ordered; dialogs/floats are just layers pushed above.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::App;

/// A renderable, focusable UI layer.
pub trait Layer {
    /// Render this layer into `area`.
    fn render(&self, frame: &mut Frame, area: Rect, app: &App);

    /// Handle a key. Return `true` if consumed.
    fn handle_key(&mut self, _key: KeyEvent, _app: &mut App) -> bool {
        false
    }

    /// Whether this layer is translucent (layers below still show through).
    /// Dialogs/floats are translucent; the base editor surface is not.
    /// (Used for dimming below-layers in M2.)
    #[allow(dead_code)]
    fn translucent(&self) -> bool {
        false
    }

    /// Type tag helpers for M0 focus rules.
    fn is_dialog(&self) -> bool {
        false
    }
}

pub struct Compositor {
    layers: Vec<Box<dyn Layer>>,
}

impl Compositor {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn push(&mut self, layer: Box<dyn Layer>) {
        self.layers.push(layer);
    }

    pub fn pop(&mut self) {
        // Never pop the base editor surface.
        if self.layers.len() > 1 {
            self.layers.pop();
        }
    }

    pub fn has_dialog(&self) -> bool {
        self.layers.iter().any(|l| l.is_dialog())
    }

    /// Send a key to the topmost layer. Returns `true` if consumed.
    pub fn dispatch_key(&mut self, key: KeyEvent, app: &mut App) -> bool {
        if let Some(top) = self.layers.last_mut() {
            return top.handle_key(key, app);
        }
        false
    }

    /// Render bottom-up. When a translucent layer is on top, everything below
    /// it still renders (dimming is a later refinement).
    pub fn render(&self, frame: &mut Frame, area: Rect, app: &App) {
        for layer in &self.layers {
            layer.render(frame, area, app);
        }
    }
}
