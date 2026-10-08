//! The chat containers: one shared `ChatView` (input + scroll state,
//! key handling → actions), two thin layer shells — the modal popup
//! (`Space a i`, `C-i`) and the right-side panel (`Space a t`). One
//! session, two presentations (docs/design/agent.md).

use eggplant_core::input::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::action::{AppAction, Handled};
use crate::app::App;
use crate::components::chat::{self, ChatProps};
use crate::compositor::{Layer, LayerKind, Side};
use crate::element::Element;
use crate::stylesheet::Stylesheet;

pub const MODAL_ID: &str = "chat";
pub const PANEL_ID: &str = "chat-panel";
/// Panel width in columns.
const PANEL_SIZE: u16 = 48;

/// Shared chat state: the input draft and transcript scroll. The
/// transcript itself lives in `App.agent` (one session); these are the
/// per-view bits (React: container-local `useState`).
#[derive(Default)]
struct ChatView {
    input: String,
    scroll: usize,
    /// Last frame's transcript viewport height (for page scroll).
    page: usize,
}

/// What `Esc` means for this presentation.
enum EscBehavior {
    /// Modal: close the float.
    Close,
    /// Panel: return focus to the editor.
    Unfocus,
}

impl ChatView {
    fn props<'a>(&'a self, title: &'a str, app: &'a App) -> ChatProps<'a> {
        ChatProps {
            title,
            items: &app.agent.transcript,
            input: &self.input,
            running: app.agent.running,
            scroll: self.scroll,
        }
    }

    fn view(&self, title: &str, area: Rect, app: &App) -> Element {
        let sheet = Stylesheet::new(&app.theme.current);
        chat::view(&self.props(title, app), area, &sheet)
    }

    fn handle_key(&mut self, key: KeyEvent, esc: EscBehavior) -> Handled {
        match key.code {
            KeyCode::Esc => match esc {
                EscBehavior::Close => Handled::one(AppAction::CloseSelf),
                EscBehavior::Unfocus => Handled::one(AppAction::Unfocus),
            },
            KeyCode::Enter => {
                let text = self.input.trim().to_owned();
                if text.is_empty() {
                    return Handled::quiet();
                }
                self.input.clear();
                self.scroll = 0; // sending snaps to the tail
                Handled::one(AppAction::AgentPrompt(text))
            }
            KeyCode::Backspace => {
                self.input.pop();
                Handled::quiet()
            }
            // Interrupt: C-F8 (dedicated, mirrors the global binding) and
            // C-c (terminal muscle memory — swallowed by the chat views
            // anyway, so it can't reach force-quit here).
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Handled::one(AppAction::AgentAbort)
            }
            KeyCode::F(8) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Handled::one(AppAction::AgentAbort)
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                Handled::quiet()
            }
            KeyCode::Up => {
                self.scroll += 1;
                Handled::quiet()
            }
            KeyCode::Down => {
                self.scroll = self.scroll.saturating_sub(1);
                Handled::quiet()
            }
            KeyCode::PageUp => {
                self.scroll += self.page.max(1);
                Handled::quiet()
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_sub(self.page.max(1));
                Handled::quiet()
            }
            _ => Handled::quiet(), // chat views swallow keys (modal-ish)
        }
    }

    fn resize(&mut self, area: Rect) {
        // The transcript viewport is the area minus borders and input.
        self.page = (area.height as usize).saturating_sub(4);
    }
}

/// The popup: a centered float over the editor.
pub struct ChatModal {
    chat: ChatView,
}

impl ChatModal {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            chat: ChatView::default(),
        })
    }
}

impl Layer for ChatModal {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        // Centered 60%×60% float.
        use ratatui::layout::{Constraint, Layout};
        let [_, vertical, _] = Layout::vertical([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .areas(area);
        let [_, horizontal, _] = Layout::horizontal([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .areas(vertical);
        Element::fixed(
            horizontal,
            Element::cleared(
                self.chat
                    .view("agent — Esc closes, C-F8 aborts", horizontal, app),
            ),
        )
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &App) -> Handled {
        self.chat.handle_key(key, EscBehavior::Close)
    }

    fn resize(&mut self, area: Rect, _app: &App) {
        // The float's content area: 60% of the frame.
        self.chat.resize(Rect {
            width: area.width * 3 / 5,
            height: area.height * 3 / 5,
            ..area
        });
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        MODAL_ID
    }
}

/// The panel: a right-docked window, a persistent workspace member.
pub struct ChatPanel {
    chat: ChatView,
}

impl ChatPanel {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            chat: ChatView::default(),
        })
    }
}

impl Layer for ChatPanel {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        Element::cleared(
            self.chat
                .view("agent — Esc unfocuses, C-F8 aborts", area, app),
        )
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &App) -> Handled {
        self.chat.handle_key(key, EscBehavior::Unfocus)
    }

    fn resize(&mut self, area: Rect, _app: &App) {
        self.chat.resize(area);
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Panel {
            side: Side::Right,
            size: PANEL_SIZE,
        }
    }

    fn id(&self) -> &'static str {
        PANEL_ID
    }
}
