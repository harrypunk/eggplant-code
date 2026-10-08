//! `edit` — exact-text multi-replacement (pi's edit contract):
//! each `old_text` must occur exactly once in the original; edits are
//! matched against the original, not incrementally; overlapping edits
//! are rejected. The host applies them as one transaction.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::host::{HostCall, HostClient, HostReply, TextEdit};
use crate::tool::{Tool, ToolOutput};
use crate::tools::arg_path;

pub struct Edit;

#[async_trait]
impl Tool for Edit {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn snippet(&self) -> &'static str {
        "Make precise file edits with exact text replacement, including multiple disjoint edits in one call"
    }

    fn guidelines(&self) -> &'static [&'static str] {
        &[
            "Every old_text must match the file exactly and uniquely — read the file first.",
            "If two changes touch the same block, merge them into one edit.",
        ]
    }

    fn params_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to edit" },
                "edits": {
                    "type": "array",
                    "description": "One or more targeted replacements, matched against the original file, not incrementally. Edits must not overlap or nest.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": { "type": "string", "description": "Exact text to replace; must be unique in the file" },
                            "new_text": { "type": "string", "description": "Replacement text" }
                        },
                        "required": ["old_text", "new_text"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    async fn execute(&self, args: Value, host: &HostClient) -> ToolOutput {
        let call = || -> Result<HostCall, String> {
            let edits = args
                .get("edits")
                .and_then(Value::as_array)
                .ok_or_else(|| "missing array argument 'edits'".to_string())?
                .iter()
                .map(|e| {
                    Ok(TextEdit {
                        old_text: e
                            .get("old_text")
                            .and_then(Value::as_str)
                            .ok_or("edit missing 'old_text'")?
                            .to_owned(),
                        new_text: e
                            .get("new_text")
                            .and_then(Value::as_str)
                            .ok_or("edit missing 'new_text'")?
                            .to_owned(),
                    })
                })
                .collect::<Result<Vec<_>, &str>>()
                .map_err(str::to_owned)?;
            Ok(HostCall::Edit {
                path: arg_path(&args, "path")?,
                edits,
            })
        };
        let call = match call() {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(e),
        };
        match host.call(call).await {
            Ok(HostReply::Text(report)) => ToolOutput::ok(report),
            Ok(_) => ToolOutput::error("unexpected host reply"),
            Err(e) => ToolOutput::error(e),
        }
    }
}

/// Pure edit application, used by the host side: match each
/// `old_text` against the original content (exact first, then the
/// fuzzy normalization: trailing whitespace per line), apply all
/// replacements, reject ambiguity/overlap. Returns the new content.
pub fn apply_edits(content: &str, edits: &[TextEdit]) -> Result<String, String> {
    // Match every edit against the ORIGINAL content first.
    let spans: Vec<(usize, usize)> = edits
        .iter()
        .map(|edit| match_span(content, &edit.old_text))
        .collect::<Result<_, _>>()?;
    // Reject overlap/nesting.
    let mut sorted = spans.clone();
    sorted.sort_unstable();
    for pair in sorted.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err("edits overlap; merge changes touching the same block".to_string());
        }
    }
    // Apply back-to-front so spans stay valid.
    let mut result = content.to_owned();
    let mut indexed: Vec<_> = spans.into_iter().zip(edits.iter()).collect();
    indexed.sort_by_key(|((start, _), _)| usize::MAX - start);
    for ((start, end), edit) in indexed {
        result.replace_range(start..end, &edit.new_text);
    }
    Ok(result)
}

/// Find the unique byte span of `old_text`, with one fuzzy fallback:
/// line-wise trailing-whitespace-insensitive matching (pi's most
/// valuable normalization; smart quotes/dashes deferred).
fn match_span(content: &str, old_text: &str) -> Result<(usize, usize), String> {
    let mut exact = content.match_indices(old_text);
    match (exact.next(), exact.next()) {
        (Some((start, _)), None) => return Ok((start, start + old_text.len())),
        (None, None) => {}
        _ => return Err("old_text is not unique in the file".to_string()),
    }
    // Fuzzy: ignore trailing whitespace per line on both sides.
    let needle: Vec<&str> = old_text.lines().map(str::trim_end).collect();
    let hay_lines: Vec<&str> = content.lines().collect();
    let mut matches = Vec::new();
    for start in 0..hay_lines
        .len()
        .saturating_sub(needle.len().saturating_sub(1))
    {
        let window = &hay_lines[start..start + needle.len()];
        if window
            .iter()
            .zip(&needle)
            .all(|(have, want)| have.trim_end() == *want)
        {
            // Byte span: start of the `start`-th line through the last
            // matched line — line lengths + INTERIOR newlines only, so
            // the terminator after the block survives the replacement.
            let byte_start = hay_lines[..start]
                .iter()
                .map(|l| l.len() + 1)
                .sum::<usize>();
            let byte_end =
                byte_start + window.iter().map(|l| l.len()).sum::<usize>() + (window.len() - 1);
            matches.push((byte_start, byte_end));
        }
    }
    match matches.as_slice() {
        [span] => Ok(*span),
        [] => Err("old_text not found in the file".to_string()),
        _ => Err("old_text is not unique in the file".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> TextEdit {
        TextEdit {
            old_text: old.into(),
            new_text: new.into(),
        }
    }

    #[test]
    fn exact_unique_replacement() {
        let out = apply_edits(
            "fn a() {}\nfn b() {}\n",
            &[edit("fn b() {}", "fn b() { todo!() }")],
        )
        .unwrap();
        assert_eq!(out, "fn a() {}\nfn b() { todo!() }\n");
    }

    #[test]
    fn ambiguous_match_is_rejected() {
        let err = apply_edits("x\nx\n", &[edit("x", "y")]).unwrap_err();
        assert!(err.contains("not unique"), "{err}");
    }

    #[test]
    fn missing_match_is_rejected() {
        assert!(apply_edits("abc", &[edit("zzz", "y")]).is_err());
    }

    #[test]
    fn disjoint_edits_apply_in_one_pass() {
        let out = apply_edits(
            "alpha\nbeta\ngamma\n",
            &[edit("gamma", "G"), edit("alpha", "A")],
        )
        .unwrap();
        assert_eq!(out, "A\nbeta\nG\n");
    }

    #[test]
    fn overlapping_edits_are_rejected() {
        let err = apply_edits("abcdef", &[edit("abc", "x"), edit("bcd", "y")]).unwrap_err();
        assert!(err.contains("overlap"), "{err}");
    }

    #[test]
    fn exact_match_prefers_minimal_replacement() {
        // Model trimmed the trailing spaces; exact substring still matches
        // and the replacement stays minimal (file's whitespace untouched).
        let content = "let x = 1;   \nlet y = 2;\n";
        let out = apply_edits(content, &[edit("let x = 1;", "let x = 42;")]).unwrap();
        assert_eq!(out, "let x = 42;   \nlet y = 2;\n");
    }

    #[test]
    fn fuzzy_matches_block_with_trailing_whitespace() {
        // Exact fails (trailing spaces after the brace); fuzzy line-wise
        // trim-end matching finds the block.
        let content = "fn f() {   \n    x()\n}\n";
        let out = apply_edits(
            content,
            &[edit("fn f() {\n    x()\n}", "fn f() {\n    y()\n}")],
        )
        .unwrap();
        assert_eq!(out, "fn f() {\n    y()\n}\n");
    }
}
