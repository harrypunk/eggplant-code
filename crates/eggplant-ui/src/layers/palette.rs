//! The command palette (`Space` in normal mode) — fuzzy search over the
//! command registry. `Enter` runs the selected command, `Esc` closes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::commands::Command;
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::fuzzy;

const MAX_ROWS: usize = 8;

pub struct Palette {
    input: String,
    /// Snapshot of the registry's commands (palette executes by value).
    commands: Vec<Command>,
    selected: usize,
}

impl Palette {
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            input: String::new(),
            commands,
            selected: 0,
        }
    }

    /// Commands matching the current input, best first, capped for display.
    fn filtered(&self) -> Vec<Command> {
        fuzzy::filter(&self.input, &self.commands, |c| c.id)
            .into_iter()
            .take(MAX_ROWS)
            .map(|(_, command)| *command)
            .collect()
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.filtered().len();
        if count == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
    }
}

impl Layer for Palette {
    fn render(&self, frame: &mut Frame, area: Rect, _app: &App, _focused: bool) {
        // Centered horizontally, hugging the top of the body area.
        let width = (area.width * 3 / 5).max(30).min(area.width);
        let height = (MAX_ROWS as u16 + 3).min(area.height); // input + rows + borders
        let rect = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + 1,
            width,
            height,
        };
        frame.render_widget(Clear, rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" palette ")
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let [input_area, list_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);

        let input = Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(&self.input),
        ]);
        frame.render_widget(Paragraph::new(input), input_area);
        frame.set_cursor_position((input_area.x + 2 + self.input.len() as u16, input_area.y));

        let rows: Vec<Line> = self
            .filtered()
            .into_iter()
            .enumerate()
            .map(|(i, command)| {
                let style = if i == self.selected {
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(format!(" {:<20}", command.id), style),
                    Span::styled(command.description, style.fg(Color::Gray)),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(rows), list_area);
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Esc => KeyResult::Close,
            KeyCode::Enter => match self.filtered().get(self.selected) {
                Some(command) => KeyResult::Execute(*command),
                None => KeyResult::Close,
            },
            KeyCode::Up => {
                self.move_selection(-1);
                KeyResult::Consumed
            }
            KeyCode::Down => {
                self.move_selection(1);
                KeyResult::Consumed
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.selected = 0;
                KeyResult::Consumed
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                self.selected = 0;
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed, // modal-ish
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "palette"
    }
}
