//! Buffer grep (`Space s c`): a picker over the current buffer's lines.

use crate::compositor::KeyResult;

use super::picker::{Picker, PickerSource, PickerSpec};

/// One buffer line, as a picker item.
pub struct BufferLine {
    /// 0-based line number.
    line: usize,
    text: String,
}

/// A live picker over `lines` (all lines of the current buffer): fuzzy
/// filters as you type, Enter jumps to the selected line.
pub fn buffer_grep(lines: Vec<String>) -> Picker<BufferLine> {
    let items = lines
        .into_iter()
        .enumerate()
        .map(|(line, text)| BufferLine { line, text })
        .collect();
    Picker::new(PickerSpec {
        title: "grep",
        source: PickerSource::List {
            items,
            text_of: |item| &item.text,
        },
        project: |item| (format!(":{}", item.line + 1), item.text.clone()),
        on_select: |item, app| {
            app.editor.move_to_line(item.line);
            // Jumping to a line moves the cursor: focus follows.
            KeyResult::CloseUnfocus
        },
        preview_of: None,
    })
}
