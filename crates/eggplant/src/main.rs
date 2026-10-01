//! eggplant-code — event loop + compositor wiring.
//!
//! - base layer: editor surface (helix-core document, modal editing)
//! - floating dialog layer (`F2` to toggle, demo)
//! - notification toasts (`F3` to spawn, demo)
//! - `Ctrl-C`/`Ctrl-Q` quits; `Ctrl-S` saves

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
use eggplant_ui::layers::dialog::Dialog;
use eggplant_ui::layers::editor::EditorSurface;
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
                if handle_global_key(key, &mut app, &mut compositor) {
                    break;
                }
            }
            Event::Resize(_, _) => {} // redraw happens next loop iteration
            _ => {}
        }
    }
    Ok(())
}

/// Global keys. Returns `true` when the app should quit.
///
/// Focus model: the topmost layer gets keys first; if it doesn't consume
/// them, they fall through to global handling here.
fn handle_global_key(key: KeyEvent, app: &mut App, compositor: &mut Compositor) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'))
    {
        return true;
    }

    // Let the focused (top) layer handle the key first.
    if compositor.dispatch_key(key, app) != KeyResult::Ignored {
        return false;
    }

    match key.code {
        KeyCode::F(2) => {
            if compositor.has_dialog() {
                compositor.pop();
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
    false
}
