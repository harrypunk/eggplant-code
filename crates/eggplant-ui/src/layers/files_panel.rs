//! A docked file-explorer panel (toggle: `Ctrl-E`) showing the directory
//! tree under a **fixed root** — the root never changes.
//!
//! `j`/`k` move, `l` expands the directory under the cursor, `h` collapses
//! it (or jumps to the parent row), `Enter` toggles a directory or opens a
//! file, `Esc` returns focus to the editor. Directory listings are cached
//! lazily on expand; a refresh command is planned with the file-operation
//! commands (create/rename/delete).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};

use crate::app::App;
use crate::components::files_panel::{self, FilesPanelProps, RowKind, RowProps};
use crate::compositor::{KeyResult, Layer, LayerKind, Side};
use crate::element::Element;
use crate::layers::notification::Notification;

pub const PANEL_ID: &str = "files";
const WIDTH: u16 = 32;

struct Entry {
    name: String,
    is_dir: bool,
}

/// One visible row in the flattened tree.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    path: PathBuf,
    name: String,
    depth: usize,
    is_dir: bool,
    expanded: bool,
}

pub struct FilesPanel {
    /// Fixed tree root — never changes for the panel's lifetime.
    root: PathBuf,
    /// Directories whose children are visible (the root is always expanded).
    expanded: HashSet<PathBuf>,
    /// Lazily loaded directory listings, filled on expand.
    cache: HashMap<PathBuf, Vec<Entry>>,
    selected: usize,
    /// First visible row (scroll offset within the list).
    offset: usize,
    /// Inner list height from the compositor's `resize` hook (Rule 5).
    inner_height: usize,
}

/// A directory's children: directories first, then case-insensitive names.
fn read_children(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: entry.file_type().is_ok_and(|t| t.is_dir()),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

impl FilesPanel {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        let mut cache = HashMap::new();
        cache.insert(root.clone(), read_children(&root)?);
        Ok(Self {
            expanded: HashSet::from([root.clone()]),
            root,
            cache,
            selected: 0,
            offset: 0,
            inner_height: 1,
        })
    }

    /// The flattened, currently-visible tree (derived from `expanded`).
    fn rows(&self) -> Vec<Row> {
        fn walk(panel: &FilesPanel, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
            let Some(children) = panel.cache.get(dir) else {
                return;
            };
            for child in children {
                let path = dir.join(&child.name);
                let expanded = panel.expanded.contains(&path);
                rows.push(Row {
                    name: child.name.clone(),
                    path: path.clone(),
                    depth,
                    is_dir: child.is_dir,
                    expanded,
                });
                if child.is_dir && expanded {
                    walk(panel, &path, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, &self.root.clone(), 0, &mut rows);
        rows
    }

    fn selected_row(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.selected)
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.rows().len();
        if len == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        self.ensure_selection_visible();
    }

    /// Scroll the window so the selected row stays visible.
    fn ensure_selection_visible(&mut self) {
        let height = self.inner_height.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
    }

    /// Expand the selected directory. Errors (permissions, races) surface as
    /// `Err`; the caller notifies.
    fn expand_selected(&mut self) -> io::Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        if !row.is_dir || row.expanded {
            return Ok(());
        }
        if !self.cache.contains_key(&row.path) {
            self.cache
                .insert(row.path.clone(), read_children(&row.path)?);
        }
        self.expanded.insert(row.path);
        Ok(())
    }

    /// `l`: expand a collapsed directory; descend into an expanded one.
    fn expand_or_descend(&mut self) -> io::Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        if !row.is_dir {
            return Ok(());
        }
        if row.expanded {
            // First child is the next row (if the directory is non-empty).
            let len = self.rows().len();
            if self.selected + 1 < len {
                self.move_selection(1);
            }
            Ok(())
        } else {
            self.expand_selected()
        }
    }

    /// `h`: collapse an expanded directory; otherwise jump to the parent row.
    fn collapse_or_parent(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.is_dir && row.expanded {
            self.expanded.remove(&row.path);
            return;
        }
        // Select the parent directory's row (it's visible: this row is).
        if let Some(parent) = row.path.parent()
            && parent.starts_with(&self.root)
            && parent != row.path
            && let Some(index) = self.rows().iter().position(|r| r.path == parent)
        {
            self.selected = index;
            self.ensure_selection_visible();
        }
    }

    /// `Enter` on a directory: toggle expanded/collapsed.
    fn toggle_selected_dir(&mut self) -> io::Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        if row.expanded {
            self.expanded.remove(&row.path);
            Ok(())
        } else {
            self.expand_selected()
        }
    }

    fn open_selected(&mut self, app: &mut App) -> KeyResult {
        let Some(row) = self.selected_row() else {
            return KeyResult::Consumed;
        };
        if row.is_dir {
            if let Err(err) = self.toggle_selected_dir() {
                app.notifications
                    .push(Notification::error(format!("cannot read directory: {err}")));
            }
            return KeyResult::Consumed;
        }
        match app.editor.open_buffer(&row.path) {
            Ok(()) => {
                app.notifications.push(Notification::info(format!(
                    "opened {}",
                    app.editor.display_name().unwrap_or_default()
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
                title: self.root.display().to_string(),
                rows: self
                    .rows()
                    .into_iter()
                    .skip(self.offset)
                    .map(|row| RowProps {
                        name: row.name,
                        depth: row.depth,
                        kind: if row.is_dir {
                            RowKind::Dir {
                                expanded: row.expanded,
                            }
                        } else {
                            RowKind::File
                        },
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
            KeyCode::Char('l') | KeyCode::Right => {
                if let Err(err) = self.expand_or_descend() {
                    app.notifications
                        .push(Notification::error(format!("cannot read directory: {err}")));
                }
                KeyResult::Consumed
            }
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                self.collapse_or_parent();
                KeyResult::Consumed
            }
            KeyCode::Enter => self.open_selected(app),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// root/
    ///   a_dir/{a1.txt, a2.txt}
    ///   b.txt
    ///   z_dir/
    fn test_tree() -> (PathBuf, FilesPanel) {
        // Unique per test: the suite runs in parallel in one process.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("eggplant-tree-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a_dir")).unwrap();
        fs::create_dir_all(root.join("z_dir")).unwrap();
        fs::write(root.join("a_dir/a1.txt"), "1").unwrap();
        fs::write(root.join("a_dir/a2.txt"), "2").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        let panel = FilesPanel::new(root.clone()).unwrap();
        (root, panel)
    }

    fn names(panel: &FilesPanel) -> Vec<String> {
        panel
            .rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    #[test]
    fn initial_rows_are_top_level_dirs_first() {
        let (root, panel) = test_tree();
        assert_eq!(names(&panel), ["a_dir", "z_dir", "b.txt"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expand_reveals_children_and_collapse_hides_them() {
        let (root, mut panel) = test_tree();
        panel.expand_selected().unwrap(); // a_dir selected (row 0)
        assert_eq!(
            names(&panel),
            ["a_dir", "  a1.txt", "  a2.txt", "z_dir", "b.txt"]
        );

        panel.collapse_or_parent(); // h on expanded dir collapses it
        assert_eq!(names(&panel), ["a_dir", "z_dir", "b.txt"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn h_on_file_selects_parent_dir_row() {
        let (root, mut panel) = test_tree();
        panel.expand_selected().unwrap(); // expand a_dir
        panel.move_selection(1); // select a1.txt
        panel.collapse_or_parent();
        assert_eq!(panel.selected, 0); // back on a_dir, still expanded
        assert!(panel.rows()[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn l_expands_then_descends() {
        let (root, mut panel) = test_tree();
        panel.expand_or_descend().unwrap(); // expand a_dir
        assert!(panel.rows()[0].expanded);
        assert_eq!(panel.selected, 0);
        panel.expand_or_descend().unwrap(); // descend to first child
        assert_eq!(panel.selected, 1);
        assert_eq!(panel.rows()[1].name, "a1.txt");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enter_toggles_directories() {
        let (root, mut panel) = test_tree();
        panel.toggle_selected_dir().unwrap();
        assert!(panel.rows()[0].expanded);
        panel.toggle_selected_dir().unwrap();
        assert!(!panel.rows()[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn root_is_never_left() {
        let (root, mut panel) = test_tree();
        // h on a top-level row: parent is the root itself — stay put.
        panel.collapse_or_parent();
        assert_eq!(panel.selected, 0);
        assert_eq!(names(&panel), ["a_dir", "z_dir", "b.txt"]);
        fs::remove_dir_all(root).unwrap();
    }
}
