//! eggplant-code — event loop + compositor wiring.
//!
//! - base layer: editor surface (helix-core document, modal editing)
//! - `Ctrl-E`: toggle file-explorer panel; `Ctrl-W`: cycle focus
//! - floating dialog layer (`F2` demo), notification toasts (`F3` demo)
//! - `Ctrl-Q` quits (confirms on unsaved changes); `Ctrl-C` force-quits

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use eggplant_core::Editor;
use eggplant_ui::app::App;
use eggplant_ui::compositor::{Compositor, KeyResult};
use eggplant_ui::layers::dialog::{ConfirmDialog, Dialog};
use eggplant_ui::layers::editor::EditorSurface;
use eggplant_ui::layers::files_panel::{self, FilesPanel};
use eggplant_ui::layers::notification::Notification;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

fn main() -> io::Result<()> {
    let editor = match std::env::args().nth(1).map(PathBuf::from) {
        Some(path) => Editor::open(&path),
        None => Editor::scratch(),
    }
    .map_err(io::Error::other)?;

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, editor);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, editor: Editor) -> io::Result<()> {
    let mut app = App::new(editor);
    let mut compositor = Compositor::new();

    // Base layer: the editor surface. Later layers stack on top.
    compositor.push(Box::new(EditorSurface::new()));

    app.notifications
        .push(Notification::info("welcome to eggplant-code"));

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            compositor.render(frame, area, &app);
            app.notifications.render(frame, area);
        })?;

        // Drain expired notifications each tick.
        app.notifications.retain_visible();

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                handle_key(key, &mut app, &mut compositor);
                if app.should_quit {
                    break;
                }
            }
            Event::Resize(_, _) => {} // redraw happens next loop iteration
            _ => {}
        }
    }
    Ok(())
}

/// Key routing: the focused layer gets keys first (modal layers swallow
/// everything); whatever falls through is handled by global bindings here.
fn handle_key(key: KeyEvent, app: &mut App, compositor: &mut Compositor) {
    if compositor.dispatch_key(key, app) != KeyResult::Ignored {
        return;
    }

    let ctrl =
        |c: char| key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(c);

    if ctrl('c') {
        app.should_quit = true; // force quit, no confirm
    } else if ctrl('q') {
        if app.editor.is_modified() {
            compositor.push(Box::new(ConfirmDialog::new(
                "unsaved changes",
                "Quit without saving?",
                |app: &mut App| app.should_quit = true,
            )));
        } else {
            app.should_quit = true;
        }
    } else if ctrl('e') {
        if compositor.has(files_panel::PANEL_ID) {
            compositor.remove_by_id(files_panel::PANEL_ID);
        } else {
            match FilesPanel::new(std::env::current_dir().unwrap_or_default()) {
                Ok(panel) => compositor.push(Box::new(panel)),
                Err(err) => app
                    .notifications
                    .push(Notification::error(format!("files panel: {err}"))),
            }
        }
    } else if ctrl('w') {
        compositor.focus_next();
    } else {
        match key.code {
            KeyCode::F(2) => {
                if compositor.has("dialog") {
                    compositor.remove_by_id("dialog");
                } else {
                    compositor.push(Box::new(Dialog::new(
                        "dialog",
                        "floating layers work.\n\n`F2` or `Esc` closes me.",
                    )));
                }
            }
            KeyCode::F(3) => {
                app.tick_count += 1;
                app.notifications.push(Notification::info(format!(
                    "notification #{}",
                    app.tick_count
                )));
            }
            _ => {}
        }
    }
}
