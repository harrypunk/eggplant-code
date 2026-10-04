//! Notification toasts — stacked top-right, above all layers.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::element::Element;
use crate::layers::notification::Level;
use crate::theme::Theme;

const WIDTH: u16 = 40;
const HEIGHT: u16 = 3;
const MARGIN: u16 = 1;
/// How many toasts are shown at once; older ones collapse into a "+N more" row.
const MAX_VISIBLE: usize = 5;

/// One toast, projected for display.
pub struct ToastProps {
    pub message: String,
    pub level: Level,
}

fn level_color(level: Level, theme: &Theme) -> ratatui::style::Color {
    match level {
        Level::Info => theme.info,
        Level::Warn => theme.warn,
        Level::Error => theme.error,
    }
}

pub fn view(toasts: &[ToastProps], area: Rect, theme: &Theme) -> Element {
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
                style: Style::default().fg(theme.comment),
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
                    border_style: Style::default()
                        .fg(level_color(toast.level, theme))
                        .add_modifier(Modifier::BOLD),
                    style: Style::default().fg(theme.fg).bg(theme.surface),
                    child: Box::new(Element::text(vec![Line::from(toast.message.clone())])),
                }),
            )
        });
    children.extend(toast_elements);

    Element::Stack(children)
}
