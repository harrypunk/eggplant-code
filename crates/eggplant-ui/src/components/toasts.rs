//! Notification toasts — stacked top-right, above all layers.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use crate::element::Element;
use crate::layers::notification::Level;

const WIDTH: u16 = 40;
const HEIGHT: u16 = 3;
const MARGIN: u16 = 1;
/// How many toasts are shown at once; older ones collapse into a "+N more" row.
const MAX_VISIBLE: usize = 5;

/// One toast, projected for display.
pub struct ToastProps<'a> {
    pub message: &'a str,
    pub level: Level,
}

fn border_style(level: Level) -> Style {
    let color = match level {
        Level::Info => Color::Cyan,
        Level::Warn => Color::Yellow,
        Level::Error => Color::Red,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub fn view<'a>(toasts: &[ToastProps<'a>], area: Rect) -> Element<'a> {
    let hidden = toasts.len().saturating_sub(MAX_VISIBLE);
    let mut children: Vec<Element> = Vec::new();

    if hidden > 0 {
        children.push(Element::fixed(
            Rect::new(
                area.x + area.width.saturating_sub(WIDTH + MARGIN),
                area.y + MARGIN,
                WIDTH.min(area.width),
                1,
            ),
            Element::Text {
                lines: vec![Line::from(format!("+{hidden} earlier"))],
                style: Style::default().fg(Color::DarkGray),
                wrap: false,
            },
        ));
    }

    let toast_elements = toasts
        .iter()
        .skip(hidden)
        .enumerate()
        .filter(|(i, _)| {
            area.y + MARGIN + u16::from(hidden > 0) + *i as u16 * HEIGHT + HEIGHT <= area.bottom()
        })
        .map(|(i, toast)| {
            let rect = Rect::new(
                area.x + area.width.saturating_sub(WIDTH + MARGIN),
                area.y + MARGIN + u16::from(hidden > 0) + i as u16 * HEIGHT,
                WIDTH.min(area.width),
                HEIGHT,
            );
            Element::fixed(
                rect,
                Element::cleared(Element::Bordered {
                    title: None,
                    border_style: border_style(toast.level),
                    style: Style::default().bg(Color::Black),
                    child: Box::new(Element::text(vec![Line::from(toast.message)])),
                }),
            )
        });
    children.extend(toast_elements);

    Element::Stack(children)
}
