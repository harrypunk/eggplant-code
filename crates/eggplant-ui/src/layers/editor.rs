//! The base editor surface: document text + gutter + modal keys.
//! (The statusline is global chrome — see `crate::statusline`.)

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eggplant_core::Mode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::layers::notification::Notification;

#[derive(Default)]
pub struct EditorSurface {
    /// First visible line (vertical scroll offset).
    scroll: usize,
    /// Document generation we last saw; a change resets the scroll.
    seen_generation: usize,
    /// Last rendered editor height, so key handling can keep the cursor visible.
    viewport_height: Cell<usize>,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_cursor_visible(&mut self, app: &App) {
        // Document replaced? Drop per-document state.
        if app.editor.generation() != self.seen_generation {
            self.seen_generation = app.editor.generation();
            self.scroll = 0;
        }
        let cursor_line = app.editor.cursor().0;
        let height = self.viewport_height.get().max(1);
        if cursor_line < self.scroll {
            self.scroll = cursor_line;
        } else if cursor_line >= self.scroll + height {
            self.scroll = cursor_line + 1 - height;
        }
    }

    fn render_text_area(&self, frame: &mut Frame, area: Rect, app: &App) {
        let gutter_width = app.editor.line_count().max(1).ilog10() as u16 + 2;
        let [gutter_area, text_area] =
            Layout::horizontal([Constraint::Length(gutter_width), Constraint::Min(1)]).areas(area);

        let (cursor_line, cursor_col) = app.editor.cursor();
        let lines = app
            .editor
            .lines(self.scroll..self.scroll + area.height as usize);

        // Gutter: line numbers, current line emphasized.
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
        frame.render_widget(Paragraph::new(gutter), gutter_area);

        let text: Vec<Line> = lines.into_iter().map(Line::from).collect();
        frame.render_widget(Paragraph::new(text), text_area);

        // Terminal cursor tracks the editor cursor (char-col ≈ display col for now).
        let cursor_x = text_area.x + cursor_col as u16;
        let cursor_y = text_area.y + (cursor_line - self.scroll) as u16;
        if cursor_x < text_area.right() && cursor_y < text_area.bottom() {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // ---- key handling ----

    /// Plain character keys only — Ctrl/Alt combos fall through to globals.
    fn plain_char(key: &KeyEvent) -> Option<char> {
        match key.code {
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                Some(c)
            }
            _ => None,
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let editor = &mut app.editor;
        match (Self::plain_char(&key), key.code) {
            (Some('h'), _) | (_, KeyCode::Left) => editor.move_left(1),
            (Some('j'), _) | (_, KeyCode::Down) => editor.move_down(1),
            (Some('k'), _) | (_, KeyCode::Up) => editor.move_up(1),
            (Some('l'), _) | (_, KeyCode::Right) => editor.move_right(1),
            (Some('w'), _) => editor.move_word_forward(1),
            (Some('e'), _) => editor.move_word_end(1),
            (Some('b'), _) => editor.move_word_backward(1),
            (Some('0'), _) => editor.move_line_start(),
            (Some('$'), _) => editor.move_line_end(),
            (Some('G'), _) => editor.move_last_line(),
            (Some('x'), _) => editor.delete_char_at_cursor(),
            (Some('i'), _) => editor.enter_insert(),
            (Some('a'), _) => editor.enter_append(),
            (Some('o'), _) => {
                editor.open_line_below();
                editor.enter_insert();
            }
            (Some('O'), _) => {
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
            KeyCode::Delete => editor.delete_char_at_cursor(),
            KeyCode::Left => editor.move_left(1),
            KeyCode::Right => editor.move_right(1),
            KeyCode::Up => editor.move_up(1),
            KeyCode::Down => editor.move_down(1),
            KeyCode::Char(_) => match Self::plain_char(&key) {
                Some(c) => editor.insert_char(c),
                None => return KeyResult::Ignored,
            },
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
    fn render(&self, frame: &mut Frame, area: Rect, app: &App, _focused: bool) {
        self.viewport_height.set(area.height as usize);
        self.render_text_area(frame, area, app);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            return self.save(app);
        }

        let result = match app.editor.mode() {
            Mode::Normal => self.handle_normal_key(key, app),
            Mode::Insert => self.handle_insert_key(key, app),
        };
        self.ensure_cursor_visible(app);
        result
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Base
    }

    fn id(&self) -> &'static str {
        "editor"
    }
}
