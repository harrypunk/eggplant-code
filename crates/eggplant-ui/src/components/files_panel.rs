//! The file-explorer panel: titled border + entry list with selection.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;

/// One panel entry, projected for display.
pub struct EntryProps {
    pub name: String,
    pub is_dir: bool,
}

/// Everything the panel view needs — nothing more.
pub struct FilesPanelProps {
    /// Panel title (current directory, display-formatted).
    pub title: String,
    /// Entries from the scroll offset onward (the widget clips the rest).
    pub entries: Vec<EntryProps>,
    /// Selection index, relative to the *full* entry list (the view is
    /// already offset, so rows before it are skipped by the container).
    pub selected_in_view: usize,
    pub focused: bool,
}

pub fn view(props: &FilesPanelProps, _area: Rect) -> Element<'static> {
    let border_style = if props.focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let rows: Vec<Line> = props
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let label = if entry.is_dir {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            let style = if i == props.selected_in_view {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            Line::from(Span::styled(label, style))
        })
        .collect();

    Element::cleared(Element::bordered(
        format!(" {} ", props.title),
        border_style,
        Element::text(rows),
    ))
}
