//! Modal editing, Redux-style: keys resolve to **actions** (data), actions
//! are **interpreted** against a narrow context (interface).
//!
//! Three separated concerns:
//! - keymaps (`NORMAL_KEYMAP` / `VISUAL_KEYMAP` / `INSERT_KEYMAP`) are pure
//!   tables — binding *policy*, later loadable from a config file;
//! - [`resolve`] is the input state machine (count prefixes, armed
//!   operators) — the only place pending input lives;
//! - [`interpret`] executes one action against [`EditorCtx`] — editing
//!   *semantics*, one flat match, testable without `App`.

use crate::input::{KeyCode, KeyEvent, KeyModifiers};
use crate::{Editor, Mode, Motion};

use crate::input::KeyStroke;

/// Normal-mode keys that wait for a second key: the `d`/`y` operators and
/// the `g` prefix (`gg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKey {
    Delete,
    Yank,
    Goto,
    /// `z` prefix: view commands (`zz` center…). Not an operator.
    View,
}

impl PendingKey {
    /// The key that arms it (for the pending hint).
    pub fn key(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Yank => 'y',
            Self::Goto => 'g',
            Self::View => 'z',
        }
    }
}

/// Pending modal input: count prefix and armed operator + its count.
/// `Copy`: `resolve` is pure — it takes a snapshot and returns the next
/// state alongside the resolution (the action-flow contract).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PendingState {
    pub count: Option<usize>,
    pub key: Option<(PendingKey, usize)>,
}

impl PendingState {
    /// Vim `showcmd`-style hint: `"5"` for a bare count, `"d"` / `"d2"` for
    /// an armed operator.
    pub fn hint(&self) -> Option<String> {
        let mut hint = String::new();
        if let Some((operator, count)) = self.key {
            hint.push(operator.key());
            if count > 1 {
                hint.push_str(&count.to_string());
            }
        }
        if let Some(count) = self.count {
            hint.push_str(&count.to_string());
        }
        (!hint.is_empty()).then_some(hint)
    }
}

/// A semantic editing intent — what a key *means*, independent of which key
/// produced it. The closed set of things the editor can be asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorAction {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    WordForward,
    WordEnd,
    WordBackward,
    LineStart,
    LineEnd,
    GotoBottom,
    /// `gg`: first line, or line `count` with a count.
    MoveFirstLine,
    DeleteChar,
    DeleteLine,
    YankLine,
    PasteAfter,
    NextSearchMatch,
    PrevSearchMatch,
    ClearSearch,
    Undo,
    Redo,
    EnterInsert,
    EnterAppend,
    OpenBelow,
    OpenAbove,
    EnterVisual,
    EnterVisualLine,
    ExitToNormal,
    /// `v` in visual mode: switch charwise/linewise, or exit (vim toggle).
    VisualCharOrExit,
    VisualLineOrExit,
    DeleteSelection,
    YankSelection,
    Newline,
    DeleteBackward,
    DeleteForward,
}

impl EditorAction {
    /// Every action — the registry auto-registers these as `edit.*`
    /// commands, so palette/keymaps/modal keys share one dispatch path.
    pub const ALL: &'static [EditorAction] = &[
        EditorAction::MoveLeft,
        EditorAction::MoveRight,
        EditorAction::MoveUp,
        EditorAction::MoveDown,
        EditorAction::WordForward,
        EditorAction::WordEnd,
        EditorAction::WordBackward,
        EditorAction::LineStart,
        EditorAction::LineEnd,
        EditorAction::GotoBottom,
        EditorAction::MoveFirstLine,
        EditorAction::DeleteChar,
        EditorAction::DeleteLine,
        EditorAction::YankLine,
        EditorAction::PasteAfter,
        EditorAction::NextSearchMatch,
        EditorAction::PrevSearchMatch,
        EditorAction::ClearSearch,
        EditorAction::Undo,
        EditorAction::Redo,
        EditorAction::EnterInsert,
        EditorAction::EnterAppend,
        EditorAction::OpenBelow,
        EditorAction::OpenAbove,
        EditorAction::EnterVisual,
        EditorAction::EnterVisualLine,
        EditorAction::ExitToNormal,
        EditorAction::VisualCharOrExit,
        EditorAction::VisualLineOrExit,
        EditorAction::DeleteSelection,
        EditorAction::YankSelection,
        EditorAction::Newline,
        EditorAction::DeleteBackward,
        EditorAction::DeleteForward,
    ];

    /// Stable registry id (`keys.toml` will bind these by name).
    pub fn id(self) -> &'static str {
        match self {
            EditorAction::MoveLeft => "edit.move-left",
            EditorAction::MoveRight => "edit.move-right",
            EditorAction::MoveUp => "edit.move-up",
            EditorAction::MoveDown => "edit.move-down",
            EditorAction::WordForward => "edit.word-forward",
            EditorAction::WordEnd => "edit.word-end",
            EditorAction::WordBackward => "edit.word-backward",
            EditorAction::LineStart => "edit.line-start",
            EditorAction::LineEnd => "edit.line-end",
            EditorAction::GotoBottom => "edit.goto-bottom",
            EditorAction::MoveFirstLine => "edit.goto-first-line",
            EditorAction::DeleteChar => "edit.delete-char",
            EditorAction::DeleteLine => "edit.delete-line",
            EditorAction::YankLine => "edit.yank-line",
            EditorAction::PasteAfter => "edit.paste-after",
            EditorAction::NextSearchMatch => "edit.next-search-match",
            EditorAction::PrevSearchMatch => "edit.prev-search-match",
            EditorAction::ClearSearch => "edit.clear-search",
            EditorAction::Undo => "edit.undo",
            EditorAction::Redo => "edit.redo",
            EditorAction::EnterInsert => "edit.enter-insert",
            EditorAction::EnterAppend => "edit.enter-append",
            EditorAction::OpenBelow => "edit.open-below",
            EditorAction::OpenAbove => "edit.open-above",
            EditorAction::EnterVisual => "edit.enter-visual",
            EditorAction::EnterVisualLine => "edit.enter-visual-line",
            EditorAction::ExitToNormal => "edit.exit-to-normal",
            EditorAction::VisualCharOrExit => "edit.visual-char-or-exit",
            EditorAction::VisualLineOrExit => "edit.visual-line-or-exit",
            EditorAction::DeleteSelection => "edit.delete-selection",
            EditorAction::YankSelection => "edit.yank-selection",
            EditorAction::Newline => "edit.newline",
            EditorAction::DeleteBackward => "edit.delete-backward",
            EditorAction::DeleteForward => "edit.delete-forward",
        }
    }

    /// Human description (palette/which-key).
    pub fn description(self) -> &'static str {
        match self {
            EditorAction::MoveLeft => "Move left",
            EditorAction::MoveRight => "Move right",
            EditorAction::MoveUp => "Move up",
            EditorAction::MoveDown => "Move down",
            EditorAction::WordForward => "Word forward",
            EditorAction::WordEnd => "Word end",
            EditorAction::WordBackward => "Word backward",
            EditorAction::LineStart => "Line start",
            EditorAction::LineEnd => "Line end",
            EditorAction::GotoBottom => "Goto bottom",
            EditorAction::MoveFirstLine => "Goto first line",
            EditorAction::DeleteChar => "Delete char",
            EditorAction::DeleteLine => "Delete line",
            EditorAction::YankLine => "Yank line",
            EditorAction::PasteAfter => "Paste after",
            EditorAction::NextSearchMatch => "Next search match",
            EditorAction::PrevSearchMatch => "Previous search match",
            EditorAction::ClearSearch => "Clear search highlight",
            EditorAction::Undo => "Undo",
            EditorAction::Redo => "Redo",
            EditorAction::EnterInsert => "Enter insert mode",
            EditorAction::EnterAppend => "Enter insert mode (append)",
            EditorAction::OpenBelow => "Open line below",
            EditorAction::OpenAbove => "Open line above",
            EditorAction::EnterVisual => "Enter visual mode",
            EditorAction::EnterVisualLine => "Enter visual line mode",
            EditorAction::ExitToNormal => "Exit to normal mode",
            EditorAction::VisualCharOrExit => "Visual charwise / exit",
            EditorAction::VisualLineOrExit => "Visual linewise / exit",
            EditorAction::DeleteSelection => "Delete selection",
            EditorAction::YankSelection => "Yank selection",
            EditorAction::Newline => "Insert newline",
            EditorAction::DeleteBackward => "Delete backward",
            EditorAction::DeleteForward => "Delete forward",
        }
    }
}

/// What a keypress resolved to. Payload-carrying outcomes live here (not
/// in `EditorAction`) so the action set stays closed and registrable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    /// Execute this action with this count.
    Act(EditorAction, usize),
    /// A view (viewport) intent — interpreted by the editor surface, which
    /// owns scroll state; `interpret_resolved` never sees these.
    View(ViewAction),
    /// Insert-mode plain char.
    Insert(char),
    /// Operator + motion resolved (`dw`…): delete/yank the motion's range.
    DeleteMotion(Motion, usize),
    YankMotion(Motion, usize),
    /// Input swallowed without an action: digit accumulated, operator armed
    /// or cancelled.
    Swallowed,
    /// Not ours — fall through (e.g. `Space` to the global keymap).
    Ignored,
}

/// Viewport intents (`zz`): the closed set of view commands. Buffer-agnostic
/// — they move the window over the text, never the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewAction {
    /// Center the cursor line vertically (`zz`).
    CenterCursor,
    /// One page down (`C-f`): cursor and scroll follow, vim-style.
    PageDown,
    /// One page up (`C-u`).
    PageUp,
}

/// Single-stroke view keys (not sequences — those go through the pending
/// machine). No count: `5 C-f` is just `C-f`.
const VIEW_KEYS: &[(KeyStroke, ViewAction)] = &[
    (KeyStroke::ctrl('f'), ViewAction::PageDown),
    (KeyStroke::ctrl('u'), ViewAction::PageUp),
];

/// The narrow context actions execute against (interface segregation):
/// editing semantics need the buffer facade and a notification sink —
/// nothing else.
pub trait EditorCtx {
    fn editor(&mut self) -> &mut Editor;
    fn notify(&mut self, message: &str);
}

/// Execute one action. All arms converge on the facade; the multi-step ones
/// (`open_below`, visual flavor toggles) live here and nowhere else.
pub fn interpret(action: EditorAction, count: usize, ctx: &mut impl EditorCtx) {
    match action {
        EditorAction::MoveLeft => ctx.editor().move_left(count),
        EditorAction::MoveRight => ctx.editor().move_right(count),
        EditorAction::MoveUp => ctx.editor().move_up(count),
        EditorAction::MoveDown => ctx.editor().move_down(count),
        EditorAction::WordForward => ctx.editor().move_word_forward(count),
        EditorAction::WordEnd => ctx.editor().move_word_end(count),
        EditorAction::WordBackward => ctx.editor().move_word_backward(count),
        EditorAction::LineStart => ctx.editor().move_line_start(),
        EditorAction::LineEnd => ctx.editor().move_line_end(),
        EditorAction::GotoBottom => ctx.editor().move_last_line(),
        EditorAction::MoveFirstLine => ctx.editor().move_first_line(count),
        EditorAction::DeleteChar | EditorAction::DeleteForward => {
            ctx.editor().delete_char_at_cursor()
        }
        EditorAction::DeleteLine => ctx.editor().delete_line(),
        EditorAction::YankLine => ctx.editor().yank_line(),
        EditorAction::PasteAfter => ctx.editor().paste_after(),
        EditorAction::NextSearchMatch => {
            for _ in 0..count {
                ctx.editor().next_search_match();
            }
        }
        EditorAction::PrevSearchMatch => {
            for _ in 0..count {
                ctx.editor().prev_search_match();
            }
        }
        EditorAction::ClearSearch => ctx.editor().clear_search(),
        EditorAction::Undo => {
            if !ctx.editor().undo() {
                ctx.notify("already at oldest change");
            }
        }
        EditorAction::Redo => {
            if !ctx.editor().redo() {
                ctx.notify("already at newest change");
            }
        }
        EditorAction::EnterInsert => ctx.editor().enter_insert(),
        EditorAction::EnterAppend => ctx.editor().enter_append(),
        EditorAction::OpenBelow => {
            ctx.editor().open_line_below();
            ctx.editor().enter_insert();
        }
        EditorAction::OpenAbove => {
            ctx.editor().open_line_above();
            ctx.editor().enter_insert();
        }
        EditorAction::EnterVisual => ctx.editor().enter_visual(),
        EditorAction::EnterVisualLine => ctx.editor().enter_visual_line(),
        EditorAction::ExitToNormal => ctx.editor().enter_normal(),
        EditorAction::VisualCharOrExit => match ctx.editor().mode() {
            Mode::VisualLine => ctx.editor().enter_visual(),
            _ => ctx.editor().enter_normal(),
        },
        EditorAction::VisualLineOrExit => match ctx.editor().mode() {
            Mode::VisualLine => ctx.editor().enter_normal(),
            _ => ctx.editor().enter_visual_line(),
        },
        EditorAction::DeleteSelection => ctx.editor().delete_selection(),
        EditorAction::YankSelection => ctx.editor().yank_selection(),
        EditorAction::Newline => ctx.editor().insert_newline(),
        EditorAction::DeleteBackward => ctx.editor().delete_backward(),
    }
}

/// Execute a resolution with a payload (or a plain action). The single
/// funnel for edit semantics: the modal layer calls this, and so does the
/// registry's `CommandKind::Edit` dispatch — nothing else touches the
/// facade from the UI crate.
pub fn interpret_resolved(resolved: Resolved, ctx: &mut impl EditorCtx) {
    match resolved {
        Resolved::Act(action, count) => interpret(action, count, ctx),
        Resolved::Insert(c) => ctx.editor().insert_char(c),
        Resolved::DeleteMotion(motion, count) => {
            let range = ctx.editor().operator_range(motion, count);
            ctx.editor().delete_range(range);
        }
        Resolved::YankMotion(motion, count) => {
            let range = ctx.editor().operator_range(motion, count);
            ctx.editor().yank_range(range);
        }
        Resolved::Swallowed | Resolved::Ignored => {} // key-level outcomes
        Resolved::View(_) => {} // interpreted by the editor surface (viewport owner)
    }
}

// ---- keymaps (binding policy as data) ----

const NONE: KeyModifiers = KeyModifiers::NONE;

/// Normal mode. Digits, `d`/`y`/`g` arming are resolved before lookup.
/// `Space` is deliberately unbound: it falls through to the global keymap.
static NORMAL_KEYMAP: &[(KeyStroke, EditorAction)] = &[
    (KeyStroke::char('h'), EditorAction::MoveLeft),
    (KeyStroke::new(KeyCode::Left, NONE), EditorAction::MoveLeft),
    (KeyStroke::char('j'), EditorAction::MoveDown),
    (KeyStroke::new(KeyCode::Down, NONE), EditorAction::MoveDown),
    (KeyStroke::char('k'), EditorAction::MoveUp),
    (KeyStroke::new(KeyCode::Up, NONE), EditorAction::MoveUp),
    (KeyStroke::char('l'), EditorAction::MoveRight),
    (
        KeyStroke::new(KeyCode::Right, NONE),
        EditorAction::MoveRight,
    ),
    (KeyStroke::char('w'), EditorAction::WordForward),
    (KeyStroke::char('e'), EditorAction::WordEnd),
    (KeyStroke::char('b'), EditorAction::WordBackward),
    (KeyStroke::char('0'), EditorAction::LineStart),
    (KeyStroke::char('$'), EditorAction::LineEnd),
    (KeyStroke::char('G'), EditorAction::GotoBottom),
    (KeyStroke::char('x'), EditorAction::DeleteChar),
    (KeyStroke::char('p'), EditorAction::PasteAfter),
    (KeyStroke::char('n'), EditorAction::NextSearchMatch),
    (KeyStroke::char('N'), EditorAction::PrevSearchMatch),
    (KeyStroke::char('u'), EditorAction::Undo),
    (KeyStroke::ctrl('r'), EditorAction::Redo),
    (KeyStroke::char('i'), EditorAction::EnterInsert),
    (KeyStroke::char('v'), EditorAction::EnterVisual),
    (KeyStroke::char('V'), EditorAction::EnterVisualLine),
    (KeyStroke::char('a'), EditorAction::EnterAppend),
    (KeyStroke::char('o'), EditorAction::OpenBelow),
    (KeyStroke::char('O'), EditorAction::OpenAbove),
    (
        KeyStroke::new(KeyCode::Esc, NONE),
        EditorAction::ClearSearch,
    ),
];

/// Visual mode: motions extend the selection; `d`/`x`/`y` act on it.
static VISUAL_KEYMAP: &[(KeyStroke, EditorAction)] = &[
    (KeyStroke::char('h'), EditorAction::MoveLeft),
    (KeyStroke::new(KeyCode::Left, NONE), EditorAction::MoveLeft),
    (KeyStroke::char('j'), EditorAction::MoveDown),
    (KeyStroke::new(KeyCode::Down, NONE), EditorAction::MoveDown),
    (KeyStroke::char('k'), EditorAction::MoveUp),
    (KeyStroke::new(KeyCode::Up, NONE), EditorAction::MoveUp),
    (KeyStroke::char('l'), EditorAction::MoveRight),
    (
        KeyStroke::new(KeyCode::Right, NONE),
        EditorAction::MoveRight,
    ),
    (KeyStroke::char('w'), EditorAction::WordForward),
    (KeyStroke::char('e'), EditorAction::WordEnd),
    (KeyStroke::char('b'), EditorAction::WordBackward),
    (KeyStroke::char('0'), EditorAction::LineStart),
    (KeyStroke::char('$'), EditorAction::LineEnd),
    (KeyStroke::char('G'), EditorAction::GotoBottom),
    (KeyStroke::char('d'), EditorAction::DeleteSelection),
    (KeyStroke::char('x'), EditorAction::DeleteSelection),
    (KeyStroke::char('y'), EditorAction::YankSelection),
    (KeyStroke::char('v'), EditorAction::VisualCharOrExit),
    (KeyStroke::char('V'), EditorAction::VisualLineOrExit),
    (
        KeyStroke::new(KeyCode::Esc, NONE),
        EditorAction::ExitToNormal,
    ),
];

/// Insert mode; unbound plain chars insert themselves.
static INSERT_KEYMAP: &[(KeyStroke, EditorAction)] = &[
    (
        KeyStroke::new(KeyCode::Esc, NONE),
        EditorAction::ExitToNormal,
    ),
    (KeyStroke::new(KeyCode::Enter, NONE), EditorAction::Newline),
    (
        KeyStroke::new(KeyCode::Backspace, NONE),
        EditorAction::DeleteBackward,
    ),
    (
        KeyStroke::new(KeyCode::Delete, NONE),
        EditorAction::DeleteForward,
    ),
    (KeyStroke::new(KeyCode::Left, NONE), EditorAction::MoveLeft),
    (
        KeyStroke::new(KeyCode::Right, NONE),
        EditorAction::MoveRight,
    ),
    (KeyStroke::new(KeyCode::Up, NONE), EditorAction::MoveUp),
    (KeyStroke::new(KeyCode::Down, NONE), EditorAction::MoveDown),
];

/// Motions an operator consumes (`dw`, `y$`, …).
static OPERATOR_MOTIONS: &[(KeyStroke, Motion)] = &[
    (KeyStroke::char('w'), Motion::WordForward),
    (KeyStroke::char('e'), Motion::WordEnd),
    (KeyStroke::char('b'), Motion::WordBackward),
    (KeyStroke::char('0'), Motion::LineStart),
    (KeyStroke::char('$'), Motion::LineEnd),
    (KeyStroke::char('h'), Motion::Left),
    (KeyStroke::new(KeyCode::Left, NONE), Motion::Left),
    (KeyStroke::char('l'), Motion::Right),
    (KeyStroke::new(KeyCode::Right, NONE), Motion::Right),
];

/// The modal keymaps as runtime data: compiled defaults, overridable from
/// the config file (user entries are *prepended*, so they shadow defaults).
#[derive(Clone)]
pub struct Keymaps {
    pub normal: Vec<(KeyStroke, EditorAction)>,
    pub visual: Vec<(KeyStroke, EditorAction)>,
    pub insert: Vec<(KeyStroke, EditorAction)>,
    /// Motions an operator consumes (`dw`…).
    pub operator_motions: Vec<(KeyStroke, Motion)>,
}

impl Default for Keymaps {
    fn default() -> Self {
        Self {
            normal: NORMAL_KEYMAP.to_vec(),
            visual: VISUAL_KEYMAP.to_vec(),
            insert: INSERT_KEYMAP.to_vec(),
            operator_motions: OPERATOR_MOTIONS.to_vec(),
        }
    }
}

impl Keymaps {
    /// Shadow-or-add bindings (`Vec::splice`: user entries first).
    pub fn override_keys(&mut self, mode: Mode, entries: Vec<(KeyStroke, EditorAction)>) {
        let table = match mode {
            Mode::Normal => &mut self.normal,
            Mode::Insert => &mut self.insert,
            Mode::Visual | Mode::VisualLine => &mut self.visual,
        };
        table.splice(..0, entries);
    }

    pub fn override_operator_motions(&mut self, entries: Vec<(KeyStroke, Motion)>) {
        self.operator_motions.splice(..0, entries);
    }
}

pub fn lookup<T: Copy>(keymap: &[(KeyStroke, T)], key: &KeyEvent) -> Option<T> {
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

/// The modal input state machine: fold a keypress (plus pending state) into
/// a resolution. Counts and armed operators live here and nowhere else.
/// Pure: `(pending, mode, key, keymaps) → (next pending, resolution)`.
/// The caller stores the returned `PendingState` (via an action) — resolve
/// itself touches nothing.
pub fn resolve(
    pending: &PendingState,
    mode: Mode,
    key: KeyEvent,
    keymaps: &Keymaps,
) -> (PendingState, Resolved) {
    match mode {
        Mode::Insert => (*pending, resolve_insert(keymaps, key)),
        Mode::Normal => resolve_modal(*pending, Mode::Normal, key, keymaps),
        Mode::Visual | Mode::VisualLine => resolve_modal(*pending, Mode::Visual, key, keymaps),
    }
}

fn resolve_insert(keymaps: &Keymaps, key: KeyEvent) -> Resolved {
    match lookup(&keymaps.insert, &key) {
        Some(action) => Resolved::Act(action, 1),
        None => match plain_char(&key) {
            Some(c) => Resolved::Insert(c),
            None => Resolved::Ignored,
        },
    }
}

/// Normal and visual share the machinery; `Mode` picks the keymap and
/// whether `d`/`y` arm operators (visual binds them directly).
fn resolve_modal(
    pending: PendingState,
    mode: Mode,
    key: KeyEvent,
    keymaps: &Keymaps,
) -> (PendingState, Resolved) {
    let mut pending = pending;
    let resolved = resolve_modal_inner(&mut pending, mode, key, keymaps);
    (pending, resolved)
}

fn resolve_modal_inner(
    pending: &mut PendingState,
    mode: Mode,
    key: KeyEvent,
    keymaps: &Keymaps,
) -> Resolved {
    // Count prefix: digits accumulate (`0` is a motion when no count yet).
    if let Some(digit @ ('1'..='9' | '0')) = plain_char(&key) {
        let d = digit.to_digit(10).unwrap() as usize;
        if d > 0 || pending.count.is_some() {
            pending.count = Some(pending.count.unwrap_or(0) * 10 + d);
            return Resolved::Swallowed;
        }
    }
    let count = pending.count.take().unwrap_or(1);

    // Operator-pending: this key resolves the operator.
    if let Some((pending_key, op_count)) = pending.key.take() {
        let total = op_count * count;
        return match (pending_key, plain_char(&key)) {
            (PendingKey::Delete, Some('d')) => Resolved::Act(EditorAction::DeleteLine, total),
            (PendingKey::Yank, Some('y')) => Resolved::Act(EditorAction::YankLine, total),
            (PendingKey::Goto, Some('g')) => Resolved::Act(EditorAction::MoveFirstLine, total),
            (PendingKey::View, Some('z')) => Resolved::View(ViewAction::CenterCursor),
            _ => {
                // A motion resolves an operator; prefixes cancel otherwise.
                if matches!(pending_key, PendingKey::Goto | PendingKey::View) {
                    return Resolved::Swallowed;
                }
                match lookup(&keymaps.operator_motions, &key) {
                    Some(motion) => match pending_key {
                        PendingKey::Delete => Resolved::DeleteMotion(motion, total),
                        PendingKey::Yank => Resolved::YankMotion(motion, total),
                        PendingKey::Goto | PendingKey::View => unreachable!(),
                    },
                    None => Resolved::Swallowed, // cancelled
                }
            }
        };
    }

    // `d`/`y`/`g` arm a pending key (visual binds its own `d`/`y` directly).
    if mode == Mode::Normal {
        if let Some(pending_key) = plain_char(&key).and_then(arm_key) {
            pending.key = Some((pending_key, count));
            return Resolved::Swallowed;
        }
    } else if let Some(prefix) = plain_char(&key).and_then(visual_prefix_key) {
        pending.key = Some((prefix, count));
        return Resolved::Swallowed;
    }

    if let Some(view) = lookup(VIEW_KEYS, &key) {
        return Resolved::View(view); // count deliberately dropped
    }

    let keymap = match mode {
        Mode::Normal => &keymaps.normal,
        _ => &keymaps.visual,
    };
    match lookup(keymap, &key) {
        Some(action) => Resolved::Act(action, count),
        None => Resolved::Ignored, // dead key; count already dropped
    }
}

fn arm_key(c: char) -> Option<PendingKey> {
    match c {
        'd' => Some(PendingKey::Delete),
        'y' => Some(PendingKey::Yank),
        'g' => Some(PendingKey::Goto),
        'z' => Some(PendingKey::View),
        _ => None,
    }
}

/// Visual-mode prefixes: `d`/`y` are direct bindings there; `g`/`z` arm.
fn visual_prefix_key(c: char) -> Option<PendingKey> {
    match c {
        'g' => Some(PendingKey::Goto),
        'z' => Some(PendingKey::View),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn resolve_normal(keys: &str) -> (Resolved, PendingState) {
        let keymaps = Keymaps::default();
        let mut pending = PendingState::default();
        let mut last = Resolved::Ignored;
        for c in keys.chars() {
            (pending, last) = resolve(&pending, Mode::Normal, key(c), &keymaps);
        }
        (last, pending)
    }

    #[test]
    fn plain_motion_resolves_with_default_count() {
        let (resolved, _) = resolve_normal("j");
        assert_eq!(resolved, Resolved::Act(EditorAction::MoveDown, 1));
    }

    #[test]
    fn count_prefix_multiplies_and_consumes() {
        let (resolved, _) = resolve_normal("12j");
        assert_eq!(resolved, Resolved::Act(EditorAction::MoveDown, 12));
    }

    #[test]
    fn zero_is_a_motion_without_a_count() {
        let (resolved, _) = resolve_normal("0");
        assert_eq!(resolved, Resolved::Act(EditorAction::LineStart, 1));
        // With a count started, `0` is a digit: `10$` = count 10, line end.
        let (resolved, _) = resolve_normal("10$");
        assert_eq!(resolved, Resolved::Act(EditorAction::LineEnd, 10));
    }

    #[test]
    fn operators_arm_then_resolve() {
        let (resolved, _) = resolve_normal("dd");
        assert_eq!(resolved, Resolved::Act(EditorAction::DeleteLine, 1));

        let (resolved, _) = resolve_normal("dw");
        assert_eq!(resolved, Resolved::DeleteMotion(Motion::WordForward, 1));

        // Counts compose either way: 2dw == d2w.
        let (a, _) = resolve_normal("2dw");
        let (b, _) = resolve_normal("d2w");
        assert_eq!(a, Resolved::DeleteMotion(Motion::WordForward, 2));
        assert_eq!(a, b);
    }

    #[test]
    fn zz_resolves_to_a_view_intent() {
        let (resolved, pending) = resolve_normal("zz");
        assert_eq!(resolved, Resolved::View(ViewAction::CenterCursor));
        assert_eq!(pending.key, None);
    }

    #[test]
    fn z_prefix_cancels_on_anything_but_z() {
        let (resolved, pending) = resolve_normal("zx");
        assert_eq!(resolved, Resolved::Swallowed);
        assert_eq!(pending.key, None, "cancelled prefix is forgotten");
    }

    #[test]
    fn zz_also_works_in_visual() {
        let keymaps = Keymaps::default();
        let pending = PendingState::default();
        let (pending, _) = resolve(&pending, Mode::Visual, key('z'), &keymaps);
        assert_eq!(pending.key, Some((PendingKey::View, 1)));
        let (_, resolved) = resolve(&pending, Mode::Visual, key('z'), &keymaps);
        assert_eq!(resolved, Resolved::View(ViewAction::CenterCursor));
    }

    #[test]
    fn ctrl_f_b_resolve_to_page_view_intents() {
        use crate::input::{KeyCode, KeyModifiers};
        let keymaps = Keymaps::default();
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);

        let pending = PendingState::default();
        let (_, r) = resolve(&pending, Mode::Normal, ctrl('f'), &keymaps);
        assert_eq!(r, Resolved::View(ViewAction::PageDown));
        let (_, r) = resolve(&pending, Mode::Normal, ctrl('u'), &keymaps);
        assert_eq!(r, Resolved::View(ViewAction::PageUp));
        // Visual too, and no count semantics (a pending count is dropped).
        let (_, r) = resolve(&pending, Mode::Visual, ctrl('f'), &keymaps);
        assert_eq!(r, Resolved::View(ViewAction::PageDown));
    }

    #[test]
    fn non_motion_cancels_the_operator() {
        let (resolved, pending) = resolve_normal("dz");
        assert_eq!(resolved, Resolved::Swallowed);
        assert_eq!(pending.key, None, "cancelled operator is forgotten");
    }

    #[test]
    fn gg_jumps_with_count() {
        let (resolved, _) = resolve_normal("gg");
        assert_eq!(resolved, Resolved::Act(EditorAction::MoveFirstLine, 1));
        let (resolved, _) = resolve_normal("5gg");
        assert_eq!(resolved, Resolved::Act(EditorAction::MoveFirstLine, 5));
    }

    #[test]
    fn visual_binds_operator_keys_directly() {
        let keymaps = Keymaps::default();
        let pending = PendingState::default();
        let (pending, resolved) = resolve(&pending, Mode::Visual, key('d'), &keymaps);
        assert_eq!(resolved, Resolved::Act(EditorAction::DeleteSelection, 1));
        assert_eq!(pending.key, None, "visual d never arms");
        let (_, resolved) = resolve(&pending, Mode::VisualLine, key('v'), &keymaps);
        assert_eq!(resolved, Resolved::Act(EditorAction::VisualCharOrExit, 1));
    }

    #[test]
    fn unbound_key_is_ignored_and_drops_the_count() {
        let (resolved, pending) = resolve_normal("5 ");
        assert_eq!(resolved, Resolved::Ignored);
        assert_eq!(pending.count, None);
    }

    #[test]
    fn insert_inserts_plain_chars() {
        let keymaps = Keymaps::default();
        let pending = PendingState::default();
        let (_, resolved) = resolve(&pending, Mode::Insert, key('x'), &keymaps);
        assert_eq!(resolved, Resolved::Insert('x'));
    }

    // ---- interpret (against a minimal EditorCtx) ----

    struct MockCtx {
        editor: Editor,
        notes: Vec<String>,
    }

    impl EditorCtx for MockCtx {
        fn editor(&mut self) -> &mut Editor {
            &mut self.editor
        }
        fn notify(&mut self, message: &str) {
            self.notes.push(message.to_owned());
        }
    }

    #[test]
    fn interpret_undo_at_oldest_notifies() {
        let mut ctx = MockCtx {
            editor: Editor::scratch().unwrap(),
            notes: Vec::new(),
        };
        interpret(EditorAction::Undo, 1, &mut ctx);
        assert_eq!(ctx.notes, ["already at oldest change"]);
    }

    #[test]
    fn interpret_open_below_enters_insert() {
        let mut ctx = MockCtx {
            editor: Editor::scratch().unwrap(),
            notes: Vec::new(),
        };
        interpret(EditorAction::OpenBelow, 1, &mut ctx);
        assert_eq!(ctx.editor.mode(), Mode::Insert);
    }
}
