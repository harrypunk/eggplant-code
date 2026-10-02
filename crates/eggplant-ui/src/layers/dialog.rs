//! Floating dialog layers: a generic message dialog and a yes/no confirm dialog.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::App;
use crate::compositor::{KeyResult, Layer, LayerKind};

/// Centered rect of `percent_x`/`percent_y` within `area`.
fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let [_, vertical, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);
    let [_, horizontal, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(vertical);
    horizontal
}

/// Render an opaque, centered floating box with a title.
fn render_float(frame: &mut Frame, area: Rect, title: &str, body: &str) {
    let area = centered(area, 50, 30);
    frame.render_widget(Clear, area);
    let dialog = Paragraph::new(body).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} "))
            .style(Style::default().bg(Color::Black)),
    );
    frame.render_widget(dialog.wrap(Wrap { trim: false }), area);
}

/// Simple modal message dialog (demo of the float layer kind).
pub struct Dialog {
    title: String,
    body: String,
}

impl Dialog {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
        }
    }
}

impl Layer for Dialog {
    fn render(&self, frame: &mut Frame, area: Rect, _app: &App, _focused: bool) {
        render_float(frame, area, &self.title, &self.body);
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            // Modal: Esc or the toggle key closes it, everything else is
            // swallowed so it can't leak to layers below.
            KeyCode::Esc | KeyCode::F(2) => KeyResult::Close,
            _ => KeyResult::Consumed,
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "dialog"
    }
}

/// Callback run when a `ConfirmDialog` is accepted.
type ConfirmAction = Box<dyn FnOnce(&mut App)>;

/// Modal yes/no confirmation; runs `on_confirm` when accepted.
pub struct ConfirmDialog {
    title: String,
    message: String,
    on_confirm: Option<ConfirmAction>,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl FnOnce(&mut App) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            on_confirm: Some(Box::new(on_confirm)),
        }
    }
}

impl Layer for ConfirmDialog {
    fn render(&self, frame: &mut Frame, area: Rect, _app: &App, _focused: bool) {
        let body = format!("{}\n\n[y] yes   [n] no", self.message);
        render_float(frame, area, &self.title, &body);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(on_confirm) = self.on_confirm.take() {
                    on_confirm(app);
                }
                KeyResult::Close
            }
            KeyCode::Char('n') | KeyCode::Esc => KeyResult::Close,
            _ => KeyResult::Consumed, // modal
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "confirm"
    }
}
