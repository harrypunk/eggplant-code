//! `ThemeSpec`: the boundary type between theme *sources* (ghostty config,
//! built-ins, future user files) and theme *derivation* (`derive.rs`).
//!
//! A spec is terminal-agnostic raw color data — nothing here knows who
//! produced it, and nothing downstream knows where it came from (DIP).

use ratatui::style::Color;

/// Raw colors as a terminal defines them: background/foreground, optional
/// cursor/selection colors, and the 16 ANSI palette entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSpec {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Option<Color>,
    pub selection_bg: Option<Color>,
    pub selection_fg: Option<Color>,
    /// ANSI palette 0–15 (`None` = entry not defined by the source).
    pub palette: [Option<Color>; 16],
}

impl ThemeSpec {
    /// Palette entry `i`, falling back to the foreground when undefined.
    pub fn ansi(&self, i: usize) -> Color {
        self.palette[i].unwrap_or(self.foreground)
    }
}
