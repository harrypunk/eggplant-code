//! Darkness detection: which of the terminal's light/dark variants is
//! live right now. The terminal itself is the truth (OSC 11 background
//! query via `termbg`, which talks to `/dev/tty` directly) — no D-Bus,
//! no desktop-portal dependency, correct even when the theme was picked
//! manually.
//!
//! The trait is the seam (DIP): the event loop holds a `TerminalProbe`,
//! tests inject stubs.

use std::time::Duration;

/// Answers "is the terminal currently dark?". `None` = couldn't tell;
/// callers keep whatever theme they have.
pub trait DarknessProbe {
    fn is_dark(&mut self) -> Option<bool>;
}

/// OSC 11 query against the real terminal. A failing probe is dead
/// forever: terminals that don't answer would make every retry a
/// blocking timeout, and terminals that do answer keep answering.
pub struct TerminalProbe {
    alive: bool,
}

impl TerminalProbe {
    pub fn new() -> Self {
        Self { alive: true }
    }
}

impl Default for TerminalProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl DarknessProbe for TerminalProbe {
    fn is_dark(&mut self) -> Option<bool> {
        if !self.alive {
            return None;
        }
        match termbg::theme(Duration::from_millis(500)) {
            Ok(termbg::Theme::Dark) => Some(true),
            Ok(termbg::Theme::Light) => Some(false),
            Err(_) => {
                self.alive = false;
                None
            }
        }
    }
}
