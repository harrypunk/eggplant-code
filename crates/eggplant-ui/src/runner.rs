//! The event loop: render → tick → dispatch events, until quit.
//!
//! Terminal lifecycle lives in `crate::terminal`, startup wiring in
//! `crate::startup`; this module is only the runtime loop.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};
use ratatui::layout::Rect;

use crate::app::App;
use crate::compositor::{Compositor, KeyResult};
use crate::startup::{self, StartupTarget};
use crate::terminal::{CrosstermTerminal, TerminalGuard};

/// Run the app to completion.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    // Boot before entering the terminal: startup errors print normally.
    let (mut app, mut compositor) = startup::boot(&StartupTarget::resolve(path))?;
    let mut terminal = TerminalGuard::enter()?;
    event_loop(terminal.inner(), &mut app, &mut compositor)
}

fn event_loop(
    terminal: &mut CrosstermTerminal,
    app: &mut App,
    compositor: &mut Compositor,
) -> io::Result<()> {
    while !app.is_quitting() {
        render_frame(terminal, compositor, app)?;

        // Tick: drain expired notifications.
        app.notifications.retain_visible();

        if let Some(event) = next_event()? {
            handle_event(event, app, compositor);
        }
    }
    Ok(())
}

/// Pre-render lifecycle + paint: layers update viewport-dependent state from
/// their resolved area, then render is a pure function of state (Rule 5).
fn render_frame(
    terminal: &mut CrosstermTerminal,
    compositor: &mut Compositor,
    app: &App,
) -> io::Result<()> {
    let size = terminal.size()?;
    let area = Rect::new(0, 0, size.width, size.height);
    compositor.resize(area, app);
    terminal.draw(|frame| compositor.render(frame, area, app))?;
    Ok(())
}

/// Wait up to one tick for the next terminal event.
fn next_event() -> io::Result<Option<Event>> {
    if event::poll(Duration::from_millis(250))? {
        event::read().map(Some)
    } else {
        Ok(None)
    }
}

fn handle_event(event: Event, app: &mut App, compositor: &mut Compositor) {
    match event {
        Event::Key(key) => dispatch_key(key, app, compositor),
        Event::Resize(_, _) => {} // redraw happens next loop iteration
        _ => {}
    }
}

/// Key routing: the focused layer gets the key first (modal layers swallow
/// everything); the global keymap is the fallback.
fn dispatch_key(key: KeyEvent, app: &mut App, compositor: &mut Compositor) {
    if matches!(compositor.dispatch_key(key, app), KeyResult::Ignored)
        && let Some(command) = app.registry.lookup_key(&key)
    {
        compositor.execute(command, app);
    }
}
