//! The event loop: render → tick → dispatch events, until quit.
//!
//! Terminal lifecycle lives in `crate::terminal`, startup wiring in
//! `crate::startup`; this module is only the runtime loop.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event};
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
    let mut probe = crate::theme::probe::TerminalProbe::new();
    while !app.is_quitting() {
        render_frame(terminal, compositor, app)?;

        // Tick: drain expired notifications, re-probe the terminal theme
        // (the backstop for OS light/dark flips while we're focused).
        app.notifications.retain_visible();
        app.tick_count = app.tick_count.wrapping_add(1);
        if app.tick_count.is_multiple_of(THEME_PROBE_EVERY_TICKS) {
            refresh_theme(app, &mut probe);
        }

        if let Some(event) = next_event()? {
            handle_event(event, app, compositor, &mut probe);
        }
    }
    Ok(())
}

/// A theme probe every ~3s (250ms ticks) — cheap (one OSC 11 round-trip),
/// and only while following a live-switching source.
const THEME_PROBE_EVERY_TICKS: u32 = 12;

/// Re-derive the theme if the terminal flipped light/dark.
fn refresh_theme(app: &mut App, probe: &mut impl crate::theme::probe::DarknessProbe) {
    if let Some(theme) = crate::theme::resolve::refresh(&mut app.theme_follow, probe) {
        app.theme = theme;
        app.notifications
            .push(crate::layers::notification::Notification::info(
                "theme: followed the terminal's light/dark switch",
            ));
    }
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

fn handle_event(
    event: Event,
    app: &mut App,
    compositor: &mut Compositor,
    probe: &mut impl crate::theme::probe::DarknessProbe,
) {
    match event {
        // The ONE place crossterm events become core input (the boundary
        // rule — docs/design/architecture.md). Keys we don't model are
        // dropped.
        Event::Key(key) => {
            if let Some(key) = translate_key(key) {
                dispatch_key(key, app, compositor);
            }
        }
        // Coming back to the editor is the moment a theme flip is most
        // likely to be visible — re-probe.
        Event::FocusGained => refresh_theme(app, probe),
        Event::Resize(_, _) => {} // redraw happens next loop iteration
        _ => {}
    }
}

/// crossterm → core input vocabulary (the shell's only translation).
fn translate_key(key: crossterm::event::KeyEvent) -> Option<eggplant_core::input::KeyEvent> {
    use crossterm::event::{KeyCode as C, KeyModifiers as M};
    use eggplant_core::input::{KeyCode, KeyModifiers};
    let code = match key.code {
        C::Char(c) => KeyCode::Char(c),
        C::Enter => KeyCode::Enter,
        C::Esc => KeyCode::Esc,
        C::Backspace => KeyCode::Backspace,
        C::Delete => KeyCode::Delete,
        C::Left => KeyCode::Left,
        C::Right => KeyCode::Right,
        C::Up => KeyCode::Up,
        C::Down => KeyCode::Down,
        C::Home => KeyCode::Home,
        C::End => KeyCode::End,
        C::PageUp => KeyCode::PageUp,
        C::PageDown => KeyCode::PageDown,
        C::Tab => KeyCode::Tab,
        C::BackTab => KeyCode::BackTab,
        C::F(n) => KeyCode::F(n),
        _ => return None,
    };
    let mut modifiers = KeyModifiers::NONE;
    if key.modifiers.contains(M::CONTROL) {
        modifiers |= KeyModifiers::CONTROL;
    }
    if key.modifiers.contains(M::ALT) {
        modifiers |= KeyModifiers::ALT;
    }
    if key.modifiers.contains(M::SHIFT) {
        modifiers |= KeyModifiers::SHIFT;
    }
    Some(eggplant_core::input::KeyEvent { code, modifiers })
}

/// Key routing: the focused layer gets the key first (modal layers swallow
/// everything); the global keymap is the fallback.
fn dispatch_key(key: eggplant_core::input::KeyEvent, app: &mut App, compositor: &mut Compositor) {
    if matches!(compositor.dispatch_key(key, app), KeyResult::Ignored)
        && let Some(command) = app.registry.lookup_key(&key)
    {
        compositor.execute(command, app);
    }
}
