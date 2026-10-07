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

/// Fuzzy-filter and rank `items` (best first) by a display-text projection.
pub fn filter<'a, T>(
    pattern: &str,
    items: &'a [T],
    text_of: impl Fn(&'a T) -> &'a str,
) -> Vec<(u32, &'a T)> {
    let mut scored: Vec<(u32, &T)> = items
        .iter()
        .filter_map(|item| fuzzy_score(pattern, text_of(item)).map(|score| (score, item)))
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
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
    fn filter_ranks_best_first() {
        let items = ["file.save", "focus.next", "demo.dialog"];
        let ranked = filter("fi", &items, |s| s);
        assert_eq!(ranked[0].1, &"file.save");
    }
}
