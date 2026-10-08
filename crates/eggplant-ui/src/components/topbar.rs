//! The buffer topbar: one tab per open buffer (vscode/neovim-bufferline
//! style). Current buffer highlighted, modified buffers marked.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// One buffer tab, projected for display.
pub struct BufferTab {
    pub name: String,
    pub modified: bool,
    pub current: bool,
}

pub fn view(tabs: &[BufferTab], _area: Rect, sheet: &Stylesheet) -> Element {
    let bar = sheet.style(StyleClass::MutedOnBar);
    let current_style = sheet.style(StyleClass::SelectedStrong);

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
