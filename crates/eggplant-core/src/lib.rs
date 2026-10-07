//! eggplant-core — the headless editor engine.
//!
//! Everything that makes an editor an editor, with zero terminal
//! dependencies: the helix facade, the editing pipeline (keymaps, resolve,
//! interpret), viewport policy, workspace/files, tree, grep, fuzzy — plus
//! the input vocabulary shells translate into (docs/design/architecture.md).

pub mod editing;
pub mod editor;
pub mod files;
pub mod filetree;
pub mod highlight;
pub mod input;
pub mod mode;

pub use editor::{BufferInfo, Editor, Motion, Register};
pub use highlight::{HighlightedSpan, SyntaxScope};
pub use mode::Mode;

/// Re-export of the v1 backend so the rest of the workspace never depends
/// on helix crates directly.
pub mod backend {
    pub use helix_core;
    pub use helix_view;
}
