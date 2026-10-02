//! The `:` command line — a one-line input at the bottom of the screen.
//!
//! `Enter` runs the input as an ex command (see `crate::ex_commands`),
//! `Esc` cancels. Append-only editing (chars + backspace) for now.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::App;
use crate::compositor::{KeyResult, Layer, LayerKind};

#[derive(Default)]
pub struct CommandLine {
    input: String,
}

impl CommandLine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Layer for CommandLine {
    fn render(&self, frame: &mut Frame, area: Rect, _app: &App, _focused: bool) {
        // One line at the very bottom of the body area.
        let line = Rect {
            height: 1,
            y: area.bottom().saturating_sub(1),
            ..area
        };
        frame.render_widget(Clear, line);

        let prompt = Line::from(vec![
            Span::styled(":", Style::default().fg(Color::Cyan)),
            Span::raw(&self.input),
        ]);
        frame.render_widget(Paragraph::new(prompt), line);
        frame.set_cursor_position((line.x + 1 + self.input.len() as u16, line.y));
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Esc => KeyResult::Close,
            KeyCode::Enter => KeyResult::RunEx(std::mem::take(&mut self.input)),
            KeyCode::Backspace => {
                self.input.pop();
                KeyResult::Consumed
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed, // modal-ish: swallow everything else
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "command-line"
    }
}
