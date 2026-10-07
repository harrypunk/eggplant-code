//! The file picker (`Space f p`): a picker over workspace files.

use crate::app::App;
use eggplant_core::files::FileEntry;

use super::picker::{Picker, PickerSource, PickerSpec, Select};

/// Pathological-tree guard: plenty for real projects, bounded for `/`.
const FILE_CAP: usize = 20_000;

/// Fuzzy over relative paths; Enter opens the file.
pub fn file_picker(app: &App) -> Picker<FileEntry> {
    Picker::new(PickerSpec {
        title: "open",
        source: PickerSource::List {
            items: app.workspace.collect_files(FILE_CAP),
            text_of: |file| &file.rel,
        },
        project: |file| {
            let name = file
                .abs
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.rel.clone());
            (name, file.rel.clone())
        },
        on_select: |file| Select::OpenAt {
            path: file.abs.clone(),
            at: None,
        },
        preview_of: None,
    })
}
