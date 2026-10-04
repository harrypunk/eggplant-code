//! The UI runtime: terminal setup/teardown + the event loop.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use eggplant_core::Editor;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;

use crate::app::App;
use crate::compositor::{Compositor, KeyResult};
use crate::layers::editor::EditorSurface;
use crate::layers::files_panel::FilesPanel;
use crate::layers::notification::Notification;

type CrosstermTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// What `eggplant [path]` was pointed at.
#[derive(Debug)]
enum StartupTarget {
    /// No argument: empty scratch buffer.
    Scratch,
    /// A file to edit.
    File(PathBuf),
    /// A directory: scratch buffer + file explorer rooted there (netrw-style).
    Directory(PathBuf),
}

impl StartupTarget {
    fn resolve(path: Option<PathBuf>) -> Self {
        match path {
            None => Self::Scratch,
            Some(path) if path.is_dir() => Self::Directory(path.canonicalize().unwrap_or(path)),
            Some(path) => Self::File(path),
        }
    }
}

/// Run the app to completion (owns the terminal while running).
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, StartupTarget::resolve(path));
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> io::Result<CrosstermTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut CrosstermTerminal) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

fn event_loop(terminal: &mut CrosstermTerminal, target: StartupTarget) -> io::Result<()> {
    let editor = match &target {
        StartupTarget::File(path) => Editor::open(path),
        _ => Editor::scratch(),
    }
    .map_err(io::Error::other)?;
    let mut app = App::new(editor);
    let mut compositor = Compositor::new();

    // Base layer: the editor surface. Later layers stack on top.
    compositor.push(Box::new(EditorSurface::new()));

    // Directory startup: dock the file explorer (netrw-style), focused.
    if let StartupTarget::Directory(dir) = &target {
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

    loop {
        // Pre-render lifecycle: layers update viewport-dependent state from
        // their resolved area, then render stays a pure function of state.
        let size = terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        compositor.resize(area, &app);
        terminal.draw(|frame| compositor.render(frame, area, &app))?;

        // Drain expired notifications each tick.
        app.notifications.retain_visible();

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                // Routing: the focused layer gets the key first (modal layers
                // swallow everything); the global keymap is the fallback.
                if matches!(compositor.dispatch_key(key, &mut app), KeyResult::Ignored)
                    && let Some(command) = app.registry.lookup_key(&key)
                {
                    (command.execute)(&mut app, &mut compositor);
                }
                if app.is_quitting() {
                    break;
                }
            }
            Event::Resize(_, _) => {} // redraw happens next loop iteration
            _ => {}
        }
    }
    Ok(())
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
