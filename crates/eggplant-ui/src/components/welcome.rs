//! The welcome screen: shown by the editor surface when no buffer is open
//! (e.g. directory startup). ASCII logo + version + key hints, centered.
//!
//! Pure component: the logo is data, layout is computed from `area`.
//! Responsive: falls back to a plain title when the logo doesn't fit.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// ANSI-shadow block letters spelling EGGPLANT.
const LOGO: &str = "\
 ███████╗ ██████╗  ██████╗ ██████╗ ██╗      █████╗ ███╗   ██╗████████╗
 ██╔════╝██╔════╝ ██╔════╝ ██╔══██╗██║     ██╔══██╗████╗  ██║╚══██╔══╝
 █████╗  ██║  ███╗██║  ███╗██████╔╝██║     ███████║██╔██╗ ██║   ██║
 ██╔══╝  ██║   ██║██║   ██║██╔═══╝ ██║     ██╔══██║██║╚██╗██║   ██║
 ███████╗╚██████╔╝╚██████╔╝██║     ███████╗██║  ██║██║ ╚████║   ██║
 ╚══════╝ ╚═════╝  ╚═════╝ ╚═╝     ╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝   ╚═╝";

const LOGO_WIDTH: usize = 73;
const LOGO_HEIGHT: usize = 6;
/// Total block height: logo + blank + version + blank + hints.
const BLOCK_HEIGHT: u16 = (LOGO_HEIGHT + 4) as u16;

/// Everything the welcome screen needs — nothing more.
pub struct WelcomeProps {
    pub version: &'static str,
}

pub fn view(props: &WelcomeProps, area: Rect, theme: &Theme) -> Element {
    let accent = Style::default().fg(theme.accent);
    let muted = Style::default().fg(theme.comment);

    let fits = area.width >= LOGO_WIDTH as u16 && area.height >= BLOCK_HEIGHT + 2;
    let mut lines: Vec<Line> = Vec::new();
    if fits {
        lines.extend(
            LOGO.lines()
                .map(|l| Line::from(Span::styled(l.to_owned(), accent))),
        );
    } else {
        lines.push(Line::from(Span::styled(
            "eggplant-code",
            accent.add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("v{}", props.version),
        muted,
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "C-e explorer  ·  Space commands  ·  C-S-p palette",
        muted,
    )));

    // Center the block: width of the widest line, vertically centered.
    let width = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.len()).sum::<usize>())
        .max()
        .unwrap_or(0) as u16;
    let block = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(lines.len() as u16) / 2,
        width: width.min(area.width),
        height: lines.len() as u16,
    };

    Element::Stack(vec![
        // Paint the editor background so the screen isn't terminal-default.
        Element::Text {
            lines: vec![],
            style: Style::default().bg(theme.bg),
            wrap: false,
        },
        Element::fixed(block, Element::text(lines)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(props: &WelcomeProps, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                crate::element::paint(frame, view(props, area, &Theme::default()), area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    const PROPS: WelcomeProps = WelcomeProps { version: "0.0.0" };

    #[test]
    fn big_screen_shows_the_logo() {
        let out = render(&PROPS, 90, 24);
        assert!(out.contains('█'), "logo blocks visible:\n{out}");
        assert!(out.contains("v0.0.0"));
        assert!(out.contains("C-e explorer"));
    }

    #[test]
    fn small_screen_falls_back_to_plain_title() {
        let out = render(&PROPS, 40, 10);
        assert!(out.contains("eggplant-code"));
        assert!(!out.contains('█'), "no logo on small screens:\n{out}");
    }
}
