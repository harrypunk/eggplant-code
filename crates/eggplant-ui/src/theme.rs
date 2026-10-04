//! Color themes: semantic palette slots + a built-in registry.
//!
//! Components never hardcode colors — they read semantic slots from a
//! `Theme` passed in by their container (Rule 5: theme is state, stored in
//! `App`). Custom theme files (ghostty-style `~/.config/eggplant/themes`)
//! are planned for M7; the registry is the seam.

use ratatui::style::Color;

/// Semantic color slots every component reads from.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    /// Editor background / default text.
    pub bg: Color,
    pub fg: Color,
    /// Muted text: gutter numbers, descriptions, unfocused borders.
    pub comment: Color,
    /// Selected row / current-line background.
    pub selection: Color,
    /// Primary accent: focused borders, prompts.
    pub accent: Color,
    /// Secondary accent: current line number, focus tags.
    pub accent_alt: Color,
    /// Float/panel/toast background.
    pub surface: Color,
    pub statusline: Color,
    pub mode_normal: Color,
    pub mode_insert: Color,
    pub info: Color,
    pub warn: Color,
    pub error: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// Tokyo Night (night variant) — the default.
pub const TOKYO_NIGHT: Theme = Theme {
    name: "tokyo-night",
    bg: rgb(0x1a, 0x1b, 0x26),
    fg: rgb(0xc0, 0xca, 0xf5),
    comment: rgb(0x56, 0x5f, 0x89),
    selection: rgb(0x29, 0x2e, 0x42),
    accent: rgb(0x7a, 0xa2, 0xf7),
    accent_alt: rgb(0xe0, 0xaf, 0x68),
    surface: rgb(0x1f, 0x23, 0x35),
    statusline: rgb(0x1f, 0x23, 0x35),
    mode_normal: rgb(0x7a, 0xa2, 0xf7),
    mode_insert: rgb(0x9e, 0xce, 0x6a),
    info: rgb(0x7d, 0xcf, 0xff),
    warn: rgb(0xe0, 0xaf, 0x68),
    error: rgb(0xf7, 0x76, 0x8e),
};

/// The original ad-hoc look: follows the terminal's own palette.
pub const CLASSIC: Theme = Theme {
    name: "classic",
    bg: Color::Reset,
    fg: Color::Reset,
    comment: Color::DarkGray,
    selection: Color::DarkGray,
    accent: Color::Cyan,
    accent_alt: Color::Yellow,
    surface: Color::Black,
    statusline: Color::DarkGray,
    mode_normal: Color::Cyan,
    mode_insert: Color::Green,
    info: Color::Cyan,
    warn: Color::Yellow,
    error: Color::Red,
};

/// Built-in themes, in display order.
pub const BUILTINS: &[Theme] = &[TOKYO_NIGHT, CLASSIC];

impl Theme {
    pub fn by_name(name: &str) -> Option<Theme> {
        BUILTINS.iter().copied().find(|t| t.name == name)
    }

    pub fn available() -> Vec<&'static str> {
        BUILTINS.iter().map(|t| t.name).collect()
    }

    /// The theme after `current` in the registry (wrapping).
    pub fn next_after(current: &str) -> Theme {
        let index = BUILTINS
            .iter()
            .position(|t| t.name == current)
            .map(|i| i + 1)
            .unwrap_or(0);
        BUILTINS[index % BUILTINS.len()]
    }
}

impl Default for Theme {
    fn default() -> Self {
        TOKYO_NIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_name() {
        assert!(Theme::by_name("tokyo-night").is_some());
        assert!(Theme::by_name("dracula").is_none());
        assert!(Theme::available().contains(&"tokyo-night"));
    }
}
