//! Pure view components — props in, `Element` tree out.
//!
//! Components never see `Frame`, `App`, or the compositor: they are the
//! presentational layer (React function components). Containers in
//! `crate::layers` own state and events and map them to props.

pub mod dialog;
pub mod editor;
pub mod files_panel;
pub mod picker;
pub mod preview;
pub mod prompt;
pub mod statusline;
pub mod toasts;
pub mod topbar;
pub mod welcome;
pub mod which_key;
