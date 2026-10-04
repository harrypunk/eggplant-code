//! `:` ex commands — a declarative table of vim-style command-line actions.
//!
//! Parsing: `<name>[!] [args...]` — `!` is a generic "force" flag. Names
//! match by prefix-free alias or full name (first match wins).

use crate::app::App;
use crate::commands;
use crate::compositor::Compositor;
use crate::layers::notification::Notification;
use crate::theme::Theme;

/// A `:` command-line action.
pub struct ExCommand {
    /// Full name (`:write`).
    pub name: &'static str,
    /// Short aliases (`:w`).
    pub aliases: &'static [&'static str],
    pub help: &'static str,
    /// Run with the raw argument string and the `!` flag.
    /// Return `Err(message)` to report failure via notification.
    pub run: fn(&mut App, &mut Compositor, args: &str, bang: bool) -> Result<(), String>,
}

/// The ex-command table.
pub const EX_COMMANDS: &[ExCommand] = &[
    ExCommand {
        name: "write",
        aliases: &["w"],
        help: "Save the current buffer",
        run: |app, compositor, _, _| {
            commands::save_with_notification(app, compositor);
            Ok(())
        },
    },
    ExCommand {
        name: "quit",
        aliases: &["q"],
        help: "Quit (fails on unsaved changes; `q!` forces)",
        run: |app, _, _, bang| {
            if bang {
                app.request_quit();
            } else if app.editor.any_modified() {
                return Err("unsaved changes (use q! to force)".into());
            } else {
                app.request_quit();
            }
            Ok(())
        },
    },
    ExCommand {
        name: "write-quit",
        aliases: &["wq", "x"],
        help: "Save, then quit",
        run: |app, compositor, _, bang| {
            app.editor
                .save()
                .map_err(|err| format!("save failed: {err:#}"))?;
            if bang || !app.editor.any_modified() {
                app.request_quit();
            } else {
                // Other buffers still dirty: fall back to the confirm flow.
                commands::quit(app, compositor);
            }
            Ok(())
        },
    },
    ExCommand {
        name: "edit",
        aliases: &["e"],
        help: "Open a file into a buffer: `:e <path>`",
        run: |app, _, args, _| {
            let path = args.trim();
            if path.is_empty() {
                return Err("usage: :e <path>".into());
            }
            app.editor
                .open_buffer(path)
                .map_err(|err| format!("{err:#}"))
        },
    },
    ExCommand {
        name: "buffer",
        aliases: &["b"],
        help: "Switch to buffer N: `:b <n>`",
        run: |app, _, args, _| {
            let index: usize = args
                .trim()
                .parse()
                .map_err(|_| "usage: :b <number>".to_owned())?;
            app.editor
                .switch_buffer(index.saturating_sub(1))
                .map_err(|err| format!("{err:#}"))
        },
    },
    ExCommand {
        name: "bdelete",
        aliases: &["bd"],
        help: "Close the current buffer (`bd!` discards changes)",
        run: |app, _, _, bang| {
            app.editor
                .close_current_buffer(bang)
                .map_err(|err| format!("{err:#}"))
        },
    },
    ExCommand {
        name: "bnext",
        aliases: &["bn"],
        help: "Switch to next buffer",
        run: |app, _, _, _| {
            app.editor.next_buffer();
            Ok(())
        },
    },
    ExCommand {
        name: "bprev",
        aliases: &["bp"],
        help: "Switch to previous buffer",
        run: |app, _, _, _| {
            app.editor.prev_buffer();
            Ok(())
        },
    },
    ExCommand {
        name: "theme",
        aliases: &[],
        help: "Switch color theme: `:theme <name>`",
        run: |app, _, args, _| {
            let name = args.trim();
            match Theme::by_name(name) {
                Some(theme) => {
                    app.theme = theme;
                    app.notifications
                        .push(Notification::info(format!("theme: {}", theme.name)));
                    Ok(())
                }
                None => Err(format!(
                    "unknown theme '{name}' (available: {})",
                    Theme::available().join(", ")
                )),
            }
        },
    },
    ExCommand {
        name: "ls",
        aliases: &[],
        help: "List open buffers",
        run: |app, _, _, _| {
            let listing = app
                .editor
                .buffers_info()
                .iter()
                .map(|b| {
                    format!(
                        "{}{} {}",
                        if b.current { "%" } else { " " },
                        b.index + 1,
                        b.name
                    ) + if b.modified { " [+]" } else { "" }
                })
                .collect::<Vec<_>>()
                .join("\n");
            app.notifications.push(Notification::info(listing));
            Ok(())
        },
    },
];

/// Parse and run a `:` input line. Errors surface as notifications.
pub fn execute(app: &mut App, compositor: &mut Compositor, input: &str) {
    let input = input.trim();
    if input.is_empty() {
        return;
    }

    let (name, bang) = match input.strip_suffix('!') {
        Some(name) => (name, true),
        None => (input, false),
    };
    let (name, args) = match name.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args),
        None => (name, ""),
    };

    let command = EX_COMMANDS
        .iter()
        .find(|cmd| cmd.name == name || cmd.aliases.contains(&name));
    let result = match command {
        Some(cmd) => (cmd.run)(app, compositor, args, bang),
        None => Err(format!("not an editor command: {name}")),
    };
    if let Err(message) = result {
        app.notifications.push(Notification::error(message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_no_duplicate_names_or_aliases() {
        let mut seen: Vec<&str> = Vec::new();
        for cmd in EX_COMMANDS {
            for name in std::iter::once(cmd.name).chain(cmd.aliases.iter().copied()) {
                assert!(!seen.contains(&name), "duplicate ex name: {name}");
                seen.push(name);
            }
        }
    }
}
