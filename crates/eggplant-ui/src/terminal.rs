//! Terminal lifecycle: raw mode + alternate screen, restored on drop.
//!
//! `TerminalGuard` is RAII: whatever happens in the event loop — error or
//! panic — the user's terminal is left clean.

use std::io;

use crossterm::event::{
    DisableFocusChange, EnableFocusChange, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub type CrosstermTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// Owns the terminal while the app runs; restores it on drop.
pub struct TerminalGuard {
    terminal: CrosstermTerminal,
}

impl TerminalGuard {
    /// Enter raw mode + alternate screen and create the terminal.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        // Focus events: re-probing the terminal theme on focus-in is how
        // we notice the OS light/dark flipping (no push channel exists).
        execute!(stdout, EnterAlternateScreen, EnableFocusChange)?;
        // Kitty keyboard protocol (best-effort): disambiguated keys give
        // us C-i distinct from Tab (the chat popup shortcut). Terminals
        // without support ignore the push; C-i then degrades to Tab.
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }

    pub fn inner(&mut self) -> &mut CrosstermTerminal {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort restore: we're unwinding, errors here are unactionable.
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableFocusChange
        );
        let _ = self.terminal.show_cursor();
    }
}
