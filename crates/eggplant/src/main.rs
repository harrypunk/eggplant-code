//! eggplant-code — AI-native terminal editor.
//!
//! Thin launcher: parse CLI args, build the editor, hand over to the UI
//! runtime. All UI logic lives in `eggplant-ui` (see its `runner`).

use std::io;
use std::path::PathBuf;

use eggplant_core::Editor;

fn main() -> io::Result<()> {
    let editor = match std::env::args().nth(1).map(PathBuf::from) {
        Some(path) => Editor::open(&path),
        None => Editor::scratch(),
    }
    .map_err(io::Error::other)?;

    eggplant_ui::runner::run(editor)
}
