//! eggplant-core — editor backend facade.
//!
//! The UI talks to the editing backend through this crate only, so the
//! backend (v1: helix-core) can be swapped for our own core later without
//! touching the UI.
//!
//! M1 will introduce the facade traits (documents, selections, edits)
//! wrapping `helix_core`.

/// Re-export of the v1 backend so the rest of the workspace never depends
/// on helix crates directly.
pub mod backend {
    pub use helix_core;
}
