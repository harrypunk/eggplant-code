//! Project grep (`Space s p`): a live picker over workspace matches with
//! a preview pane (docs/design/live-grep.md).

use eggplant_core::grep::{self, GrepHit};

use crate::app::App;
use crate::components::preview::{PreviewProps, PreviewRow};

use super::picker::{Picker, PickerSource, PickerSpec};
use crate::action::AppAction;

/// Context lines BEFORE the hit — the preview's scroll position.
/// Below it, the file fills the pane (`budget` rows, the pane's real
/// capacity): preview-as-viewport, no artificial cap.
const CONTEXT: usize = 5;

/// Live query across the workspace; the preview shows the selected hit in
/// context; Enter opens the file at the match.
pub fn project_grep(_app: &App) -> Picker<GrepHit> {
    Picker::new(PickerSpec {
        title: "project grep",
        source: PickerSource::Query {
            run: |input, app| grep::search_workspace(&app.workspace, input),
        },
        project: |hit| {
            (
                format!("{}:{}", hit.rel, hit.line + 1),
                hit.text.trim().to_owned(),
            )
        },
        on_select: |hit| AppAction::OpenBuffer {
            path: hit.abs.clone(),
            at: Some((hit.line, hit.cols.0)),
        },
        preview_of: Some(preview_props),
    })
}

/// The preview is a PEEK, not hand-built text: open the hit's file as a
/// read-only core document and reuse the editor's syntax-highlighted
/// spans (docs/design/live-grep.md). Files that fail to open fall back
/// to plain context lines.
fn preview_props(hit: &GrepHit, app: &App, budget: usize) -> Option<PreviewProps> {
    let first = hit.line.saturating_sub(CONTEXT);
    let title = format!("{}:{}", hit.rel, hit.line + 1);
    let rows: Vec<PreviewRow> = match app.editor.peek(&hit.abs) {
        Ok(peek) => {
            let display_count = peek.line_count().saturating_sub(1); // phantom line
            let end = display_count.min(first + budget);
            (first..end)
                .map(|line| PreviewRow {
                    spans: peek.highlighted_line(line),
                    search_marks: match_band(hit, line),
                })
                .collect()
        }
        Err(_) => {
            let (first, lines) = eggplant_core::peek::context_lines(&hit.abs, hit.line, CONTEXT)?;
            let lines: Vec<String> = lines.into_iter().take(budget).collect();
            let title = format!("{}:{}", hit.rel, hit.line + 1);
            let rows = lines
                .into_iter()
                .enumerate()
                .map(|(i, text)| PreviewRow {
                    spans: vec![eggplant_core::HighlightedSpan { text, scope: None }],
                    search_marks: match_band(hit, first + i),
                })
                .collect();
            return Some(PreviewProps {
                title,
                first_line: first,
                rows,
                focus_row: hit.line - first,
            });
        }
    };
    Some(PreviewProps {
        title,
        first_line: first,
        rows,
        focus_row: hit.line - first,
    })
}

/// The match band rides on the hit's row only.
fn match_band(hit: &GrepHit, line: usize) -> Vec<(usize, usize, bool)> {
    if line == hit.line {
        vec![(hit.cols.0, hit.cols.1, true)]
    } else {
        Vec::new()
    }
}
