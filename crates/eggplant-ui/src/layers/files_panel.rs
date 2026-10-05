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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};

use crate::app::App;
use crate::components::files_panel::{self, FilesPanelProps, RowKind, RowProps};
use crate::compositor::{KeyResult, Layer, LayerKind, Side};
use crate::element::Element;
use crate::files::IgnoreRules;
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
    /// `false`: ignore rules + dotfiles are filtered out (default).
    /// `true` (`I` toggle): the full, unfiltered listing.
    show_all: bool,
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
            show_all: false,
            root,
            cache,
            selected: 0,
            offset: 0,
            inner_height: 1,
        })
    }

    /// The flattened, currently-visible tree (derived from `expanded`).
    /// Unless `show_all`, dotfiles and ignore-rule matches are filtered.
    fn rows(&self, ignores: &IgnoreRules) -> Vec<Row> {
        fn walk(
            panel: &FilesPanel,
            ignores: &IgnoreRules,
            dir: &Path,
            depth: usize,
            rows: &mut Vec<Row>,
        ) {
            let Some(children) = panel.cache.get(dir) else {
                return;
            };
            for child in children {
                let path = dir.join(&child.name);
                if !panel.show_all
                    && (child.name.starts_with('.') || ignores.is_ignored(&path, child.is_dir))
                {
                    continue;
                }
                let expanded = panel.expanded.contains(&path);
                rows.push(Row {
                    name: child.name.clone(),
                    path: path.clone(),
                    depth,
                    is_dir: child.is_dir,
                    expanded,
                });
                if child.is_dir && expanded {
                    walk(panel, ignores, &path, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, ignores, &self.root.clone(), 0, &mut rows);
        rows
    }

    fn selected_row(&self, ignores: &IgnoreRules) -> Option<Row> {
        self.rows(ignores).into_iter().nth(self.selected)
    }

    fn move_selection(&mut self, delta: isize, ignores: &IgnoreRules) {
        let len = self.rows(ignores).len();
        if len == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        self.ensure_selection_visible();
    }

    /// `I`: toggle the full / filtered listing.
    fn toggle_show_all(&mut self, ignores: &IgnoreRules) -> bool {
        self.show_all = !self.show_all;
        // Selection may point past the end of the now-shorter list.
        let len = self.rows(ignores).len();
        if len > 0 {
            self.selected = self.selected.min(len - 1);
        } else {
            self.selected = 0;
        }
        self.ensure_selection_visible();
        self.show_all
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
    fn expand_selected(&mut self, ignores: &IgnoreRules) -> io::Result<()> {
        let Some(row) = self.selected_row(ignores) else {
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
    fn expand_or_descend(&mut self, ignores: &IgnoreRules) -> io::Result<()> {
        let Some(row) = self.selected_row(ignores) else {
            return Ok(());
        };
        if !row.is_dir {
            return Ok(());
        }
        if row.expanded {
            // First child is the next row (if the directory is non-empty).
            let len = self.rows(ignores).len();
            if self.selected + 1 < len {
                self.move_selection(1, ignores);
            }
            Ok(())
        } else {
            self.expand_selected(ignores)
        }
    }

    /// `h`: collapse an expanded directory; otherwise jump to the parent row.
    fn collapse_or_parent(&mut self, ignores: &IgnoreRules) {
        let Some(row) = self.selected_row(ignores) else {
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
            && let Some(index) = self.rows(ignores).iter().position(|r| r.path == parent)
        {
            self.selected = index;
            self.ensure_selection_visible();
        }
    }

    /// `Enter` on a directory: toggle expanded/collapsed.
    fn toggle_selected_dir(&mut self, ignores: &IgnoreRules) -> io::Result<()> {
        let Some(row) = self.selected_row(ignores) else {
            return Ok(());
        };
        if row.expanded {
            self.expanded.remove(&row.path);
            Ok(())
        } else {
            self.expand_selected(ignores)
        }
    }

    fn open_selected(&mut self, app: &mut App) -> KeyResult {
        let Some(row) = self.selected_row(&app.file_ignores) else {
            return KeyResult::Consumed;
        };
        if row.is_dir {
            if let Err(err) = self.toggle_selected_dir(&app.file_ignores) {
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
                    .rows(&app.file_ignores)
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
        // Plain letters only: Ctrl/Alt-modified keys (C-l, C-h…) belong to
        // the global keymap (window focus moves through the panel too).
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return KeyResult::Ignored;
        }
        match key.code {
            KeyCode::Esc => KeyResult::Unfocus,
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1, &app.file_ignores);
                KeyResult::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1, &app.file_ignores);
                KeyResult::Consumed
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if let Err(err) = self.expand_or_descend(&app.file_ignores) {
                    app.notifications
                        .push(Notification::error(format!("cannot read directory: {err}")));
                }
                KeyResult::Consumed
            }
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                self.collapse_or_parent(&app.file_ignores);
                KeyResult::Consumed
            }
            KeyCode::Char('I') => {
                let show_all = self.toggle_show_all(&app.file_ignores);
                app.notifications.push(Notification::info(if show_all {
                    "explorer: showing all files"
                } else {
                    "explorer: filtered"
                }));
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

    fn rules_for(root: &Path) -> IgnoreRules {
        IgnoreRules::new(root, &[])
    }

    fn names(panel: &FilesPanel, ignores: &IgnoreRules) -> Vec<String> {
        panel
            .rows(ignores)
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    #[test]
    fn initial_rows_are_top_level_dirs_first() {
        let (root, panel) = test_tree();
        assert_eq!(
            names(&panel, &rules_for(&root)),
            ["a_dir", "z_dir", "b.txt"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expand_reveals_children_and_collapse_hides_them() {
        let (root, mut panel) = test_tree();
        let rules = rules_for(&root);
        panel.expand_selected(&rules).unwrap(); // a_dir selected (row 0)
        assert_eq!(
            names(&panel, &rules),
            ["a_dir", "  a1.txt", "  a2.txt", "z_dir", "b.txt"]
        );

        panel.collapse_or_parent(&rules); // h on expanded dir collapses it
        assert_eq!(names(&panel, &rules), ["a_dir", "z_dir", "b.txt"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn h_on_file_selects_parent_dir_row() {
        let (root, mut panel) = test_tree();
        let rules = rules_for(&root);
        panel.expand_selected(&rules).unwrap(); // expand a_dir
        panel.move_selection(1, &rules); // select a1.txt
        panel.collapse_or_parent(&rules);
        assert_eq!(panel.selected, 0); // back on a_dir, still expanded
        assert!(panel.rows(&rules)[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn l_expands_then_descends() {
        let (root, mut panel) = test_tree();
        let rules = rules_for(&root);
        panel.expand_or_descend(&rules).unwrap(); // expand a_dir
        assert!(panel.rows(&rules)[0].expanded);
        assert_eq!(panel.selected, 0);
        panel.expand_or_descend(&rules).unwrap(); // descend to first child
        assert_eq!(panel.selected, 1);
        assert_eq!(panel.rows(&rules)[1].name, "a1.txt");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enter_toggles_directories() {
        let (root, mut panel) = test_tree();
        let rules = rules_for(&root);
        panel.toggle_selected_dir(&rules).unwrap();
        assert!(panel.rows(&rules)[0].expanded);
        panel.toggle_selected_dir(&rules).unwrap();
        assert!(!panel.rows(&rules)[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn root_is_never_left() {
        let (root, mut panel) = test_tree();
        let rules = rules_for(&root);
        // h on a top-level row: parent is the root itself — stay put.
        panel.collapse_or_parent(&rules);
        assert_eq!(panel.selected, 0);
        assert_eq!(names(&panel, &rules), ["a_dir", "z_dir", "b.txt"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ctrl_modified_keys_fall_through_to_global_keymap() {
        // C-l is Char('l') + CONTROL: without the guard the panel eats it
        // as "expand" and window focus can never move right out of the
        // explorer.
        let (root, mut panel) = test_tree();
        let mut app = App::new(eggplant_core::Editor::scratch().unwrap());

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
        assert!(panel.rows(&rules_for(&root))[0].expanded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn toggle_show_all_reveals_ignored_and_dotfiles() {
        let (root, mut panel) = test_tree();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/build.o"), "").unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        // Refresh the cached root listing (fixture wrote it before these).
        panel
            .cache
            .insert(root.clone(), read_children(&root).unwrap());
        let rules = rules_for(&root);

        // filtered: no target/, no dotfiles
        assert_eq!(names(&panel, &rules), ["a_dir", "z_dir", "b.txt"]);

        // full: everything, selection clamps when shrinking back
        assert!(panel.toggle_show_all(&rules));
        assert_eq!(
            names(&panel, &rules),
            ["a_dir", "target", "z_dir", ".env", "b.txt"]
        );
        panel.move_selection(10, &rules);
        assert_eq!(panel.selected, 4);
        assert!(!panel.toggle_show_all(&rules));
        assert!(panel.selected <= 2, "clamped into the filtered list");
        fs::remove_dir_all(root).unwrap();
    }
}
