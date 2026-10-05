//! Application bootstrap (composition root): resolve the CLI target and
//! build the initial `App` + `Compositor` wiring for it.

use std::io;
use std::path::PathBuf;

use eggplant_core::Editor;

use crate::app::App;
use crate::compositor::Compositor;
use crate::config::Config;
use crate::layers::editor::EditorSurface;
use crate::layers::files_panel::FilesPanel;
use crate::layers::notification::Notification;

/// What `eggplant [path]` was pointed at.
#[derive(Debug)]
pub enum StartupTarget {
    /// No argument: empty scratch buffer.
    Scratch,
    /// A file to edit.
    File(PathBuf),
    /// A directory: scratch buffer + file explorer rooted there (netrw-style).
    Directory(PathBuf),
}

impl StartupTarget {
    pub fn resolve(path: Option<PathBuf>) -> Self {
        match path {
            None => Self::Scratch,
            Some(path) if path.is_dir() => Self::Directory(path.canonicalize().unwrap_or(path)),
            Some(path) => Self::File(path),
        }
    }
}

/// Build the initial app state and layer stack for `target`.
pub fn boot(target: &StartupTarget) -> io::Result<(App, Compositor)> {
    let editor = match target {
        StartupTarget::File(path) => Editor::open(path),
        StartupTarget::Directory(_) => Editor::empty(),
        StartupTarget::Scratch => Editor::scratch(),
    }
    .map_err(io::Error::other)?;

    let mut app = App::new(editor);
    match Config::load() {
        Ok(Some(config)) => config.apply(&mut app),
        Ok(None) => {}
        Err(err) => app
            .notifications
            .push(Notification::error(format!("config: {err}"))),
    }
    let mut compositor = Compositor::new();

    // Base layer: the editor surface. Later layers stack on top.
    compositor.push(Box::new(EditorSurface::new()));

    // Directory startup: dock the file explorer (netrw-style), focused.
    if let StartupTarget::Directory(dir) = target {
        match FilesPanel::new(dir.clone()) {
            Ok(panel) => {
                compositor.push(Box::new(panel));
                app.notifications
                    .push(Notification::info(format!("browsing {}", dir.display())));
            }
            Err(err) => app
                .notifications
                .push(Notification::error(format!("files panel: {err}"))),
        }
    }

    app.notifications
        .push(Notification::info("welcome to eggplant-code"));

    Ok((app, compositor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_target_classification() {
        assert!(matches!(
            StartupTarget::resolve(None),
            StartupTarget::Scratch
        ));
        assert!(matches!(
            StartupTarget::resolve(Some(std::env::temp_dir())),
            StartupTarget::Directory(_)
        ));
        let missing = std::env::temp_dir().join("eggplant-definitely-missing-xyz.rs");
        assert!(matches!(
            StartupTarget::resolve(Some(missing)),
            StartupTarget::File(_)
        ));
    }
}
