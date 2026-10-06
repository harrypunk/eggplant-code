//! Theme resolution: pick the initial theme and follow live switches.
//!
//! Precedence: explicit `theme = "name"` in our config always wins →
//! ghostty (only when running inside ghostty) → built-in default.
//! `Follow` is pure state (kept on `App`); `refresh` re-derives the
//! `Theme` when the terminal flips light/dark.

use super::ghostty::{self, GhosttyTheme};
use super::probe::DarknessProbe;
use super::{Theme, derive};

/// Which source the active theme follows (App state; plain data).
#[derive(Debug, Clone, Copy)]
pub enum Follow {
    /// Explicit or default theme: never changes at runtime.
    Fixed,
    /// Following ghostty: holds both variants + which is active.
    Ghostty { pair: GhosttyTheme, dark: bool },
}

/// The outcome of initial resolution.
pub struct Resolution {
    pub theme: Theme,
    pub follow: Follow,
    pub warnings: Vec<String>,
}

/// Resolve the startup theme. `explicit` is `[theme] name` from our
/// config; `inside_ghostty` and `load` are seams so the policy is
/// testable without env vars or a filesystem.
pub fn initial(
    explicit: Option<&str>,
    inside_ghostty: bool,
    probe: &mut impl DarknessProbe,
) -> Resolution {
    let mut warnings = Vec::new();
    if let Some(name) = explicit {
        match Theme::by_name(name) {
            Some(theme) => {
                return Resolution {
                    theme,
                    follow: Follow::Fixed,
                    warnings,
                };
            }
            None => warnings.push(format!("unknown theme '{name}'")),
        }
    }
    if inside_ghostty && let Some(resolution) = follow_ghostty(probe, &mut warnings) {
        return resolution;
    }
    Resolution {
        theme: Theme::default(),
        follow: Follow::Fixed,
        warnings,
    }
}

/// Are we running inside a ghostty terminal?
pub fn inside_ghostty() -> bool {
    std::env::var("TERM_PROGRAM").as_deref() == Ok("ghostty")
}

/// Ghostty path: load both variants, probe which is live, derive.
fn follow_ghostty(
    probe: &mut impl DarknessProbe,
    warnings: &mut Vec<String>,
) -> Option<Resolution> {
    let pair = match ghostty::load() {
        Ok(pair) => pair,
        Err(err) => {
            warnings.push(format!("ghostty theme: {err}"));
            return None;
        }
    };
    // Probe failed → assume dark (our default stance); refresh retries.
    let dark = probe.is_dark().unwrap_or(true);
    let spec = if dark { &pair.dark } else { &pair.light };
    Some(Resolution {
        theme: derive::theme(spec),
        follow: Follow::Ghostty { pair, dark },
        warnings: std::mem::take(warnings),
    })
}

/// Re-probe the terminal; a flipped light/dark state re-derives the
/// theme. `None` = nothing changed (or not following, or probe dead).
pub fn refresh(follow: &mut Follow, probe: &mut impl DarknessProbe) -> Option<Theme> {
    let Follow::Ghostty { pair, dark } = follow else {
        return None;
    };
    let now = probe.is_dark()?;
    if now == *dark {
        return None;
    }
    *dark = now;
    Some(derive::theme(if now { &pair.dark } else { &pair.light }))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubProbe(Option<bool>);
    impl DarknessProbe for StubProbe {
        fn is_dark(&mut self) -> Option<bool> {
            self.0
        }
    }

    #[test]
    fn explicit_name_wins_and_never_follows() {
        let res = initial(Some("classic"), true, &mut StubProbe(Some(true)));
        assert_eq!(res.theme.name, "classic");
        assert!(matches!(res.follow, Follow::Fixed));
        assert!(res.warnings.is_empty());
    }

    #[test]
    fn unknown_name_warns_and_falls_back() {
        let mut res = initial(Some("nope"), false, &mut StubProbe(Some(true)));
        assert_eq!(res.theme.name, Theme::default().name);
        assert!(res.warnings.pop().unwrap().contains("unknown theme 'nope'"));
    }

    #[test]
    fn outside_ghostty_uses_the_default() {
        let res = initial(None, false, &mut StubProbe(Some(false)));
        assert_eq!(res.theme.name, Theme::default().name);
        assert!(matches!(res.follow, Follow::Fixed));
    }

    #[test]
    fn refresh_only_on_flip() {
        let spec = super::super::spec::ThemeSpec {
            background: ratatui::style::Color::Rgb(0, 0, 0),
            foreground: ratatui::style::Color::Rgb(255, 255, 255),
            cursor: None,
            selection_bg: None,
            selection_fg: None,
            palette: [None; 16],
        };
        let pair = GhosttyTheme {
            light: spec,
            dark: spec,
        };
        let mut follow = Follow::Ghostty { pair, dark: true };
        // No flip → no new theme.
        assert!(refresh(&mut follow, &mut StubProbe(Some(true))).is_none());
        // Probe can't tell → keep what we have.
        assert!(refresh(&mut follow, &mut StubProbe(None)).is_none());
        // Flip → re-derive, state tracks.
        assert!(refresh(&mut follow, &mut StubProbe(Some(false))).is_some());
        assert!(matches!(follow, Follow::Ghostty { dark: false, .. }));
        // Fixed sources never refresh.
        assert!(refresh(&mut Follow::Fixed, &mut StubProbe(Some(false))).is_none());
    }
}
