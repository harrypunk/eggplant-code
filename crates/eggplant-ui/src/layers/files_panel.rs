//! A docked file-explorer panel (toggle: `Ctrl-E`).
//!
//! `j`/`k` move, `Enter` opens a file (or descends a directory),
//! `Backspace`/`h` goes to the parent directory, `Esc` returns focus to the
//! editor. The panel stays open until toggled off.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};

use crate::app::App;
use crate::components::files_panel::{self, EntryProps, FilesPanelProps};
use crate::compositor::{KeyResult, Layer, LayerKind, Side};
use crate::element::Element;
use crate::layers::notification::Notification;

pub const PANEL_ID: &str = "files";
const WIDTH: u16 = 32;

struct Entry {
    name: String,
    is_dir: bool,
}

pub struct FilesPanel {
    dir: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    /// First visible row (scroll offset within the list).
    offset: usize,
    /// Inner list height from the compositor's `resize` hook; used to keep the
    /// selection visible. Updated outside the view path (Rule 5).
    inner_height: usize,
}

impl FilesPanel {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        Self::load(dir)
    }

    fn load(dir: PathBuf) -> io::Result<Self> {
        let mut entries: Vec<Entry> = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| Entry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: entry.file_type().is_ok_and(|t| t.is_dir()),
            })
            .collect();
        // Directories first, then case-insensitive name order.
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(Self {
            dir,
            entries,
            selected: 0,
            offset: 0,
            inner_height: 1,
        })
    }

    fn reload(&mut self, dir: PathBuf, app: &mut App) {
        match Self::load(dir) {
            Ok(loaded) => *self = loaded,
            Err(err) => app
                .notifications
                .push(Notification::error(format!("cannot read directory: {err}"))),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.ensure_selection_visible();
    }

    /// Scroll the window so the selected entry stays visible.
    fn ensure_selection_visible(&mut self) {
        let height = self.inner_height.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
    }

    fn selected_path(&self) -> Option<(PathBuf, bool)> {
        self.entries
            .get(self.selected)
            .map(|entry| (self.dir.join(&entry.name), entry.is_dir))
    }

    fn open_selected(&mut self, app: &mut App) -> KeyResult {
        let Some((path, is_dir)) = self.selected_path() else {
            return KeyResult::Consumed;
        };
        if is_dir {
            self.reload(path, app);
            return KeyResult::Consumed;
        }

        match app.editor.open_buffer(&path) {
            Ok(()) => {
                app.notifications.push(Notification::info(format!(
                    "opened {}",
                    app.editor.display_name()
                )));
                KeyResult::Unfocus
            }
            Err(err) => {
                app.notifications
                    .push(Notification::error(format!("open failed: {err:#}")));
                KeyResult::Consumed
            }
        }
    }
}

impl Layer for FilesPanel {
    fn view(&self, area: Rect, app: &App, focused: bool) -> Element {
        files_panel::view(
            &FilesPanelProps {
                title: self.dir.display().to_string(),
                entries: self
                    .entries
                    .iter()
                    .skip(self.offset)
                    .map(|entry| EntryProps {
                        name: entry.name.clone(),
                        is_dir: entry.is_dir,
                    })
                    .collect(),
                selected_in_view: self.selected - self.offset,
                focused,
            },
            area,
            &app.theme,
        )
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        match key.code {
            KeyCode::Esc => KeyResult::Unfocus,
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                KeyResult::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                KeyResult::Consumed
            }
            KeyCode::Enter => self.open_selected(app),
            KeyCode::Backspace | KeyCode::Char('h') => {
                if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
                    self.reload(parent, app);
                }
                KeyResult::Consumed
            }
            _ => KeyResult::Ignored,
        }
    }

    fn resize(&mut self, area: Rect, _app: &App) {
        let inner = Block::default().borders(Borders::ALL).inner(area);
        self.inner_height = inner.height as usize;
        self.ensure_selection_visible();
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Panel {
            side: Side::Left,
            size: WIDTH,
        }
    }

    fn id(&self) -> &'static str {
        PANEL_ID
    }
}
