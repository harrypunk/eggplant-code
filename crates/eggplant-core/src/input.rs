//! Terminal-agnostic input vocabulary: key codes, modifiers, events, and
//! keymap strokes — plain data plus parsing, no I/O. The UI shell
//! translates crossterm events into these at the event boundary; tests
//! construct them directly.

/// A key, terminal-agnostic (mirrors the crossterm subset we use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    F(u8),
}

/// Modifier bitset (`CONTROL`/`ALT`/`SHIFT`), composable via `|`/`|=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const CONTROL: Self = Self(1);
    pub const ALT: Self = Self(2);
    pub const SHIFT: Self = Self(4);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// A key press as the runtime reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyEvent {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn char(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
}

/// A keymap entry: the key combination an action is bound to. Same shape
/// as `KeyEvent` plus config parsing and shift-tolerant matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyStroke {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn char(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub const fn ctrl(c: char) -> Self {
        Self::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    pub const fn ctrl_shift(c: char) -> Self {
        Self::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL.union(KeyModifiers::SHIFT),
        )
    }

    pub const fn function(n: u8) -> Self {
        Self::new(KeyCode::F(n), KeyModifiers::NONE)
    }

    /// Parse a config-file stroke: `"C-S-p"`, `"Space"`, `"g"`, `"F2"`,
    /// `"left"`. Modifiers are `-`-prefixed (`C-`/`A-`/`S-`), key names are
    /// case-insensitive, a single char keeps its case (`G` ≠ `g`).
    pub fn parse(text: &str) -> Option<Self> {
        let (mods, key) = match text.rsplit_once('-') {
            Some((mods, key)) if !key.is_empty() => (mods, key),
            _ => ("", text),
        };
        let mut modifiers = KeyModifiers::NONE;
        for m in mods.split('-').filter(|m| !m.is_empty()) {
            modifiers |= match m.to_ascii_lowercase().as_str() {
                "c" | "ctrl" => KeyModifiers::CONTROL,
                "a" | "alt" => KeyModifiers::ALT,
                "s" | "shift" => KeyModifiers::SHIFT,
                _ => return None,
            };
        }
        let code = match key.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "esc" => KeyCode::Esc,
            "enter" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            f if f.len() >= 2
                && f.len() <= 3
                && f.starts_with('f')
                && f[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                KeyCode::F(f[1..].parse().ok()?)
            }
            _ if key.chars().count() == 1 => {
                KeyCode::Char(key.chars().next().expect("len checked"))
            }
            _ => return None,
        };
        Some(Self::new(code, modifiers))
    }

    /// Does this stroke match a runtime event? Exact on modifiers, with
    /// one tolerance: Shift alone doesn't change which char a plain
    /// binding means — terminals already report the shifted char (`G`,
    /// `$`).
    pub fn matches(&self, key: &KeyEvent) -> bool {
        if self.code != key.code {
            return false;
        }
        if self.modifiers == key.modifiers {
            return true;
        }
        self.modifiers.is_empty() && key.modifiers == KeyModifiers::SHIFT
    }
}
