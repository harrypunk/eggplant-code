//! Project grep: find pattern matches across the workspace and describe
//! their context (see docs/design/live-grep.md).
//!
//! The pure core (`grep_text`) knows nothing about files; the fs shell
//! (`search_workspace`, `preview`) is thin and uses the same `Workspace`
//! file set as the file picker — one truth for "the project".

use std::path::PathBuf;

use regex::Regex;

use crate::components::preview::PreviewProps;
use crate::files::Workspace;

/// Don't search below this pattern length (like leap's 2 chars).
pub const MIN_PATTERN: usize = 2;
/// Stop collecting after this many hits.
pub const MAX_HITS: usize = 500;
/// Skip files larger than this.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// A NUL in the first this-many bytes marks a file as binary.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
/// Preview context lines above/below the hit (±5).
const PREVIEW_CONTEXT: usize = 5;

/// One match: where it is, and the line it sits on.
#[derive(Debug, Clone)]
pub struct GrepHit {
    /// Workspace-relative path (display).
    pub rel: String,
    /// Absolute path (opening, preview).
    pub abs: PathBuf,
    /// 0-based line number.
    pub line: usize,
    /// Char column range of the match within the line.
    pub cols: (usize, usize),
    /// The matched line's full text (list display).
    pub text: String,
}

/// One match within a text: `(line, start_col, end_col)` in chars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// Compile the pattern, rg-style: invalid regexes search literally, an
/// all-lowercase pattern is case-insensitive (smart case).
fn compile(pattern: &str) -> Regex {
    let source = if Regex::new(pattern).is_ok() {
        pattern.to_owned()
    } else {
        regex::escape(pattern)
    };
    let insensitive = pattern.chars().all(|c| !c.is_uppercase());
    regex::RegexBuilder::new(&source)
        .case_insensitive(insensitive)
        .build()
        .expect("escaped pattern always compiles")
}

/// All matches of `pattern` in `text` (pure). Columns are char offsets.
pub fn grep_text(pattern: &str, text: &str) -> Vec<Match> {
    let regex = compile(pattern);
    text.lines()
        .enumerate()
        .flat_map(|(line, content)| {
            regex.find_iter(content).map(move |m| {
                let start = content[..m.start()].chars().count();
                let end = start + m.as_str().chars().count();
                Match { line, start, end }
            })
        })
        .collect()
}

/// Search every workspace file for `pattern` (capped — see the
/// constants). Empty below `MIN_PATTERN`.
pub fn search_workspace(workspace: &Workspace, pattern: &str) -> Vec<GrepHit> {
    if pattern.chars().count() < MIN_PATTERN {
        return Vec::new();
    }
    let regex = compile(pattern);
    workspace
        .collect_files(usize::MAX)
        .iter()
        .flat_map(|entry| grep_file(&regex, entry.rel.clone(), entry.abs.clone()))
        .take(MAX_HITS)
        .collect()
}

/// Matches in one readable, non-binary, small-enough file.
fn grep_file(regex: &Regex, rel: String, abs: PathBuf) -> Vec<GrepHit> {
    let Ok(meta) = std::fs::metadata(&abs) else {
        return Vec::new();
    };
    if meta.len() > MAX_FILE_BYTES {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(&abs) else {
        return Vec::new();
    };
    if bytes.iter().take(BINARY_SNIFF_BYTES).any(|b| *b == 0) {
        return Vec::new(); // binary
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .flat_map(|(line, content)| {
            regex.find_iter(content).map(move |m| {
                let start = content[..m.start()].chars().count();
                (line, start, start + m.as_str().chars().count(), content)
            })
        })
        .map(|(line, start, end, content)| GrepHit {
            rel: rel.clone(),
            abs: abs.clone(),
            line,
            cols: (start, end),
            text: content.to_owned(),
        })
        .collect()
}

/// Materialize the preview for a hit: `PREVIEW_CONTEXT` lines around it,
/// clamped at the file's start.
pub fn preview(hit: &GrepHit) -> Option<PreviewProps> {
    let text = std::fs::read_to_string(&hit.abs).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let first_line = hit.line.saturating_sub(PREVIEW_CONTEXT);
    let window: Vec<String> = lines
        .iter()
        .skip(first_line)
        .take(2 * PREVIEW_CONTEXT + 1)
        .map(|line| (*line).to_owned())
        .collect();
    Some(PreviewProps {
        title: format!("{}:{}", hit.rel, hit.line + 1),
        first_line,
        focus_row: hit.line - first_line,
        focus_cols: hit.cols,
        lines: window,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_text_finds_all_matches_with_char_columns() {
        assert_eq!(
            grep_text("ab", "ab ab\nxab"),
            vec![
                Match {
                    line: 0,
                    start: 0,
                    end: 2
                },
                Match {
                    line: 0,
                    start: 3,
                    end: 5
                },
                Match {
                    line: 1,
                    start: 1,
                    end: 3
                },
            ]
        );
        assert_eq!(grep_text("zz", "ab ab"), Vec::new());
    }

    #[test]
    fn pattern_is_regex_with_literal_fallback() {
        assert_eq!(grep_text("a.c", "abc axc").len(), 2, "regex works");
        assert_eq!(
            grep_text("a.c", "a.c").len(),
            1,
            "…and dot still matches itself"
        );
        // An unparseable pattern searches literally.
        assert_eq!(
            grep_text("a(", "a( aX"),
            vec![Match {
                line: 0,
                start: 0,
                end: 2
            }]
        );
        assert_eq!(
            grep_text("[0-9]", "[0-9] 7").len(),
            3,
            "valid classes stay regex"
        );
    }

    #[test]
    fn smart_case() {
        assert_eq!(
            grep_text("foo", "FOO foo").len(),
            2,
            "lowercase → insensitive"
        );
        assert_eq!(
            grep_text("Foo", "FOO foo Foo").len(),
            1,
            "uppercase → sensitive"
        );
    }

    #[test]
    fn grep_workspace_and_preview_over_a_temp_dir() {
        let root = std::env::temp_dir().join(format!("eggplant-grep-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    let needle = 1;\n    println!(\"{needle}\");\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("binary.bin"), b"a\0needle".as_slice()).unwrap();
        let big = root.join("big.log");
        std::fs::write(&big, "x".repeat(1024 * 1024 + 1)).unwrap();

        let ws = Workspace::new(root.clone());
        let hits = search_workspace(&ws, "needle");
        assert_eq!(hits.len(), 2, "binary and oversized files are skipped");
        assert_eq!(hits[0].rel, "src/main.rs");
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].cols, (8, 14));

        // Preview: ±3 lines clamped at the file start.
        let preview = preview(&hits[0]).unwrap();
        assert_eq!(preview.first_line, 0, "clamped at file start");
        assert_eq!(preview.focus_row, 1);
        assert_eq!(preview.focus_cols, (8, 14));
        assert_eq!(preview.lines.len(), 4);

        // Below the minimum pattern length: nothing.
        assert!(search_workspace(&ws, "n").is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }
}
