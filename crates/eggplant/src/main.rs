//! eggplant-code — AI-native terminal editor.
//!
//! Thin launcher: parse CLI args, hand over to the UI runtime. All UI logic
//! lives in `eggplant-ui` (see its `runner`).

use std::io;
use std::path::PathBuf;

use clap::Parser;

/// AI-native terminal editor.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// File or directory to open. A directory opens the file explorer
    /// (netrw-style) with a scratch buffer; omitted = scratch buffer only.
    path: Option<PathBuf>,
}

fn main() -> io::Result<()> {
    eggplant_ui::runner::run(Cli::parse().path)
}
