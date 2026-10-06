//! Ghostty theme source: read the user's ghostty config, resolve its
//! `theme = …` directive (single or `light:X,dark:Y` conditional), and
//! parse the referenced theme files into `ThemeSpec`s.
//!
//! Only this module knows ghostty's file formats and search paths.
//! Parsing is pure (`&str` → data); filesystem discovery is one thin
//! shell around it, so the logic is unit-tested without fixtures on disk.

use std::path::{Path, PathBuf};

use ratatui::style::Color;

use super::spec::ThemeSpec;

/// Both variants of the user's ghostty theme. A single (non-conditional)
/// theme fills both.
#[derive(Debug, Clone, Copy)]
pub struct GhosttyTheme {
    pub light: ThemeSpec,
    pub dark: ThemeSpec,
}

/// The `theme = …` directive, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ThemeDirective {
    Single(String),
    Conditional { light: String, dark: String },
}

/// Load the user's ghostty theme (both variants).
///
/// `Err` covers every failure — no config, no theme key, missing theme
/// file, malformed values. Callers fall back to another theme source.
pub fn load() -> Result<GhosttyTheme, String> {
    let config = config_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "no ghostty config found".to_owned())?;
    let text = std::fs::read_to_string(&config)
        .map_err(|err| format!("cannot read {}: {err}", config.display()))?;
    let directive = parse_theme_directive(&text)?
        .ok_or_else(|| format!("{} sets no theme", config.display()))?;
    let dirs = theme_search_dirs();
    let resolve = |name: &str| resolve_theme_file(&dirs, name);
    let (light, dark) = match &directive {
        ThemeDirective::Single(name) => (resolve(name)?, resolve(name)?),
        ThemeDirective::Conditional { light, dark } => (resolve(light)?, resolve(dark)?),
    };
    Ok(GhosttyTheme { light, dark })
}

/// The last `theme = …` line wins (ghostty semantics); `None` when unset.
fn parse_theme_directive(text: &str) -> Result<Option<ThemeDirective>, String> {
    let mut directive = None;
    for (key, value) in assignments(text) {
        if key == "theme" {
            directive = Some(parse_directive_value(value)?);
        }
    }
    Ok(directive)
}

fn parse_directive_value(value: &str) -> Result<ThemeDirective, String> {
    // A colon marks the conditional form (`light:X,dark:Y`) — theme
    // names themselves never contain one.
    if !value.contains(':') {
        return Ok(ThemeDirective::Single(value.trim().to_owned()));
    }
    let mut light = None;
    let mut dark = None;
    for part in value.split(',') {
        let (scheme, name) = part
            .split_once(':')
            .ok_or_else(|| format!("malformed theme conditional '{part}'"))?;
        match scheme.trim() {
            "light" => light = Some(name.trim().to_owned()),
            "dark" => dark = Some(name.trim().to_owned()),
            other => return Err(format!("unknown theme scheme '{other}'")),
        }
    }
    match (light, dark) {
        (Some(light), Some(dark)) => Ok(ThemeDirective::Conditional { light, dark }),
        _ => Err("conditional theme needs both light: and dark:".to_owned()),
    }
}

/// `key = value` pairs: trimmed, blanks and `#` comments skipped. Keys are
/// lowercased by ghostty convention; values keep their case.
fn assignments(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        line.split_once('=')
            .map(|(key, value)| (key.trim(), value.trim()))
    })
}

/// Parse a ghostty theme file into a spec. Unknown keys are ignored —
/// we only consume colors we have semantic slots for.
fn parse_theme_file(text: &str) -> Result<ThemeSpec, String> {
    let mut background = None;
    let mut foreground = None;
    let mut cursor = None;
    let mut selection_bg = None;
    let mut selection_fg = None;
    let mut palette = [None; 16];
    for (key, value) in assignments(text) {
        match key {
            "background" => background = Some(parse_color(value)?),
            "foreground" => foreground = Some(parse_color(value)?),
            "cursor-color" => cursor = Some(parse_color(value)?),
            "selection-background" => selection_bg = Some(parse_color(value)?),
            "selection-foreground" => selection_fg = Some(parse_color(value)?),
            "palette" => {
                let (index, color) = value
                    .split_once('=')
                    .ok_or_else(|| format!("malformed palette entry '{value}'"))?;
                let index: usize = index
                    .trim()
                    .parse()
                    .map_err(|_| format!("malformed palette index '{index}'"))?;
                if index < 16 {
                    palette[index] = Some(parse_color(color.trim())?);
                }
            }
            _ => {}
        }
    }
    Ok(ThemeSpec {
        background: background.ok_or_else(|| "theme defines no background".to_owned())?,
        foreground: foreground.ok_or_else(|| "theme defines no foreground".to_owned())?,
        cursor,
        selection_bg,
        selection_fg,
        palette,
    })
}

/// `#RRGGBB` or `RRGGBB`.
fn parse_color(text: &str) -> Result<Color, String> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return Err(format!("malformed color '{text}'"));
    }
    let channel = |from: usize| {
        u8::from_str_radix(&hex[from..from + 2], 16)
            .map_err(|_| format!("malformed color '{text}'"))
    };
    Ok(Color::Rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// Candidate config paths, in priority order. Ghostty's default is
/// `config`; we also accept `config.ghostty` (a common personal naming).
fn config_candidates() -> Vec<PathBuf> {
    let Some(dir) = ghostty_config_dir() else {
        return Vec::new();
    };
    vec![dir.join("config"), dir.join("config.ghostty")]
}

/// Theme search directories, in priority order: user themes first, then
/// the installation's resources dir (AppImage, /usr, /usr/local).
fn theme_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ghostty_config_dir()
        .map(|dir| vec![dir.join("themes")])
        .unwrap_or_default();
    if let Ok(resources) = std::env::var("GHOSTTY_RESOURCES_DIR") {
        dirs.push(PathBuf::from(resources).join("themes"));
    }
    if let Some(exe) = which("ghostty") {
        // Ghostty resolves resources relative to its binary: <exe>/../share.
        if let Some(prefix) = exe.parent().and_then(Path::parent) {
            dirs.push(prefix.join("share").join("ghostty").join("themes"));
        }
    }
    dirs.push(PathBuf::from("/usr/share/ghostty/themes"));
    dirs.push(PathBuf::from("/usr/local/share/ghostty/themes"));
    dirs
}

fn ghostty_config_dir() -> Option<PathBuf> {
    xdg_config_home().map(|base| base.join("ghostty"))
}

fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".config")))
}

/// Find an executable on PATH (canonicalized, so AppImage mounts resolve).
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
            .and_then(|candidate| candidate.canonicalize().ok())
    })
}

fn resolve_theme_file(dirs: &[PathBuf], name: &str) -> Result<ThemeSpec, String> {
    for dir in dirs {
        let path = dir.join(name);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
            return parse_theme_file(&text);
        }
    }
    Err(format!("ghostty theme '{name}' not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKYONIGHT_DAY: &str = "\
palette = 0=#e9e9ed
palette = 1=#f52a65
palette = 4=#2e7de9
palette = 8=#a1a6c5
palette = 15=#3760bf
background = #e1e2e7
foreground = #3760bf
cursor-color = #3760bf
selection-background = #99a7df
selection-foreground = #3760bf
";

    #[test]
    fn directive_single_theme() {
        let text = "font-size = 13\ntheme = Cobalt Next\n";
        assert_eq!(
            parse_theme_directive(text).unwrap(),
            Some(ThemeDirective::Single("Cobalt Next".to_owned()))
        );
    }

    #[test]
    fn directive_conditional_trims_and_orders_independently() {
        let text = "theme = dark:Cobalt Next , light:TokyoNight Day\n";
        assert_eq!(
            parse_theme_directive(text).unwrap(),
            Some(ThemeDirective::Conditional {
                light: "TokyoNight Day".to_owned(),
                dark: "Cobalt Next".to_owned(),
            })
        );
    }

    #[test]
    fn directive_last_wins_and_missing_is_none() {
        let text = "theme = A\ntheme = B\n";
        assert_eq!(
            parse_theme_directive(text).unwrap(),
            Some(ThemeDirective::Single("B".to_owned()))
        );
        assert_eq!(parse_theme_directive("font-size = 13\n").unwrap(), None);
    }

    #[test]
    fn directive_conditional_requires_both_schemes() {
        assert!(parse_theme_directive("theme = light:A\n").is_err());
        assert!(parse_theme_directive("theme = noon:A, dark:B\n").is_err());
    }

    #[test]
    fn theme_file_parses_colors_and_palette() {
        let spec = parse_theme_file(TOKYONIGHT_DAY).unwrap();
        assert_eq!(spec.background, Color::Rgb(0xe1, 0xe2, 0xe7));
        assert_eq!(spec.foreground, Color::Rgb(0x37, 0x60, 0xbf));
        assert_eq!(spec.cursor, Some(Color::Rgb(0x37, 0x60, 0xbf)));
        assert_eq!(spec.selection_bg, Some(Color::Rgb(0x99, 0xa7, 0xdf)));
        assert_eq!(spec.ansi(4), Color::Rgb(0x2e, 0x7d, 0xe9));
        assert_eq!(spec.ansi(8), Color::Rgb(0xa1, 0xa6, 0xc5));
        // Undefined palette entries fall back to the foreground.
        assert_eq!(spec.ansi(3), spec.foreground);
    }

    #[test]
    fn theme_file_requires_bg_and_fg_and_skips_comments() {
        assert!(parse_theme_file("background = #000000\n").is_err());
        let spec = parse_theme_file(
            "# a comment\nbackground = 000000\nforeground = ffffff\n\nunknown-key = x\n",
        )
        .unwrap();
        assert_eq!(spec.background, Color::Rgb(0, 0, 0));
    }

    #[test]
    fn color_parsing_is_strict() {
        assert_eq!(
            parse_color("#a1b2c3").unwrap(),
            Color::Rgb(0xa1, 0xb2, 0xc3)
        );
        assert_eq!(parse_color("a1b2c3").unwrap(), Color::Rgb(0xa1, 0xb2, 0xc3));
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("#gg0000").is_err());
    }
}
