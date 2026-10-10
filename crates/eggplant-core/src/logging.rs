//! Application logging: the `log` facade + our own file writer.
//!
//! Choices, deliberately:
//! - `log`, not `tracing`: we're a sync TUI with one small runtime;
//!   spans/subscribers are weight we don't need. The facade gives us
//!   `log::info!` anywhere + runtime-adjustable levels.
//! - One file, appended, at `<data_root>/logs/eggplant.log`, with a
//!   session header per start and a size cap (keep the tail) so a crash
//!   loop can't fill the disk.
//! - A panic hook writes panic + backtrace to the same file — a crash
//!   in the TUI leaves no stderr to read; this is the crash record.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use log::{LevelFilter, Log, Metadata, Record};

/// Keep at most this much of an old log across restarts (tail kept).
const MAX_BYTES: u64 = 1_000_000;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

struct FileLogger {
    file: Mutex<std::fs::File>,
    /// Uptime stamps: readable and ordering-preserving within a session.
    started: Instant,
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true // the level filter lives in log::max_level
    }

    fn log(&self, record: &Record) {
        let Ok(mut file) = self.file.lock() else {
            return; // poisoned (a panic mid-write): the panic hook covers it
        };
        let uptime = self.started.elapsed().as_secs_f64();
        let _ = writeln!(
            file,
            "[{uptime:9.3}] {:<5} {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {}
}

/// The log file path (`<data_root>/logs/eggplant.log`).
pub fn path(data_root: &std::path::Path) -> PathBuf {
    data_root.join("logs").join("eggplant.log")
}

/// Install the logger + panic hook. Returns the log path (for
/// `app.logs`). Call once at startup, before the terminal is entered.
pub fn init(data_root: &std::path::Path) -> Option<PathBuf> {
    let path = path(data_root);
    std::fs::create_dir_all(path.parent()?).ok()?;
    cap_tail(&path);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    let logger = LOGGER.get_or_init(|| FileLogger {
        file: Mutex::new(file),
        started: Instant::now(),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Trace);
    }
    log::info!("── session start ({:?}) ──", std::time::SystemTime::now());
    install_panic_hook(path.clone());
    Some(path)
}

/// A panicking TUI can't print to the terminal — record the crash in
/// the log (with backtrace), then let the default hook run (the guard
/// restores the terminal during unwinding).
fn install_panic_hook(path: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
            let _ = writeln!(file, "── PANIC: {info}");
            let _ = writeln!(file, "{}", std::backtrace::Backtrace::force_capture());
        }
        default(info);
    }));
}

/// Over the cap? Keep only the tail (crash logs live at the end).
fn cap_tail(path: &PathBuf) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= MAX_BYTES {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let keep = &bytes[bytes.len() - (MAX_BYTES / 2) as usize..];
    let start = keep
        .iter()
        .position(|b| *b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let _ = std::fs::write(path, &keep[start..]);
}

/// The level one log line carries (None = header/continuation line,
/// always kept so panics and session markers survive filtering).
fn line_level(line: &str) -> Option<log::Level> {
    let after = line.strip_prefix('[')?.split_once("] ")?.1;
    after.split_whitespace().next()?.parse().ok()
}

/// The log filtered to `min` and above (the viewer's levels).
pub fn filter_level(content: &str, min: log::LevelFilter) -> String {
    content
        .lines()
        .filter(|line| match line_level(line) {
            Some(level) => level <= min,
            None => true,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn filter_keeps_levels_at_or_above_and_continuations() {
        let log = "[  0.001] INFO  boot: hello\n\
── session start ──
[  0.010] DEBUG detail: x
[  0.020] WARN  disk: slow
[  0.030] ERROR agent: boom
  backtrace line";
        let errors = super::filter_level(log, log::LevelFilter::Warn);
        assert!(errors.contains("WARN"));
        assert!(errors.contains("ERROR"));
        assert!(!errors.contains("DEBUG"), "{errors}");
        assert!(!errors.contains("INFO"));
        assert!(errors.contains("session start"), "headers stay");
        assert!(errors.contains("backtrace"), "continuations stay");
    }
}
