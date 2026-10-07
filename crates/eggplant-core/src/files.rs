//! File discovery for pickers: workspace walking + ignore rules.
//!
//! Ignore rules are gitignore-syntax (via the `ignore` crate): built-in
//! defaults first, user patterns appended — so `!pattern` re-includes, and
//! adding/removing is one mechanism. The walker additionally respects
//! `.gitignore`/`.ignore` files and skips hidden entries.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::gitignore::GitignoreBuilder;

/// Built-in ignores: dependency/build directories that drown pickers.
const DEFAULT_IGNORES: &[&str] = &[
    "target/",
    "node_modules/",
    "__pycache__/",
    ".venv/",
    "venv/",
    "*.egg-info/",
];

/// Gitignore-style ignore rules rooted at the workspace root.
#[derive(Clone)]
pub struct IgnoreRules {
    root: PathBuf,
    gitignore: ignore::gitignore::Gitignore,
}

impl IgnoreRules {
    /// Defaults + `extra` user patterns (config `[files] ignore`).
    pub fn new(root: &Path, extra: &[String]) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        for pattern in DEFAULT_IGNORES {
            builder.add_line(None, pattern).expect("built-in pattern");
        }
        for pattern in extra {
            // Invalid user patterns are dropped; strictness buys nothing here.
            let _ = builder.add_line(None, pattern);
        }
        Self {
            root: root.to_path_buf(),
            gitignore: builder.build().expect("gitignore build"),
        }
    }

    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return false;
        };
        self.gitignore.matched(rel, is_dir).is_ignore()
    }
}

/// The workspace: root directory + the ignore rules rooted at it. Root and
/// rules change together (directory startup re-roots, config re-patterns),
/// so they live in one type — `App` composes it instead of loose fields.
pub struct Workspace {
    pub root: PathBuf,
    pub ignores: IgnoreRules,
}

impl Workspace {
    /// Rooted at `root` with the built-in ignore defaults.
    pub fn new(root: PathBuf) -> Self {
        Self {
            ignores: IgnoreRules::new(&root, &[]),
            root,
        }
    }

    /// Re-root (directory startup): rules re-root too.
    pub fn set_root(&mut self, root: PathBuf) {
        self.ignores = IgnoreRules::new(&root, &[]);
        self.root = root;
    }

    /// Apply user patterns (config `[files] ignore`, gitignore syntax).
    pub fn set_ignore_patterns(&mut self, extra: &[String]) {
        self.ignores = IgnoreRules::new(&self.root.clone(), extra);
    }

    /// Files under the root (respecting .gitignore + ignore rules), capped.
    pub fn collect_files(&self, cap: usize) -> Vec<FileEntry> {
        collect_files(&self.root, &self.ignores, cap)
    }
}

/// A discovered file: relative path for display/filtering, absolute for
/// opening.
pub struct FileEntry {
    pub rel: String,
    pub abs: PathBuf,
}

/// Walk `root` for files (respecting .gitignore + ignore rules), capped.
pub fn collect_files(root: &Path, rules: &IgnoreRules, cap: usize) -> Vec<FileEntry> {
    let filter = rules.clone();
    let filter_root = rules.root.clone();
    let mut builder = WalkBuilder::new(root);
    builder
        .git_ignore(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            !filter.is_ignored(entry.path(), is_dir)
        });
    builder
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|entry| {
            let abs = entry.into_path();
            let rel = abs.strip_prefix(&filter_root).ok()?.to_path_buf();
            Some(FileEntry {
                rel: rel.display().to_string(),
                abs,
            })
        })
        .take(cap)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "eggplant-files-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/leftpad")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("target/debug/build.o"), "").unwrap();
        std::fs::write(dir.join("node_modules/leftpad/index.js"), "").unwrap();
        dir
    }

    fn rels(files: &[FileEntry]) -> Vec<&str> {
        let mut rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        rels.sort_unstable(); // walk order is filesystem-dependent
        rels
    }

    #[test]
    fn defaults_ignore_build_and_dependency_dirs() {
        let root = fixture();
        let files = collect_files(&root, &IgnoreRules::new(&root, &[]), 1000);
        assert_eq!(rels(&files), ["src/main.rs"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn user_patterns_add_and_negate() {
        let root = fixture();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/bundle.js"), "").unwrap();

        // add: dist/ ignored too
        let rules = IgnoreRules::new(&root, &["dist/".to_string()]);
        assert_eq!(rels(&collect_files(&root, &rules, 1000)), ["src/main.rs"]);

        // remove: `!` re-includes a built-in default (gitignore semantics)
        let rules = IgnoreRules::new(&root, &["!target/".to_string()]);
        let collected = collect_files(&root, &rules, 1000);
        let files = rels(&collected);
        assert!(files.contains(&"target/debug/build.o"));
        assert!(!files.contains(&"node_modules/leftpad/index.js"));
        std::fs::remove_dir_all(&root).ok();
    }
}
