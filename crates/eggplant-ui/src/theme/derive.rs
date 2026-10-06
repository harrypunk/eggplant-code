//! Semantic mapping: `ThemeSpec` → `Theme` (+ `SyntaxTheme`).
//!
//! The ONLY place that decides "accent = ANSI blue", "constants are ANSI
//! yellow", "the statusline is bg blended 6% toward fg". Pure function —
//! table-driven and fully unit-tested. New sources never touch this; new
//! semantic slots in `Theme` get one new line here.

use ratatui::style::Color;

use super::spec::ThemeSpec;
use super::{SyntaxTheme, Theme};

/// Derive a full semantic `Theme` from raw terminal colors.
pub fn theme(spec: &ThemeSpec) -> Theme {
    let bg = spec.background;
    let fg = spec.foreground;
    let p = |i: usize| spec.ansi(i);
    let surface = blend(bg, fg, 0.06);
    Theme {
        name: "ghostty",
        bg,
        fg,
        comment: p(8), // bright black is the canonical "muted"
        selection: spec.selection_bg.unwrap_or_else(|| blend(bg, fg, 0.15)),
        search_match: blend(bg, fg, 0.25),
        search_current: p(3),
        accent: p(4),     // blue
        accent_alt: p(3), // yellow
        surface,
        statusline: surface,
        mode_normal: p(4),
        mode_insert: p(2), // green
        info: p(6),        // cyan
        warn: p(3),
        error: p(1), // red
        syntax: syntax(spec),
    }
}

/// The 12 syntax scopes mapped onto the ANSI palette, base16-terminal
/// style: magenta keywords, green strings, blue functions, cyan types,
/// yellow constants/numbers, muted punctuation.
fn syntax(spec: &ThemeSpec) -> SyntaxTheme {
    let p = |i: usize| spec.ansi(i);
    SyntaxTheme {
        keyword: p(5),
        string: p(2),
        function: p(4),
        type_: p(6),
        constant: p(3),
        number: p(3),
        variable: spec.foreground,
        operator: spec.foreground,
        punctuation: p(8),
        attribute: p(3),
        special: p(6),
    }
}

/// Linear interpolation between two RGB colors (`t = 0.0` → `a`). Non-RGB
/// inputs yield `a` unchanged (derivation only ever sees parsed hex).
pub fn blend(a: Color, b: Color, t: f64) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg_, bb)) = (a, b) else {
        return a;
    };
    let mix = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Color::Rgb(mix(ar, br), mix(ag, bg_), mix(ab, bb))
}

/// Relative luminance heuristic for light/dark decisions (0.0–1.0).
pub fn luminance(color: Color) -> Option<f64> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    Some((0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb(r, g, b)
    }

    /// The palette from ghostty's "Cobalt Next" theme.
    fn cobalt_next() -> ThemeSpec {
        let mut palette = [None; 16];
        let entries = [
            (0, rgb(0x00, 0x00, 0x00)),
            (1, rgb(0xff, 0x52, 0x7b)),
            (2, rgb(0x8c, 0xc9, 0x8f)),
            (3, rgb(0xff, 0xc6, 0x4c)),
            (4, rgb(0x40, 0x9d, 0xd4)),
            (5, rgb(0xcb, 0xa3, 0xc7)),
            (6, rgb(0x37, 0xb5, 0xb4)),
            (7, rgb(0xd7, 0xde, 0xea)),
            (8, rgb(0x62, 0x74, 0x7f)),
            (15, rgb(0xff, 0xff, 0xff)),
        ];
        for (i, c) in entries {
            palette[i] = Some(c);
        }
        ThemeSpec {
            background: rgb(0x16, 0x2c, 0x35),
            foreground: rgb(0xd7, 0xde, 0xea),
            cursor: Some(rgb(0xff, 0xc6, 0x4c)),
            selection_bg: Some(rgb(0x21, 0x42, 0x4e)),
            selection_fg: None,
            palette,
        }
    }

    #[test]
    fn derive_maps_semantic_slots_from_the_palette() {
        let spec = cobalt_next();
        let theme = theme(&spec);
        assert_eq!(theme.bg, rgb(0x16, 0x2c, 0x35));
        assert_eq!(theme.fg, rgb(0xd7, 0xde, 0xea));
        assert_eq!(theme.accent, spec.ansi(4));
        assert_eq!(theme.error, spec.ansi(1));
        assert_eq!(theme.warn, spec.ansi(3));
        assert_eq!(theme.comment, spec.ansi(8));
        assert_eq!(theme.selection, spec.selection_bg.unwrap());
        assert_eq!(theme.syntax.keyword, spec.ansi(5));
        assert_eq!(theme.syntax.string, spec.ansi(2));
    }

    #[test]
    fn derive_falls_back_for_missing_palette_and_selection() {
        let spec = ThemeSpec {
            background: rgb(0x10, 0x10, 0x10),
            foreground: rgb(0xe0, 0xe0, 0xe0),
            cursor: None,
            selection_bg: None,
            selection_fg: None,
            palette: [None; 16],
        };
        let theme = theme(&spec);
        assert_eq!(theme.accent, spec.foreground, "missing palette → fg");
        assert!(
            matches!(theme.selection, Color::Rgb(..)),
            "selection is blended, never missing"
        );
    }

    #[test]
    fn blend_interpolates_endpoints() {
        assert_eq!(blend(rgb(0, 0, 0), rgb(255, 255, 255), 0.0), rgb(0, 0, 0));
        assert_eq!(
            blend(rgb(0, 0, 0), rgb(255, 255, 255), 1.0),
            rgb(255, 255, 255)
        );
        assert_eq!(blend(rgb(0, 0, 0), rgb(200, 100, 0), 0.5), rgb(100, 50, 0));
    }

    #[test]
    fn luminance_separates_light_from_dark() {
        assert!(luminance(rgb(0xe1, 0xe2, 0xe7)).unwrap() > 0.5); // TokyoNight Day
        assert!(luminance(rgb(0x16, 0x2c, 0x35)).unwrap() < 0.5); // Cobalt Next
        assert_eq!(luminance(Color::Reset), None);
    }

    #[test]
    fn every_derived_scope_is_rgb() {
        let theme = theme(&cobalt_next());
        let scopes = [
            theme.syntax.keyword,
            theme.syntax.string,
            theme.syntax.function,
            theme.syntax.type_,
            theme.syntax.constant,
            theme.syntax.number,
            theme.syntax.variable,
            theme.syntax.operator,
            theme.syntax.punctuation,
            theme.syntax.attribute,
            theme.syntax.special,
        ];
        assert!(scopes.iter().all(|c| matches!(c, Color::Rgb(..))));
    }
}
