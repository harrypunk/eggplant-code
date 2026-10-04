//! Terminal lifecycle: raw mode + alternate screen, restored on drop.
//!
//! `TerminalGuard` is RAII: whatever happens in the event loop — error or
//! panic — the user's terminal is left clean.

use std::io;

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
        execute!(stdout, EnterAlternateScreen)?;
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
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
