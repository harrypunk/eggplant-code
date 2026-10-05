//! The base editor surface: document text + gutter + modal keys.
//! (The statusline is global chrome — see `crate::statusline`.)
//!
//! Keys are data-driven: `NORMAL_KEYMAP` / `INSERT_KEYMAP` map key strokes to
//! editor commands. Normal mode supports count prefixes (`5j`, `12G`-style).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eggplant_core::{Mode, Motion};
use ratatui::layout::Rect;

use crate::app::{App, Operator};
use crate::commands::KeyStroke;
use crate::components::editor::{self, EditorLine, EditorProps};
use crate::components::welcome::{self, WelcomeProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;
use crate::layers::notification::Notification;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// An editor-mode action. `count` is the parsed count prefix (default 1).
type EditorCommand = fn(&mut App, usize) -> KeyResult;

fn consumed() -> KeyResult {
    KeyResult::Consumed
}

macro_rules! motion {
    ($name:ident, $method:ident) => {
        fn $name(app: &mut App, count: usize) -> KeyResult {
            app.editor.$method(count);
            consumed()
        }
    };
}

motion!(move_left, move_left);
motion!(move_down, move_down);
motion!(move_up, move_up);
motion!(move_right, move_right);
motion!(word_forward, move_word_forward);
motion!(word_end, move_word_end);
motion!(word_backward, move_word_backward);

fn line_start(app: &mut App, _: usize) -> KeyResult {
    app.editor.move_line_start();
    consumed()
}

fn line_end(app: &mut App, _: usize) -> KeyResult {
    app.editor.move_line_end();
    consumed()
}

fn goto_line(app: &mut App, count: usize) -> KeyResult {
    // Bare `G` goes to the last line; `nG` goes to line n.
    if count > 1 {
        app.editor.move_to_line(count - 1);
    } else {
        app.editor.move_last_line();
    }
    consumed()
}

fn delete_char(app: &mut App, _: usize) -> KeyResult {
    app.editor.delete_char_at_cursor();
    consumed()
}

/// Resolve an armed operator against its motion key. Repeated operator
/// (`dd`/`yy`) works linewise; any non-motion key cancels (vim-style).
fn resolve_operator(operator: Operator, count: usize, key: KeyEvent, app: &mut App) -> KeyResult {
    match (operator, plain_char(&key)) {
        (Operator::Delete, Some('d')) => app.editor.delete_line(),
        (Operator::Yank, Some('y')) => app.editor.yank_line(),
        _ => {
            // A motion resolves the operator; anything else cancels it.
            if let Some(motion) = lookup(OPERATOR_MOTIONS, &key) {
                let range = app.editor.operator_range(motion, count);
                match operator {
                    Operator::Delete => app.editor.delete_range(range),
                    Operator::Yank => app.editor.yank_range(range),
                }
            }
        }
    }
    KeyResult::Consumed
}

fn paste_after(app: &mut App, _: usize) -> KeyResult {
    app.editor.paste_after();
    consumed()
}

fn undo(app: &mut App, _: usize) -> KeyResult {
    if !app.editor.undo() {
        app.notifications
            .push(Notification::info("already at oldest change"));
    }
    consumed()
}

fn redo(app: &mut App, _: usize) -> KeyResult {
    if !app.editor.redo() {
        app.notifications
            .push(Notification::info("already at newest change"));
    }
    consumed()
}

fn enter_insert(app: &mut App, _: usize) -> KeyResult {
    app.editor.enter_insert();
    consumed()
}

fn enter_append(app: &mut App, _: usize) -> KeyResult {
    app.editor.enter_append();
    consumed()
}

fn open_below(app: &mut App, _: usize) -> KeyResult {
    app.editor.open_line_below();
    app.editor.enter_insert();
    consumed()
}

fn open_above(app: &mut App, _: usize) -> KeyResult {
    app.editor.open_line_above();
    app.editor.enter_insert();
    consumed()
}

fn enter_visual(app: &mut App, _: usize) -> KeyResult {
    app.editor.enter_visual();
    consumed()
}

fn enter_normal(app: &mut App, _: usize) -> KeyResult {
    app.editor.enter_normal();
    consumed()
}

fn insert_newline(app: &mut App, _: usize) -> KeyResult {
    app.editor.insert_newline();
    consumed()
}

fn delete_backward(app: &mut App, _: usize) -> KeyResult {
    app.editor.delete_backward();
    consumed()
}

fn delete_forward(app: &mut App, _: usize) -> KeyResult {
    app.editor.delete_char_at_cursor();
    consumed()
}

/// Normal-mode bindings. Digits are handled separately (count prefix).
/// `Space` is deliberately unbound here: it falls through to the global
/// keymap, which opens the which-key menu.
static NORMAL_KEYMAP: &[(KeyStroke, EditorCommand)] = &[
    (KeyStroke::char('h'), move_left),
    (KeyStroke::new(KeyCode::Left, KeyModifiers::NONE), move_left),
    (KeyStroke::char('j'), move_down),
    (KeyStroke::new(KeyCode::Down, KeyModifiers::NONE), move_down),
    (KeyStroke::char('k'), move_up),
    (KeyStroke::new(KeyCode::Up, KeyModifiers::NONE), move_up),
    (KeyStroke::char('l'), move_right),
    (
        KeyStroke::new(KeyCode::Right, KeyModifiers::NONE),
        move_right,
    ),
    (KeyStroke::char('w'), word_forward),
    (KeyStroke::char('e'), word_end),
    (KeyStroke::char('b'), word_backward),
    (KeyStroke::char('0'), line_start),
    (KeyStroke::char('$'), line_end),
    (KeyStroke::char('G'), goto_line),
    (KeyStroke::char('x'), delete_char),
    (KeyStroke::char('p'), paste_after),
    (KeyStroke::char('u'), undo),
    (KeyStroke::ctrl('r'), redo),
    (KeyStroke::char('i'), enter_insert),
    (KeyStroke::char('v'), enter_visual),
    (KeyStroke::char('a'), enter_append),
    (KeyStroke::char('o'), open_below),
    (KeyStroke::char('O'), open_above),
];

/// Visual-mode bindings: motions only — they extend the selection.
/// `d`/`x`/`y`/`v`/`Esc` are handled directly in `handle_visual_key`.
static VISUAL_KEYMAP: &[(KeyStroke, EditorCommand)] = &[
    (KeyStroke::char('h'), move_left),
    (KeyStroke::new(KeyCode::Left, KeyModifiers::NONE), move_left),
    (KeyStroke::char('j'), move_down),
    (KeyStroke::new(KeyCode::Down, KeyModifiers::NONE), move_down),
    (KeyStroke::char('k'), move_up),
    (KeyStroke::new(KeyCode::Up, KeyModifiers::NONE), move_up),
    (KeyStroke::char('l'), move_right),
    (
        KeyStroke::new(KeyCode::Right, KeyModifiers::NONE),
        move_right,
    ),
    (KeyStroke::char('w'), word_forward),
    (KeyStroke::char('e'), word_end),
    (KeyStroke::char('b'), word_backward),
    (KeyStroke::char('0'), line_start),
    (KeyStroke::char('$'), line_end),
    (KeyStroke::char('G'), goto_line),
];

/// Insert-mode bindings; unbound plain chars insert themselves.
static INSERT_KEYMAP: &[(KeyStroke, EditorCommand)] = &[
    (
        KeyStroke::new(KeyCode::Esc, KeyModifiers::NONE),
        enter_normal,
    ),
    (
        KeyStroke::new(KeyCode::Enter, KeyModifiers::NONE),
        insert_newline,
    ),
    (
        KeyStroke::new(KeyCode::Backspace, KeyModifiers::NONE),
        delete_backward,
    ),
    (
        KeyStroke::new(KeyCode::Delete, KeyModifiers::NONE),
        delete_forward,
    ),
    (KeyStroke::new(KeyCode::Left, KeyModifiers::NONE), move_left),
    (
        KeyStroke::new(KeyCode::Right, KeyModifiers::NONE),
        move_right,
    ),
    (KeyStroke::new(KeyCode::Up, KeyModifiers::NONE), move_up),
    (KeyStroke::new(KeyCode::Down, KeyModifiers::NONE), move_down),
];

fn lookup<T: Copy>(keymap: &[(KeyStroke, T)], key: &KeyEvent) -> Option<T> {
    keymap
        .iter()
        .find(|(stroke, _)| stroke.matches(key))
        .map(|(_, command)| *command)
}

/// Plain character keys only — Ctrl/Alt combos fall through to globals.
fn plain_char(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(c)
        }
        _ => None,
    }
}

#[derive(Default)]
pub struct EditorSurface {
    /// First visible line (vertical scroll offset).
    scroll: usize,
    /// Document generation we last saw; a change resets the scroll.
    seen_generation: usize,
    /// Editor height from the compositor's `resize` hook, so key handling can
    /// keep the cursor visible. Updated outside `render` (Rule 5).
    viewport_height: usize,
}

/// Motions an operator consumes, mapped to facade motion targets.
static OPERATOR_MOTIONS: &[(KeyStroke, Motion)] = &[
    (KeyStroke::char('w'), Motion::WordForward),
    (KeyStroke::char('e'), Motion::WordEnd),
    (KeyStroke::char('b'), Motion::WordBackward),
    (KeyStroke::char('0'), Motion::LineStart),
    (KeyStroke::char('$'), Motion::LineEnd),
    (KeyStroke::char('h'), Motion::Left),
    (
        KeyStroke::new(KeyCode::Left, KeyModifiers::NONE),
        Motion::Left,
    ),
    (KeyStroke::char('l'), Motion::Right),
    (
        KeyStroke::new(KeyCode::Right, KeyModifiers::NONE),
        Motion::Right,
    ),
];

impl EditorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_cursor_visible(&mut self, app: &App) {
        // Document replaced/switched? Drop per-document state.
        if app.editor.generation() != self.seen_generation {
            self.seen_generation = app.editor.generation();
            self.scroll = 0;
        }
        let cursor_line = app.editor.cursor().0;
        let height = self.viewport_height.max(1);
        if cursor_line < self.scroll {
            self.scroll = cursor_line;
        } else if cursor_line >= self.scroll + height {
            self.scroll = cursor_line + 1 - height;
        }
    }

    // ---- key handling ----

    fn handle_normal_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        // Count prefix: digits accumulate (`0` is a motion when no count yet).
        if let Some(digit @ ('1'..='9' | '0')) = plain_char(&key) {
            let d = digit.to_digit(10).unwrap() as usize;
            if d > 0 || app.pending_count.is_some() {
                app.pending_count = Some(app.pending_count.unwrap_or(0) * 10 + d);
                return KeyResult::Consumed;
            }
        }
        let count = app.pending_count.take().unwrap_or(1);
        // Operator-pending: the next key resolves the operator.
        if let Some((operator, op_count)) = app.pending_operator.take() {
            return resolve_operator(operator, op_count * count, key, app);
        }
        // `d` / `y` arm the operator (count rides along: `2dw` == `d2w`).
        match plain_char(&key) {
            Some('d') => {
                app.pending_operator = Some((Operator::Delete, count));
                return KeyResult::Consumed;
            }
            Some('y') => {
                app.pending_operator = Some((Operator::Yank, count));
                return KeyResult::Consumed;
            }
            _ => {}
        }
        let result = match lookup(NORMAL_KEYMAP, &key) {
            Some(command) => command(app, count),
            None => KeyResult::Ignored,
        };
        if matches!(result, KeyResult::Ignored) {
            app.pending_count = None; // dead key clears a pending count
        }
        result
    }

    /// Visual mode: motions extend the selection (the facade's `set_cursor`
    /// preserves the fixed end), `d`/`x`/`y` act on it, `v`/`Esc` exits.
    fn handle_visual_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        if let Some(digit @ ('1'..='9' | '0')) = plain_char(&key) {
            let d = digit.to_digit(10).unwrap() as usize;
            if d > 0 || app.pending_count.is_some() {
                app.pending_count = Some(app.pending_count.unwrap_or(0) * 10 + d);
                return KeyResult::Consumed;
            }
        }
        let count = app.pending_count.take().unwrap_or(1);
        match plain_char(&key) {
            _ if key.code == KeyCode::Esc => app.editor.enter_normal(),
            Some('v') => app.editor.enter_normal(),
            Some('d') | Some('x') => app.editor.delete_selection(),
            Some('y') => app.editor.yank_selection(),
            _ => {
                let result = match lookup(VISUAL_KEYMAP, &key) {
                    Some(command) => command(app, count),
                    None => KeyResult::Ignored,
                };
                if matches!(result, KeyResult::Ignored) {
                    app.pending_count = None;
                }
                return result;
            }
        }
        KeyResult::Consumed
    }

    fn handle_insert_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        match lookup(INSERT_KEYMAP, &key) {
            Some(command) => command(app, 1),
            None => match plain_char(&key) {
                Some(c) => {
                    app.editor.insert_char(c);
                    KeyResult::Consumed
                }
                None => KeyResult::Ignored,
            },
        }
    }
}

impl Layer for EditorSurface {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        if !app.editor.has_buffer() {
            return welcome::view(&WelcomeProps { version: VERSION }, area, &app.theme);
        }
        editor::view(
            &EditorProps {
                lines: (self.scroll..self.scroll + area.height as usize)
                    .map(|line| EditorLine {
                        spans: app.editor.highlighted_line(line),
                        selection: app.editor.visual_selection_on_line(line),
                    })
                    .collect(),
                scroll: self.scroll,
                line_count: app.editor.line_count(),
                cursor: app.editor.cursor(),
            },
            area,
            &app.theme,
        )
    }

    fn resize(&mut self, area: Rect, app: &App) {
        self.viewport_height = area.height as usize;
        self.ensure_cursor_visible(app);
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        let result = match app.editor.mode() {
            Mode::Normal => self.handle_normal_key(key, app),
            Mode::Insert => self.handle_insert_key(key, app),
            Mode::Visual => self.handle_visual_key(key, app),
        };
        self.ensure_cursor_visible(app);
        result
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Base
    }

    fn id(&self) -> &'static str {
        "editor"
    }
}
