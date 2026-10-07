//! eggplant-ui — ratatui-based UI toolkit for eggplant-code.
//!
//! Architecture (Rule 5, UI = f(state)):
//! - `components/` — pure view functions: props in, `Element` tree out.
//! - `layers/` + chrome adapters — containers: state, events, prop mapping.
//! - `element.rs` — the declarative tree + `paint`, the only `Frame` toucher.
//! - `compositor.rs` — z-ordered layers, layout, focus, key dispatch.

pub mod app;
pub mod commands;
pub mod components;
pub mod compositor;
pub mod config;
pub mod element;
pub mod fuzzy;
pub mod keymaps;
pub mod layers;
pub mod runner;
pub mod startup;
pub mod statusline;
pub mod terminal;
pub mod theme;
pub mod topbar;
pub mod viewport;
