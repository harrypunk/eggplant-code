//! Notification toasts — the model (queue + expiry) lives here; the view is
//! the pure `components::toasts` function.

use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use crate::components::toasts::{self, ToastProps};
use crate::element::Element;

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

    pub fn warn(message: impl Into<String>) -> Self {
        Self::new(message, Level::Warn)
    }

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

    /// Project visible notifications into the pure toast component.
    pub fn view(&self, area: Rect) -> Element<'_> {
        let toasts: Vec<ToastProps> = self
            .items
            .iter()
            .filter(|n| n.is_visible())
            .map(|n| ToastProps {
                message: n.message.as_str(),
                level: n.level,
            })
            .collect();
        toasts::view(&toasts, area)
    }
}
