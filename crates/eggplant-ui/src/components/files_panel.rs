//! The file-explorer panel: titled border + entry list with selection.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::theme::Theme;

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

pub fn view(props: &FilesPanelProps, _area: Rect, theme: &Theme) -> Element {
    let border_style = if props.focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.comment)
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
                    .fg(theme.fg)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                Style::default().fg(theme.info)
            } else {
                Style::default().fg(theme.fg)
            };
            Line::from(Span::styled(label, style))
        })
        .collect();

    Element::cleared(Element::Bordered {
        title: Some(Line::from(format!(" {} ", props.title))),
        border_style,
        style: Style::default().fg(theme.fg).bg(theme.surface),
        child: Box::new(Element::text(rows)),
    })
}
