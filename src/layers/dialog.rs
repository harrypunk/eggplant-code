//! A centered floating dialog layer.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::App;
use crate::compositor::{KeyResult, Layer};

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

/// Centered rect of `percent_x`/`percent_y` within `area`.
fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

impl Layer for Dialog {
    fn render(&self, frame: &mut Frame, area: Rect, _app: &App) {
        let area = centered(area, 50, 30);
        // Clear punches a hole through layers below; the dialog is opaque.
        frame.render_widget(Clear, area);
        let dialog = Paragraph::new(self.body.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.title))
                    .style(Style::default().bg(Color::Black)),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(dialog, area);
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            // Modal dialog: Esc or the toggle key closes it, everything else
            // is swallowed so it can't leak to layers below.
            KeyCode::Esc | KeyCode::Char('d') => KeyResult::Close,
            _ => KeyResult::Consumed,
        }
    }

    fn translucent(&self) -> bool {
        true
    }

    fn is_dialog(&self) -> bool {
        true
    }
}
