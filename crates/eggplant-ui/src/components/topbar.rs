//! The buffer topbar: one tab per open buffer (vscode/neovim-bufferline
//! style). Current buffer highlighted, modified buffers marked.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

/// One buffer tab, projected for display.
pub struct BufferTab {
    pub name: String,
    pub modified: bool,
    pub current: bool,
}

pub fn view(tabs: &[BufferTab], _area: Rect, theme: &Theme) -> Element {
    let bar = Style::default().fg(theme.comment).bg(theme.statusline);
    let current_style = Style::default()
        .fg(theme.fg)
        .bg(theme.selection)
        .add_modifier(Modifier::BOLD);

    let mut spans: Vec<Span> = Vec::new();
    for tab in tabs {
        let marker = if tab.modified { " ●" } else { "" };
        let label = format!(" {}{} ", tab.name, marker);
        spans.push(Span::styled(
            label,
            if tab.current { current_style } else { bar },
        ));
        spans.push(Span::styled(" ", bar)); // gap between tabs
    }

    Element::Text {
        lines: vec![Line::from(spans)],
        style: bar,
        wrap: false,
    }
}
