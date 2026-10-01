//! The base editor surface: document text + gutter + statusline + modal keys.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eggplant_core::Mode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::compositor::{KeyResult, Layer};
use crate::layers::notification::Notification;

#[derive(Default)]
pub struct EditorSurface {
    /// First visible line (vertical scroll offset).
    scroll: usize,
    /// Last rendered editor height, so key handling can keep the cursor visible.
    viewport_height: Cell<usize>,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_cursor_visible(&mut self, cursor_line: usize) {
        let height = self.viewport_height.get().max(1);
        if cursor_line < self.scroll {
            self.scroll = cursor_line;
        } else if cursor_line >= self.scroll + height {
            self.scroll = cursor_line + 1 - height;
        }
    }

    fn render_text_area(&self, frame: &mut Frame, area: Rect, app: &App) -> Rect {
        let gutter_width = app.editor.line_count().max(1).ilog10() as u16 + 2;
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(gutter_width), Constraint::Min(1)])
            .split(area);

        let (cursor_line, cursor_col) = app.editor.cursor();
        let lines = app
            .editor
            .lines(self.scroll..self.scroll + area.height as usize);

        // Gutter: line numbers, current line emphasized (declarative, one Line per row).
        let gutter: Vec<Line> = (self.scroll..self.scroll + lines.len())
            .map(|n| {
                let style = if n == cursor_line {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Line::from(Span::styled(
                    format!("{:>w$} ", n + 1, w = (gutter_width - 1) as usize),
                    style,
                ))
            })
            .collect();
        frame.render_widget(Paragraph::new(gutter), chunks[0]);

        let text: Vec<Line> = lines.into_iter().map(Line::from).collect();
        frame.render_widget(Paragraph::new(text), chunks[1]);

        // Terminal cursor tracks the editor cursor (char-col ≈ display col for now).
        let cursor_x = chunks[1].x + cursor_col as u16;
        let cursor_y = chunks[1].y + (cursor_line - self.scroll) as u16;
        if cursor_x < chunks[1].right() && cursor_y < chunks[1].bottom() {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
        chunks[1]
    }

    fn render_statusline(&self, frame: &mut Frame, area: Rect, app: &App) {
        let mode = app.editor.mode();
        let mode_bg = match mode {
            Mode::Normal => Color::Cyan,
            Mode::Insert => Color::Green,
        };
        let modified = if app.editor.is_modified() { " [+]" } else { "" };
        let (line, col) = app.editor.cursor();
        let right = format!(" {}:{}/{} ", line + 1, col + 1, app.editor.line_count());

        let status = Line::from(vec![
            Span::styled(
                format!(" {mode} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(mode_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" {}{modified}", app.editor.display_name())),
            Span::raw(" ".repeat((area.width as usize).saturating_sub(right.len() + 20))),
            Span::raw(right),
        ]);
        frame.render_widget(
            Paragraph::new(status).style(Style::default().bg(Color::DarkGray)),
            area,
        );
    }

    // ---- key handling ----

    fn handle_normal_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        use KeyCode::Char;

        let editor = &mut app.editor;
        match key.code {
            Char('h') | KeyCode::Left => editor.move_left(1),
            Char('j') | KeyCode::Down => editor.move_down(1),
            Char('k') | KeyCode::Up => editor.move_up(1),
            Char('l') | KeyCode::Right => editor.move_right(1),
            Char('w') => editor.move_word_forward(1),
            Char('e') => editor.move_word_end(1),
            Char('b') => editor.move_word_backward(1),
            Char('0') => editor.move_line_start(),
            Char('$') => editor.move_line_end(),
            Char('G') => editor.move_last_line(),
            Char('x') => editor.delete_char_at_cursor(),
            Char('i') => editor.enter_insert(),
            Char('a') => editor.enter_append(),
            Char('o') => {
                editor.open_line_below();
                editor.enter_insert();
            }
            Char('O') => {
                editor.open_line_above();
                editor.enter_insert();
            }
            _ => return KeyResult::Ignored,
        }
        KeyResult::Consumed
    }

    fn handle_insert_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let editor = &mut app.editor;
        match key.code {
            KeyCode::Esc => editor.enter_normal(),
            KeyCode::Enter => editor.insert_newline(),
            KeyCode::Backspace => editor.delete_backward(),
            KeyCode::Left => editor.move_left(1),
            KeyCode::Right => editor.move_right(1),
            KeyCode::Up => editor.move_up(1),
            KeyCode::Down => editor.move_down(1),
            KeyCode::Char(c) => editor.insert_char(c),
            _ => return KeyResult::Ignored,
        }
        KeyResult::Consumed
    }

    fn save(&mut self, app: &mut App) -> KeyResult {
        let notification = match app.editor.save() {
            Ok(()) => Notification::info(format!("wrote {}", app.editor.display_name())),
            Err(err) => Notification::error(format!("save failed: {err:#}")),
        };
        app.notifications.push(notification);
        KeyResult::Consumed
    }
}

impl Layer for EditorSurface {
    fn render(&self, frame: &mut Frame, area: Rect, app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        self.viewport_height.set(chunks[0].height as usize);
        self.render_text_area(frame, chunks[0], app);
        self.render_statusline(frame, chunks[1], app);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            return self.save(app);
        }

        let result = match app.editor.mode() {
            Mode::Normal => self.handle_normal_key(key, app),
            Mode::Insert => self.handle_insert_key(key, app),
        };
        self.ensure_cursor_visible(app.editor.cursor().0);
        result
    }
}
