//! A docked file-explorer panel (toggle: `Ctrl-E`) over a [`FileTree`].
//!
//! The panel is list navigation over the tree's derived rows: selection,
//! scrolling, and key handling live here; tree state (expansion, listing
//! cache, filtering) lives in [`FileTree`].
//!
//! `j`/`k` move, `l` expands the directory under the cursor, `h` collapses
//! it (or jumps to the parent row), `Enter` toggles a directory or opens a
//! file, `I` toggles the full/filtered listing, `Esc` returns focus to the
//! editor. A refresh command is planned with the file-operation commands
//! (create/rename/delete).

use std::io;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};

use crate::app::App;
use crate::commands::KeyStroke;
use crate::components::files_panel::{self, FilesPanelProps, RowKind, RowProps};
use crate::compositor::{KeyResult, Layer, LayerKind, Side};
use crate::element::Element;
use crate::filetree::{FileTree, TreeRow, fs_lister};
use crate::layers::notification::Notification;

pub const PANEL_ID: &str = "files";
const WIDTH: u16 = 32;

/// The explorer's closed action set — keys are data (see
/// `crate::keymaps`), config-overridable via `[keys.explorer]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerAction {
    MoveDown,
    MoveUp,
    ExpandOrDescend,
    CollapseOrParent,
    ToggleAll,
    Open,
    Unfocus,
}

impl ExplorerAction {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "down" => Self::MoveDown,
            "up" => Self::MoveUp,
            "expand" => Self::ExpandOrDescend,
            "collapse" => Self::CollapseOrParent,
            "toggle-all" => Self::ToggleAll,
            "open" => Self::Open,
            "unfocus" => Self::Unfocus,
            _ => return None,
        })
    }
}

/// Default explorer bindings.
pub const DEFAULT_KEYS: &[(KeyStroke, ExplorerAction)] = &[
    (KeyStroke::char('j'), ExplorerAction::MoveDown),
    (
        KeyStroke::new(KeyCode::Down, KeyModifiers::NONE),
        ExplorerAction::MoveDown,
    ),
    (KeyStroke::char('k'), ExplorerAction::MoveUp),
    (
        KeyStroke::new(KeyCode::Up, KeyModifiers::NONE),
        ExplorerAction::MoveUp,
    ),
    (KeyStroke::char('l'), ExplorerAction::ExpandOrDescend),
    (
        KeyStroke::new(KeyCode::Right, KeyModifiers::NONE),
        ExplorerAction::ExpandOrDescend,
    ),
    (KeyStroke::char('h'), ExplorerAction::CollapseOrParent),
    (
        KeyStroke::new(KeyCode::Left, KeyModifiers::NONE),
        ExplorerAction::CollapseOrParent,
    ),
    (
        KeyStroke::new(KeyCode::Backspace, KeyModifiers::NONE),
        ExplorerAction::CollapseOrParent,
    ),
    (KeyStroke::char('I'), ExplorerAction::ToggleAll),
    (
        KeyStroke::new(KeyCode::Enter, KeyModifiers::NONE),
        ExplorerAction::Open,
    ),
    (
        KeyStroke::new(KeyCode::Esc, KeyModifiers::NONE),
        ExplorerAction::Unfocus,
    ),
];

pub struct FilesPanel {
    tree: FileTree,
    /// `false`: ignore rules + dotfiles are filtered out (default).
    /// `true` (`I` toggle): the full, unfiltered listing.
    show_all: bool,
    selected: usize,
    /// First visible row (scroll offset within the list).
    offset: usize,
    /// Inner list height from the compositor's `resize` hook (Rule 5).
    inner_height: usize,
}

impl FilesPanel {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        Ok(Self {
            tree: FileTree::new(root, fs_lister)?,
            show_all: false,
            selected: 0,
            offset: 0,
            inner_height: 1,
        })
    }

    fn rows(&self, app: &App) -> Vec<TreeRow> {
        self.tree.rows(&app.workspace.ignores, self.show_all)
    }

    fn selected_row(&self, app: &App) -> Option<TreeRow> {
        self.rows(app).into_iter().nth(self.selected)
    }

    fn move_selection(&mut self, delta: isize, app: &App) {
        let len = self.rows(app).len();
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

    /// `I`: toggle the full / filtered listing.
    fn toggle_show_all(&mut self, app: &App) -> bool {
        self.show_all = !self.show_all;
        // Selection may point past the end of the now-shorter list.
        let len = self.rows(app).len();
        self.selected = if len > 0 {
            self.selected.min(len - 1)
        } else {
            0
        };
        self.ensure_selection_visible();
        self.show_all
    }

    /// `l`: expand a collapsed directory; descend into an expanded one.
    fn expand_or_descend(&mut self, app: &App) -> io::Result<()> {
        let Some(row) = self.selected_row(app) else {
            return Ok(());
        };
        if !row.is_dir {
            return Ok(());
        }
        if row.expanded {
            // First child is the next row (if the directory is non-empty).
            let len = self.rows(app).len();
            if self.selected + 1 < len {
                self.move_selection(1, app);
            }
            Ok(())
        } else {
            self.tree.expand(&row.path)
        }
    }

    /// `h`: collapse an expanded directory; otherwise jump to the parent row.
    fn collapse_or_parent(&mut self, app: &App) {
        let Some(row) = self.selected_row(app) else {
            return;
        };
        if row.is_dir && row.expanded {
            self.tree.collapse(&row.path);
            return;
        }
        // Select the parent directory's row (it's visible: this row is).
        if let Some(parent) = row.path.parent()
            && parent.starts_with(self.tree.root())
            && parent != row.path
            && let Some(index) = self.rows(app).iter().position(|r| r.path == parent)
        {
            self.selected = index;
            self.ensure_selection_visible();
        }
    }

    fn open_selected(&mut self, app: &mut App) -> KeyResult {
        let Some(row) = self.selected_row(app) else {
            return KeyResult::Consumed;
        };
        if row.is_dir {
            if let Err(err) = self.tree.toggle(&row.path) {
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
                title: self.tree.root().display().to_string(),
                rows: self
                    .rows(app)
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
        // Keys are data: exact-modifier lookup means Ctrl/Alt keys (C-l,
        // C-h…) never match plain-letter bindings and fall through to the
        // global keymap on their own.
        let Some(action) = crate::editing::lookup(&app.layer_keys.explorer, &key) else {
            return KeyResult::Ignored;
        };
        match action {
            ExplorerAction::MoveDown => {
                self.move_selection(1, app);
                KeyResult::Consumed
            }
            ExplorerAction::MoveUp => {
                self.move_selection(-1, app);
                KeyResult::Consumed
            }
            ExplorerAction::ExpandOrDescend => {
                if let Err(err) = self.expand_or_descend(app) {
                    app.notifications
                        .push(Notification::error(format!("cannot read directory: {err}")));
                }
                KeyResult::Consumed
            }
            ExplorerAction::CollapseOrParent => {
                self.collapse_or_parent(app);
                KeyResult::Consumed
            }
            ExplorerAction::ToggleAll => {
                let show_all = self.toggle_show_all(app);
                app.notifications.push(Notification::info(if show_all {
                    "explorer: showing all files"
                } else {
                    "explorer: filtered"
                }));
                KeyResult::Consumed
            }
            ExplorerAction::Open => self.open_selected(app),
            ExplorerAction::Unfocus => KeyResult::Unfocus,
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
    use std::fs;

    /// root/
    ///   a_dir/{a1.txt, a2.txt}
    ///   b.txt
    ///   z_dir/
    /// (Row/expansion policy is tested in `filetree`; these cover the
    /// panel's own navigation over the rows.)
    fn test_tree() -> (PathBuf, App, FilesPanel) {
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
        let app = App::new(eggplant_core::Editor::scratch().unwrap());
        (root, app, panel)
    }

    #[test]
    fn l_expands_then_descends() {
        let (root, app, mut panel) = test_tree();
        panel.expand_or_descend(&app).unwrap(); // expand a_dir
        assert!(panel.rows(&app)[0].expanded);
        assert_eq!(panel.selected, 0);
        panel.expand_or_descend(&app).unwrap(); // descend to first child
        assert_eq!(panel.selected, 1);
        assert_eq!(panel.rows(&app)[1].name, "a1.txt");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn h_on_file_selects_parent_dir_row() {
        let (root, app, mut panel) = test_tree();
        panel.expand_or_descend(&app).unwrap(); // expand a_dir
        panel.move_selection(1, &app); // select a1.txt
        panel.collapse_or_parent(&app);
        assert_eq!(panel.selected, 0); // back on a_dir, still expanded
        assert!(panel.rows(&app)[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn h_on_top_level_stays_put() {
        let (root, app, mut panel) = test_tree();
        panel.collapse_or_parent(&app);
        assert_eq!(panel.selected, 0);
        assert_eq!(panel.rows(&app).len(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ctrl_modified_keys_fall_through_to_global_keymap() {
        // C-l is Char('l') + CONTROL: without the guard the panel eats it
        // as "expand" and window focus can never move right out of the
        // explorer.
        let (root, mut app, mut panel) = test_tree();

        let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert!(matches!(
            panel.handle_key(ctrl_l, &mut app),
            KeyResult::Ignored
        ));

        let plain_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE);
        assert!(matches!(
            panel.handle_key(plain_l, &mut app),
            KeyResult::Consumed
        ));
        assert!(panel.rows(&app)[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn toggle_show_all_reveals_ignored_and_dotfiles() {
        let (root, mut app, _) = test_tree();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/build.o"), "").unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        let mut panel = FilesPanel::new(root.clone()).unwrap(); // fresh listing
        panel.selected = 0;

        let names = |panel: &FilesPanel, app: &App| -> Vec<String> {
            panel.rows(app).iter().map(|r| r.name.clone()).collect()
        };
        app.workspace = crate::files::Workspace::new(root.clone());

        // filtered: no target/, no dotfiles
        assert_eq!(names(&panel, &app), ["a_dir", "z_dir", "b.txt"]);

        // full: everything, selection clamps when shrinking back
        assert!(panel.toggle_show_all(&app));
        assert_eq!(
            names(&panel, &app),
            ["a_dir", "target", "z_dir", ".env", "b.txt"]
        );
        panel.move_selection(10, &app);
        assert_eq!(panel.selected, 4);
        assert!(!panel.toggle_show_all(&app));
        assert!(panel.selected <= 2, "clamped into the filtered list");
        fs::remove_dir_all(root).unwrap();
    }
}
