//! A small subsequence fuzzy matcher (for the command palette).
//!
//! Returns a score when `pattern` is a subsequence of `text`
//! (case-insensitive); higher is better. Consecutive matches and matches at
//! word boundaries score higher. `None` when not a subsequence.

pub fn fuzzy_score(pattern: &str, text: &str) -> Option<u32> {
    let pattern: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    if pattern.is_empty() {
        return Some(0);
    }

    let mut score = 0;
    let mut pattern_iter = pattern.iter().peekable();
    let mut prev_matched = false;
    let mut prev_char: Option<char> = None;

    for c in text.chars().flat_map(char::to_lowercase) {
        let Some(&&want) = pattern_iter.peek() else {
            break;
        };
        if c == want {
            score += 1;
            if prev_matched {
                score += 3; // consecutive run
            }
            match prev_char {
                None => score += 4,                             // starts the text
                Some(prev) if is_separator(prev) => score += 2, // word boundary
                _ => {}
            }
            prev_matched = true;
            pattern_iter.next();
        } else {
            prev_matched = false;
        }
        prev_char = Some(c);
    }

    pattern_iter.peek().is_none().then_some(score)
}

fn is_separator(c: char) -> bool {
    matches!(c, ' ' | '.' | '-' | '_' | '/' | ':')
}

/// Bonus when the pattern matches the basename (the part after the last
/// `/`) — path pickers are almost always about the file NAME. Large
/// enough to dominate any full-path score.
const BASENAME_BONUS: u32 = 1 << 16;

/// Score `text` against `pattern`, preferring a basename match when the
/// text looks like a path.
fn path_aware_score(pattern: &str, text: &str) -> Option<u32> {
    let path_score = fuzzy_score(pattern, text);
    let basename_score = text
        .rsplit('/')
        .next()
        .filter(|basename| basename.len() < text.len())
        .and_then(|basename| fuzzy_score(pattern, basename))
        .map(|score| score + BASENAME_BONUS);
    path_score.into_iter().chain(basename_score).max()
}

/// Fuzzy-filter and rank `items` (best first) by a display-text projection.
/// Basename matches beat path matches; ties break by shorter text, then
/// alphabetically — deterministic, so results don't jitter as you type.
pub fn filter<'a, T>(
    pattern: &str,
    items: &'a [T],
    text_of: impl Fn(&'a T) -> &'a str,
) -> Vec<(u32, &'a T)> {
    let mut scored: Vec<(u32, &T)> = items
        .iter()
        .filter_map(|item| path_aware_score(pattern, text_of(item)).map(|score| (score, item)))
        .collect();
    scored.sort_by(|(a_score, a_item), (b_score, b_item)| {
        b_score
            .cmp(a_score)
            .then_with(|| text_of(a_item).len().cmp(&text_of(b_item).len()))
            .then_with(|| text_of(a_item).cmp(text_of(b_item)))
    });
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_required() {
        assert!(fuzzy_score("fs", "file.save").is_some());
        assert!(fuzzy_score("xyz", "file.save").is_none());
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_score("FS", "file.save").is_some());
    }

    #[test]
    fn better_matches_score_higher() {
        // Prefix match beats scattered match.
        let prefix = fuzzy_score("file", "file.save").unwrap();
        let scattered = fuzzy_score("file", "fix.long.entry").unwrap();
        assert!(prefix > scattered);
        // Consecutive beats gaps.
        let consecutive = fuzzy_score("save", "file.save").unwrap();
        let gapped = fuzzy_score("save", "s.a.v.e").unwrap();
        assert!(consecutive > gapped);
    }

    #[test]
    fn basename_match_beats_scattered_path_match() {
        // Regression: "app" lost to any path containing a…p…p scattered
        // across directories; the picker buried app.rs.
        let items = [
            "crates/eggplant-agent/src/lib.rs",
            "crates/eggplant-ui/src/app.rs",
            "docs/design/architecture.md",
        ];
        let ranked = filter("app", &items, |s| s);
        assert_eq!(ranked[0].1, &"crates/eggplant-ui/src/app.rs");
    }

    #[test]
    fn ties_break_shorter_then_alphabetical() {
        let items = ["z/app.rs", "a/app.rs", "a/longer/app.rs"];
        let ranked = filter("app", &items, |s| s);
        let texts: Vec<&str> = ranked.iter().map(|(_, item)| **item).collect();
        assert_eq!(texts, vec!["a/app.rs", "z/app.rs", "a/longer/app.rs"]);
    }

    #[test]
    fn filter_ranks_best_first() {
        let items = ["file.save", "focus.next", "demo.dialog"];
        let ranked = filter("fi", &items, |s| s);
        assert_eq!(ranked[0].1, &"file.save");
    }
}
