//! Project grep (`Space s p`): a live picker over workspace matches with
//! a preview pane (docs/design/live-grep.md).

use crate::app::App;
use crate::compositor::KeyResult;
use crate::layers::notification::Notification;
use crate::project_grep::{self, GrepHit};

use super::picker::{Picker, PickerSource, PickerSpec};

/// Live query across the workspace; the preview shows the selected hit in
/// context; Enter opens the file at the match.
pub fn project_grep(_app: &App) -> Picker<GrepHit> {
    Picker::new(PickerSpec {
        title: "project grep",
        source: PickerSource::Query {
            run: |input, app| project_grep::search_workspace(&app.workspace, input),
        },
        project: |hit| {
            (
                format!("{}:{}", hit.rel, hit.line + 1),
                hit.text.trim().to_owned(),
            )
        },
        on_select: |hit, app| {
            match app.editor.open_buffer(&hit.abs) {
                Ok(()) => app.editor.jump_to(hit.line, hit.cols.0),
                Err(err) => app
                    .notifications
                    .push(Notification::error(format!("open {}: {err:#}", hit.rel))),
            }
            // Jumping to a match moves the cursor: focus follows.
            KeyResult::CloseUnfocus
        },
        preview_of: Some(|hit, _| project_grep::preview(hit)),
    })
}
