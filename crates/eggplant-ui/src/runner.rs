//! The UI runtime: terminal setup/teardown + the event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use eggplant_core::Editor;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::App;
use crate::commands::global_keymap;
use crate::compositor::{Compositor, KeyResult};
use crate::layers::editor::EditorSurface;
use crate::layers::notification::Notification;

type CrosstermTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// Run the app to completion (owns the terminal while running).
pub fn run(editor: Editor) -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, editor);
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

fn event_loop(terminal: &mut CrosstermTerminal, editor: Editor) -> io::Result<()> {
    let mut app = App::new(editor);
    let keymap = global_keymap();
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
                // Routing: the focused layer gets the key first (modal layers
                // swallow everything); the global keymap is the fallback.
                if compositor.dispatch_key(key, &mut app) == KeyResult::Ignored
                    && let Some(command) = keymap.lookup(&key)
                {
                    (command.execute)(&mut app, &mut compositor);
                }
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
