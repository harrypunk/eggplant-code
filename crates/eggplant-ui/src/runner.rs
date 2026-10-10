//! The event loop: render → tick → dispatch events, until quit.
//!
//! Terminal lifecycle lives in `crate::terminal`, startup wiring in
//! `crate::startup`; this module is only the runtime loop.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often we stat open buffers for external changes. A handful of
/// stats per second is free; a filesystem watcher is not worth it.
const EXTERNAL_POLL: Duration = Duration::from_secs(1);

use crossterm::event::{self, Event};
use ratatui::layout::Rect;

use crate::action::{AppAction, Handled};
use crate::app::App;
use crate::compositor::Compositor;
use crate::startup::{self, StartupTarget};
use crate::terminal::{CrosstermTerminal, TerminalGuard};

/// Run the app to completion.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    // Logging first: everything after this lands in the log file.
    crate::logging::init();
    log::info!("eggplant {} boot, path={path:?}", env!("CARGO_PKG_VERSION"));
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
    let mut last_external_poll = Instant::now();
    while !app.is_quitting() {
        render_frame(terminal, compositor, app)?;

        // Tick: drain expired notifications, re-probe the terminal theme
        // (the backstop for OS light/dark flips while we're focused).
        app.notifications.retain_visible();
        if app.theme.tick() {
            refresh_theme(app, &mut probe);
        }
        drain_agent(app, compositor);
        drain_background(app, compositor);
        // The file watcher: stat open buffers once a second — clean
        // buffers reload, dirty ones get a conflict notice.
        if last_external_poll.elapsed() >= EXTERNAL_POLL {
            last_external_poll = Instant::now();
            poll_external(app, compositor);
            compositor.tick(app);
        }

        if let Some(event) = next_event()? {
            handle_event(event, app, compositor, &mut probe);
        }
    }
    Ok(())
}

/// Poll buffers for external edits (shell, git, another editor — agent
/// edits already come through the facade). Clean reloads broadcast a
/// change; conflicts (dirty buffer + disk change) warn once each.
fn poll_external(app: &mut App, compositor: &mut Compositor) {
    let changes = app.editor.poll_external_changes();
    if changes.is_empty() {
        return;
    }
    let short = |p: &PathBuf| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    for path in &changes.reloaded {
        app.notifications
            .push(crate::layers::notification::Notification::with_level(
                crate::layers::notification::Level::Info,
                format!("{} reloaded (changed on disk)", short(path)),
            ));
    }
    for path in &changes.conflicts {
        app.notifications
            .push(crate::layers::notification::Notification::with_level(
                crate::layers::notification::Level::Warn,
                format!(
                    "{} changed on disk; buffer modified — Space b r to reload",
                    short(path)
                ),
            ));
    }
    if !changes.reloaded.is_empty() {
        compositor.broadcast(crate::action::ActionEvent::BufferChanged, app);
    }
}

/// Background threads (auth validation, …) post data-only events; each
/// maps onto its action in dispatch.
fn drain_background(app: &mut App, compositor: &mut Compositor) {
    while let Some(event) = app.try_recv_bg() {
        match event {
            crate::action::BgEvent::AuthValidated {
                provider,
                key,
                base_url,
                outcome,
                models,
            } => compositor.dispatch(
                AppAction::AuthResult {
                    provider,
                    key,
                    base_url,
                    models,
                    outcome,
                },
                None,
                app,
            ),
            crate::action::BgEvent::ModelsListed { provider, result } => {
                compositor.dispatch(AppAction::ModelsListed { provider, result }, None, app)
            }
        }
    }
}

/// The agent drain: runtime events become dispatched actions; host calls
/// are served synchronously against the live editor (single-threaded
/// mutation), and buffer-changing ones broadcast `BufferChanged` so the
/// explorer/topbar stay truthful.
fn drain_agent(app: &mut App, compositor: &mut Compositor) {
    let Some(session) = app.agent.session() else {
        return;
    };
    // Collect first: try_recv borrows app.agent; serving needs &mut App.
    let mut messages = Vec::new();
    while let Some(message) = session.try_recv() {
        messages.push(message);
    }
    for message in messages {
        match message {
            eggplant_agent::SessionMsg::Event(event) => {
                compositor.dispatch(AppAction::Agent(event), None, app);
            }
            eggplant_agent::SessionMsg::Host(request) => {
                let touches_buffers = matches!(
                    request.call,
                    eggplant_agent::HostCall::Write { .. } | eggplant_agent::HostCall::Edit { .. }
                );
                let result = crate::agent::serve_host(app, &request.call);
                let changed = touches_buffers && result.is_ok();
                request.respond(result);
                if changed {
                    compositor.broadcast(crate::action::ActionEvent::BufferChanged, app);
                }
            }
        }
    }
}

/// Re-derive the theme if the terminal flipped light/dark.
fn refresh_theme(app: &mut App, probe: &mut impl crate::theme::probe::DarknessProbe) {
    if app.theme.refresh(probe) {
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
    if matches!(compositor.dispatch_key(key, app), Handled::Ignored)
        && let Some(command) = app.input.registry.lookup_key(&key)
    {
        compositor.execute(command, app);
    }
}
