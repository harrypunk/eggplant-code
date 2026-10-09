//! The stylesheet: the ONE place class → color is decided
//! (docs/design/state-flow.md — the HTML/CSS separation).
//!
//! Components describe *what* a thing is (`StyleClass::SearchMatch`),
//! never *which color* it has. They receive a `Stylesheet` whose theme
//! field is private: naming a concrete color from a component is a
//! compile error, not a convention. Swapping themes swaps this mapping;
//! components don't change.
//!
//! Editor text cells are compositional (syntax color × selection/search
//! background); they compose via `style()` + `bg()` — still the
//! stylesheet's values, only the precedence lives in the component
//! (that's layout logic, like CSS specificity).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use eggplant_core::SyntaxScope;

use crate::theme::Theme;

/// The semantic style vocabulary — the ONLY thing components may say
/// about appearance. Adding a variant is a design decision, made here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleClass {
    // ---- text roles ----
    /// Plain text on the default background.
    Text,
    /// De-emphasized: gutter numbers, hints, descriptions, v-rules.
    Muted,
    /// Brand accent: borders, prompt labels, directory markers.
    Accent,
    /// Secondary accent: focused line number, key hints, focus tags.
    AccentAlt,
    /// Accent + bold: titles, headers.
    Title,
    /// Inverted bold chip (leap labels).
    Chip,
    /// Inline code / code blocks: text on the raised surface.
    Code,

    // ---- surfaces ----
    /// Body text on a raised surface (panels, floats, dialogs).
    Surface,
    /// Text on the statusline bar.
    Bar,
    /// Muted text on the statusline bar (inactive tabs).
    MutedOnBar,

    // ---- state ----
    /// Focused row / visual selection: fg on the selection background.
    Selected,
    /// The bold variant (selected row's primary column, active tab).
    SelectedStrong,

    // ---- mode badges (chips: colored background) ----
    ModeNormal,
    ModeInsert,
    ModeVisual,

    // ---- search ----
    SearchMatch,
    SearchCurrent,

    // ---- feedback ----
    Info,
    Warn,
    Error,

    // ---- syntax ----
    /// Syntax-highlighted text (`None` = unscoped plain text).
    Syntax(Option<SyntaxScope>),
    /// Leap-dimmed variant: the buffer fades while leap labels show.
    SyntaxDim(Option<SyntaxScope>),
}

/// A component's handle on styling: class → concrete style. The theme is
/// private — components cannot reach past the vocabulary.
pub struct Stylesheet<'a> {
    theme: &'a Theme,
}

impl<'a> Stylesheet<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    /// The concrete style for a class — the entire class→color mapping.
    pub fn style(&self, class: StyleClass) -> Style {
        let theme = self.theme;
        match class {
            StyleClass::Text => Style::default().fg(theme.fg).bg(theme.bg),
            StyleClass::Muted => Style::default().fg(theme.comment),
            StyleClass::Accent => Style::default().fg(theme.accent),
            StyleClass::AccentAlt => Style::default().fg(theme.accent_alt),
            StyleClass::Title => Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
            StyleClass::Chip => Style::default()
                .fg(theme.bg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),

            StyleClass::Surface => Style::default().fg(theme.fg).bg(theme.surface),
            StyleClass::Code => Style::default().fg(theme.fg).bg(theme.surface),
            StyleClass::Bar => Style::default().fg(theme.fg).bg(theme.statusline),
            StyleClass::MutedOnBar => Style::default().fg(theme.comment).bg(theme.statusline),

            StyleClass::Selected => Style::default().fg(theme.fg).bg(theme.selection),
            StyleClass::SelectedStrong => Style::default()
                .fg(theme.fg)
                .bg(theme.selection)
                .add_modifier(Modifier::BOLD),

            StyleClass::ModeNormal => Style::default()
                .fg(theme.bg)
                .bg(theme.mode_normal)
                .add_modifier(Modifier::BOLD),
            StyleClass::ModeInsert => Style::default()
                .fg(theme.bg)
                .bg(theme.mode_insert)
                .add_modifier(Modifier::BOLD),
            StyleClass::ModeVisual => Style::default()
                .fg(theme.bg)
                .bg(theme.accent_alt)
                .add_modifier(Modifier::BOLD),

            StyleClass::SearchMatch => Style::default().bg(theme.search_match),
            StyleClass::SearchCurrent => Style::default().bg(theme.search_current),

            StyleClass::Info => Style::default().fg(theme.info),
            StyleClass::Warn => Style::default().fg(theme.warn),
            StyleClass::Error => Style::default().fg(theme.error),

            StyleClass::Syntax(scope) => theme.scope_style(scope),
            StyleClass::SyntaxDim(scope) => theme.scope_style(scope).fg(theme.comment),
        }
    }

    /// A class's background color — for compositing overlays (the editor's
    /// selection/search cells). The value still comes only from here.
    pub fn bg(&self, class: StyleClass) -> Option<Color> {
        self.style(class).bg
    }

    /// A background-only fill (blank surface areas).
    pub fn fill(&self, class: StyleClass) -> Style {
        Style::default().bg(self.bg(class).unwrap_or(Color::Reset))
    }

    /// The bold variant of a class (typography is the stylesheet's job
    /// too — components never touch modifiers).
    pub fn emphasized(&self, class: StyleClass) -> Style {
        self.style(class).add_modifier(Modifier::BOLD)
    }

    /// One styled span.
    pub fn span(&self, class: StyleClass, text: impl Into<String>) -> Span<'static> {
        Span::styled(text.into(), self.style(class))
    }

    /// A line of class-tagged spans.
    pub fn line(&self, spans: Vec<(StyleClass, String)>) -> Line<'static> {
        Line::from(
            spans
                .into_iter()
                .map(|(class, text)| self.span(class, text))
                .collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    /// Every class resolves, under both builtin themes — no unreachable
    /// color path exists (the "all variants" storybook page, as a test).
    #[test]
    fn every_class_resolves_under_every_theme() {
        let classes = [
            StyleClass::Text,
            StyleClass::Muted,
            StyleClass::Accent,
            StyleClass::AccentAlt,
            StyleClass::Title,
            StyleClass::Chip,
            StyleClass::Surface,
            StyleClass::Bar,
            StyleClass::MutedOnBar,
            StyleClass::Selected,
            StyleClass::SelectedStrong,
            StyleClass::ModeNormal,
            StyleClass::ModeInsert,
            StyleClass::ModeVisual,
            StyleClass::SearchMatch,
            StyleClass::SearchCurrent,
            StyleClass::Info,
            StyleClass::Warn,
            StyleClass::Error,
            StyleClass::Syntax(None),
            StyleClass::Syntax(Some(SyntaxScope::Keyword)),
            StyleClass::SyntaxDim(None),
        ];
        for name in ["tokyo-night", "classic"] {
            let theme = Theme::by_name(name).unwrap();
            let sheet = Stylesheet::new(&theme);
            for class in classes {
                let _ = sheet.style(class);
                let _ = sheet.span(class, "x");
            }
        }
    }

    #[test]
    fn the_mapping_itself() {
        let theme = Theme::default();
        let sheet = Stylesheet::new(&theme);
        assert_eq!(sheet.style(StyleClass::Muted).fg, Some(theme.comment));
        assert_eq!(sheet.style(StyleClass::Selected).bg, Some(theme.selection));
        assert!(
            sheet
                .style(StyleClass::SelectedStrong)
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(
            sheet.style(StyleClass::Chip).fg,
            Some(theme.bg),
            "chips are inverted"
        );
        assert_eq!(
            sheet
                .style(StyleClass::SyntaxDim(Some(SyntaxScope::String)))
                .fg,
            Some(theme.comment),
            "dim overrides the syntax color"
        );
    }
}
