//! The base editor surface — placeholder text area for M0.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::compositor::{KeyResult, Layer};

pub struct EditorSurface {
    lines: Vec<String>,
}

impl EditorSurface {
    pub fn new() -> Self {
        Self {
            lines: vec![
                "// eggplant-code M0".into(),
                "".into(),
                "This is the editor surface (base layer).".into(),
                "Real helix-core document rendering lands in M1.".into(),
                "".into(),
                "keys: `d` dialog  `n` notification  `q` quit".into(),
            ],
        }
    }
}

impl Layer for EditorSurface {
    fn render(&self, frame: &mut Frame, area: Rect, _app: &App) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let text: Vec<Line> = self.lines.iter().map(|l| Line::from(l.as_str())).collect();
        let editor = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" eggplant-code "),
        );
        frame.render_widget(editor, chunks[0]);

        let status = Paragraph::new(Line::from(vec![
            ratatui::text::Span::styled(
                " NORMAL ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(" M0 skeleton — d: dialog  n: notify  q: quit"),
        ]))
        .style(Style::default().bg(Color::DarkGray));
        frame.render_widget(status, chunks[1]);
    }

    fn handle_key(&mut self, _key: KeyEvent, _app: &mut App) -> KeyResult {
        // Base surface consumes nothing in M0; global handler owns keys.
        KeyResult::Ignored
    }
}
