//! Notification toasts — rendered above all layers, never focused.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WIDTH: u16 = 40;
const HEIGHT: u16 = 3;
const MARGIN: u16 = 1;
const TTL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Error,
}

pub struct Notification {
    message: String,
    level: Level,
    created: Instant,
}

impl Notification {
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message, Level::Info)
    }

    #[allow(dead_code)]
    pub fn warn(message: impl Into<String>) -> Self {
        Self::new(message, Level::Warn)
    }

    #[allow(dead_code)]
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, Level::Error)
    }

    fn new(message: impl Into<String>, level: Level) -> Self {
        Self {
            message: message.into(),
            level,
            created: Instant::now(),
        }
    }

    fn is_visible(&self) -> bool {
        self.created.elapsed() < TTL
    }

    fn border_style(&self) -> Style {
        let color = match self.level {
            Level::Info => Color::Cyan,
            Level::Warn => Color::Yellow,
            Level::Error => Color::Red,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }
}

#[derive(Default)]
pub struct Notifications {
    items: Vec<Notification>,
}

impl Notifications {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, n: Notification) {
        self.items.push(n);
    }

    pub fn retain_visible(&mut self) {
        self.items.retain(Notification::is_visible);
    }

    /// Render stacked in the top-right corner, newest last (on top).
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        for (i, n) in self.items.iter().filter(|n| n.is_visible()).enumerate() {
            let y = area.y + MARGIN + i as u16 * HEIGHT;
            if y + HEIGHT > area.height {
                break;
            }
            let rect = Rect::new(
                area.x + area.width.saturating_sub(WIDTH + MARGIN),
                y,
                WIDTH.min(area.width),
                HEIGHT,
            );
            frame.render_widget(Clear, rect);
            let toast = Paragraph::new(n.message.as_str())
                .alignment(Alignment::Left)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(n.border_style())
                        .style(Style::default().bg(Color::Black)),
                );
            frame.render_widget(toast, rect);
        }
    }
}
