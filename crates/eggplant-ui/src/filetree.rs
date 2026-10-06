//! The explorer's tree state: expansion, directory cache, and the visible
//! row derivation. Pure policy — directory I/O is injected ([`DirLister`]),
//! so tests run against in-memory listings, not temp dirs.
//!
//! Selection and scrolling are the panel's concern (list navigation over
//! the derived rows); this type only answers "what does the tree look
//! like" and "expand/collapse that directory".

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::files::IgnoreRules;

/// One directory child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Directory listing source — the injected I/O seam. Production passes
/// [`fs_lister`]; tests pass an in-memory listing.
pub type DirLister = fn(&Path) -> io::Result<Vec<DirEntry>>;

/// The real directory listing: directories first, then case-insensitive
/// names.
pub fn fs_lister(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries: Vec<DirEntry> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| DirEntry {
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

/// One visible row in the flattened tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// A directory tree under a fixed root (never changes), with lazily
/// cached listings.
pub struct FileTree {
    root: PathBuf,
    lister: DirLister,
    /// Directories whose children are visible (the root is always expanded).
    expanded: HashSet<PathBuf>,
    /// Lazily loaded directory listings, filled on expand.
    cache: HashMap<PathBuf, Vec<DirEntry>>,
}

impl FileTree {
    pub fn new(root: PathBuf, lister: DirLister) -> io::Result<Self> {
        let mut tree = Self {
            expanded: HashSet::from([root.clone()]),
            cache: HashMap::new(),
            root,
            lister,
        };
        let root = tree.root.clone();
        tree.load(&root)?;
        Ok(tree)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The flattened, currently-visible tree (derived from `expanded`).
    /// Unless `show_all`, dotfiles and ignore-rule matches are filtered out.
    pub fn rows(&self, ignores: &IgnoreRules, show_all: bool) -> Vec<TreeRow> {
        fn walk(
            tree: &FileTree,
            ignores: &IgnoreRules,
            show_all: bool,
            dir: &Path,
            depth: usize,
            rows: &mut Vec<TreeRow>,
        ) {
            let Some(children) = tree.cache.get(dir) else {
                return;
            };
            for child in children {
                let path = dir.join(&child.name);
                if !show_all
                    && (child.name.starts_with('.') || ignores.is_ignored(&path, child.is_dir))
                {
                    continue;
                }
                let expanded = tree.expanded.contains(&path);
                rows.push(TreeRow {
                    name: child.name.clone(),
                    path: path.clone(),
                    depth,
                    is_dir: child.is_dir,
                    expanded,
                });
                if child.is_dir && expanded {
                    walk(tree, ignores, show_all, &path, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, ignores, show_all, &self.root.clone(), 0, &mut rows);
        rows
    }

    /// Expand a collapsed directory, loading its listing on first use.
    pub fn expand(&mut self, dir: &Path) -> io::Result<()> {
        if self.expanded.contains(dir) {
            return Ok(());
        }
        self.load(dir)?;
        self.expanded.insert(dir.to_path_buf());
        Ok(())
    }

    pub fn collapse(&mut self, dir: &Path) {
        self.expanded.remove(dir);
    }

    /// `Enter` on a directory row.
    pub fn toggle(&mut self, dir: &Path) -> io::Result<()> {
        if self.expanded.contains(dir) {
            self.collapse(dir);
            Ok(())
        } else {
            self.expand(dir)
        }
    }

    fn load(&mut self, dir: &Path) -> io::Result<()> {
        if !self.cache.contains_key(dir) {
            let entries = (self.lister)(dir)?;
            self.cache.insert(dir.to_path_buf(), entries);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory filesystem: root has a_dir/{a1,a2}, z_dir/, b.txt.
    fn fake_lister(dir: &Path) -> io::Result<Vec<DirEntry>> {
        let entry = |name: &str, is_dir: bool| DirEntry {
            name: name.to_owned(),
            is_dir,
        };
        let listing = match dir.file_name().and_then(|n| n.to_str()) {
            Some("root") => vec![
                entry("a_dir", true),
                entry("z_dir", true),
                entry("b.txt", false),
            ],
            Some("a_dir") => vec![entry("a1.txt", false), entry("a2.txt", false)],
            Some("z_dir") => vec![],
            _ => return Err(io::Error::new(io::ErrorKind::NotFound, "no such dir")),
        };
        Ok(listing)
    }

    fn tree() -> FileTree {
        FileTree::new(PathBuf::from("/root"), fake_lister).unwrap()
    }

    fn ignores() -> IgnoreRules {
        IgnoreRules::new(Path::new("/root"), &[])
    }

    fn names(tree: &FileTree) -> Vec<String> {
        tree.rows(&ignores(), false)
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    #[test]
    fn initial_rows_are_top_level_dirs_first() {
        assert_eq!(names(&tree()), ["a_dir", "z_dir", "b.txt"]);
    }

    #[test]
    fn expand_reveals_children_and_collapse_hides_them() {
        let mut tree = tree();
        tree.expand(Path::new("/root/a_dir")).unwrap();
        assert_eq!(
            names(&tree),
            ["a_dir", "  a1.txt", "  a2.txt", "z_dir", "b.txt"]
        );
        tree.collapse(Path::new("/root/a_dir"));
        assert_eq!(names(&tree), ["a_dir", "z_dir", "b.txt"]);
    }

    #[test]
    fn toggle_goes_both_ways() {
        let mut tree = tree();
        tree.toggle(Path::new("/root/a_dir")).unwrap();
        assert!(tree.rows(&ignores(), false)[0].expanded);
        tree.toggle(Path::new("/root/a_dir")).unwrap();
        assert!(!tree.rows(&ignores(), false)[0].expanded);
    }

    #[test]
    fn listings_are_cached() {
        // Expanding twice (collapse keeps the cache) never re-lists: a
        // poisoned lister would fail the second read.
        let mut tree = tree();
        tree.expand(Path::new("/root/a_dir")).unwrap();
        tree.collapse(Path::new("/root/a_dir"));
        tree.expand(Path::new("/root/a_dir")).unwrap();
        assert_eq!(names(&tree).len(), 5);
    }

    #[test]
    fn errors_surface_from_expand() {
        let mut tree = tree();
        assert!(tree.expand(Path::new("/root/missing")).is_err());
    }
}
