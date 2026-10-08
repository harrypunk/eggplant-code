//! The file-explorer panel: titled border + indented tree with selection.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// One visible tree row, projected for display.
pub struct RowProps {
    pub name: String,
    /// Nesting depth (0 = directly under the root).
    pub depth: usize,
    pub kind: RowKind,
}

pub enum RowKind {
    /// A directory; `expanded` drives the ▸/▾ marker.
    Dir {
        expanded: bool,
    },
    File,
}

/// Everything the panel view needs — nothing more.
pub struct FilesPanelProps {
    /// Panel title (the fixed root, display-formatted).
    pub title: String,
    /// Rows from the scroll offset onward (the widget clips the rest).
    pub rows: Vec<RowProps>,
    /// Selection index relative to the scrolled window.
    pub selected_in_view: usize,
    pub focused: bool,
}

pub fn view(props: &FilesPanelProps, _area: Rect, sheet: &Stylesheet) -> Element {
    let border_style = if props.focused {
        sheet.style(StyleClass::Accent)
    } else {
        sheet.style(StyleClass::Muted)
    };

    let rows: Vec<Line> = props
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let marker = match row.kind {
                RowKind::Dir { expanded: true } => "▾ ",
                RowKind::Dir { expanded: false } => "▸ ",
                RowKind::File => "  ",
            };
            let label = format!("{}{}{}", "  ".repeat(row.depth), marker, row.name);
            let style = if i == props.selected_in_view {
                sheet.style(StyleClass::SelectedStrong)
            } else if matches!(row.kind, RowKind::Dir { .. }) {
                sheet.style(StyleClass::Info)
            } else {
                sheet.style(StyleClass::Text)
            };
            Line::from(Span::styled(label, style))
        })
        .collect();

    Element::cleared(Element::Bordered {
        title: Some(Line::from(format!(" {} ", props.title))),
        border_style,
        style: sheet.style(StyleClass::Surface),
        child: Box::new(Element::text(rows)),
    })
}
