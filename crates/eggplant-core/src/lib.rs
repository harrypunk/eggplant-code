//! eggplant-core — editor backend facade.
//!
//! The UI talks to the editing backend through this crate only, so the
//! backend (v1: helix-core/helix-view) can be swapped for our own core later
//! without touching the UI.

pub mod editor;
pub mod mode;

pub use editor::Editor;
pub use mode::Mode;

/// Re-export of the v1 backend so the rest of the workspace never depends
/// on helix crates directly.
pub mod backend {
    pub use helix_core;
    pub use helix_view;
}
