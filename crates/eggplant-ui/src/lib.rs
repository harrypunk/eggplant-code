//! eggplant-ui — ratatui-based UI toolkit for eggplant-code.
//!
//! Owns the compositor (z-ordered layers), built-in layers (editor surface,
//! dialogs, notifications) and shared UI state.

pub mod app;
pub mod commands;
pub mod compositor;
pub mod layers;
pub mod runner;
pub mod statusline;
