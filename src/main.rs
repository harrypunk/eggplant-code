//! eggplant-code — M0 skeleton
//!
//! Event loop + minimal compositor:
//! - editor surface (placeholder text buffer)
//! - floating dialog layer (`d` to toggle)
//! - notification toasts (`n` to spawn)
//! - `q` / `Esc` quits (Esc closes top layer first)

mod app;
mod compositor;
mod layers;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use crate::app::App;
use crate::compositor::Compositor;
use crate::layers::dialog::Dialog;
use crate::layers::editor::EditorSurface;
use crate::layers::notification::{Notification, Notifications};

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
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

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();
    let mut compositor = Compositor::new();

    // Base layer: the editor surface. Later layers stack on top.
    compositor.push(Box::new(EditorSurface::new()));
    // Notifications render above everything but never take focus.
    let mut notifications = Notifications::new();

    notifications.push(Notification::info("welcome to eggplant-code (M0)"));

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            compositor.render(frame, area, &app);
            notifications.render(frame, area);
        })?;

        // Drain expired notifications each tick.
        notifications.retain_visible();

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if handle_global_key(key, &mut app, &mut compositor, &mut notifications) {
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
/// Focus model for M0: the topmost layer gets keys first; if it doesn't
/// consume them, they fall through to global handling here.
fn handle_global_key(
    key: KeyEvent,
    app: &mut App,
    compositor: &mut Compositor,
    notifications: &mut Notifications,
) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return true;
    }

    // Let the focused (top) layer handle the key first.
    if compositor.dispatch_key(key, app) {
        return false;
    }

    match key.code {
        KeyCode::Esc if compositor.has_dialog() => compositor.pop(),
        KeyCode::Char('q') => return true,
        KeyCode::Char('d') => {
            if compositor.has_dialog() {
                compositor.pop();
            } else {
                compositor.push(Box::new(Dialog::new(
                    "dialog",
                    "floating layers work.\n\n`d` or `Esc` closes me.",
                )));
            }
        }
        KeyCode::Char('n') => {
            app.tick_count += 1;
            notifications.push(Notification::info(format!(
                "notification #{}",
                app.tick_count
            )));
        }
        _ => {}
    }
    false
}
