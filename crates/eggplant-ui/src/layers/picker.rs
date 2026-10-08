//! The generic picker container: owns the input and selection, derives the
//! visible item list, delegates rendering to the pure `components::picker`
//! view. Concrete pickers (command palette, buffer grep, project grep, …)
//! are constructor functions over `PickerSpec`.
//!
//! Two seams make it generic (docs/design/live-grep.md):
//! - `PickerSource`: static list + fuzzy filter, or a live query that
//!   re-derives items on every input change.
//! - `preview_of`: materializes the selected item's preview at event time
//!   (Rule 5: I/O here, never in the view).

use eggplant_core::input::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::action::{AppAction, Handled};
use crate::app::App;
use crate::commands::KeyStroke;
use crate::components::picker::{self, PickerItem, PickerProps};
use crate::components::preview::PreviewProps;
use crate::compositor::{Layer, LayerKind};
use crate::element::Element;
use eggplant_core::fuzzy;

/// The picker's closed action set (config: `[keys.picker]`). Typed chars
/// and Backspace edit the filter — text-field behavior, not bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    MoveDown,
    MoveUp,
    Confirm,
    Close,
}

impl PickerAction {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "down" => Self::MoveDown,
            "up" => Self::MoveUp,
            "confirm" => Self::Confirm,
            "close" => Self::Close,
            _ => return None,
        })
    }
}

/// Default picker bindings.
pub const DEFAULT_KEYS: &[(KeyStroke, PickerAction)] = &[
    (
        KeyStroke::new(KeyCode::Down, KeyModifiers::NONE),
        PickerAction::MoveDown,
    ),
    (
        KeyStroke::new(KeyCode::Up, KeyModifiers::NONE),
        PickerAction::MoveUp,
    ),
    (KeyStroke::ctrl('n'), PickerAction::MoveDown),
    (KeyStroke::ctrl('p'), PickerAction::MoveUp),
    (
        KeyStroke::new(KeyCode::Enter, KeyModifiers::NONE),
        PickerAction::Confirm,
    ),
    (
        KeyStroke::new(KeyCode::Esc, KeyModifiers::NONE),
        PickerAction::Close,
    ),
];

/// Where a picker's items come from.
pub enum PickerSource<T> {
    /// Static items, fuzzy-filtered by the input (palette, buffer grep,
    /// file picker).
    List {
        items: Vec<T>,
        /// Text the fuzzy filter matches against.
        text_of: fn(&T) -> &str,
    },
    /// Live derivation: the input re-runs the query on every change
    /// (project grep). The query result is the item list — no fuzzy on
    /// top.
    Query { run: fn(&str, &App) -> Vec<T> },
}

/// The preview seam: materialize an item's preview given the pane's row
/// budget.
pub type PreviewFn<T> = fn(&T, &App, usize) -> Option<PreviewProps>;

/// What makes a picker concrete: its item source plus function pointers —
/// how to display, what Enter does, optionally how to preview.
pub struct PickerSpec<T> {
    /// Frame title ("palette", "grep", …).
    pub title: &'static str,
    pub source: PickerSource<T>,
    /// Display projection: (primary column, free-form text).
    pub project: fn(&T) -> (String, String),
    /// Enter on an item — returns an ACTION (data), never performs it.
    /// The compositor's dispatch interprets it (the state-flow contract:
    /// specs are pure; effects live in one place).
    pub on_select: fn(&T) -> AppAction,
    /// Materialize the selected item's preview (runs at event time).
    /// The third argument is the row budget — the preview pane's text
    /// capacity from the shared layout formula (preview-as-viewport:
    /// produce `scroll .. scroll + budget` rows, never a magic cap).
    /// `None` when the item has nothing previewable.
    pub preview_of: Option<PreviewFn<T>>,
}

pub struct Picker<T> {
    input: String,
    selected: usize,
    /// Last area from the compositor's `resize` hook — feeds the preview
    /// row budget.
    area: Rect,
    /// Materialized items: the whole list for `List`, the last query
    /// result for `Query`.
    items: Vec<T>,
    preview: Option<PreviewProps>,
    spec: PickerSpec<T>,
}

impl<T> Picker<T> {
    pub fn new(spec: PickerSpec<T>) -> Self {
        let items = match &spec.source {
            PickerSource::List { .. } => Vec::new(),  // moved out below
            PickerSource::Query { .. } => Vec::new(), // queries start empty
        };
        let mut picker = Self {
            input: String::new(),
            selected: 0,
            area: Rect::default(),
            items,
            preview: None,
            spec,
        };
        if let PickerSource::List { items, .. } = &mut picker.spec.source {
            picker.items = std::mem::take(items);
        }
        picker
    }

    /// The visible items: fuzzy-filtered for `List`, as-queried for
    /// `Query` (selector — derived, never cached).
    fn filtered(&self) -> Vec<&T> {
        match &self.spec.source {
            PickerSource::List { text_of, .. } => {
                fuzzy::filter(&self.input, &self.items, |item| text_of(item))
                    .into_iter()
                    .map(|(_, item)| item)
                    .collect()
            }
            PickerSource::Query { .. } => self.items.iter().collect(),
        }
    }

    fn move_selection(&mut self, delta: i32, app: &App) {
        let len = self.filtered().len();
        if len > 0 {
            self.selected = (self.selected as i32 + delta).rem_euclid(len as i32) as usize;
        }
        self.refresh_preview(app);
    }

    /// The input changed: re-derive (query sources), reset the selection,
    /// re-materialize the preview.
    fn input_changed(&mut self, app: &App) {
        if let PickerSource::Query { run } = &self.spec.source {
            self.items = run(&self.input, app);
        }
        self.selected = 0;
        self.refresh_preview(app);
    }

    /// Preview is state: materialized here (event time), painted by the
    /// view (pure). The budget comes from the layout formula, so exactly
    /// the visible rows are produced — no arbitrary cap.
    fn refresh_preview(&mut self, app: &App) {
        let budget = picker::preview_budget(self.area);
        self.preview = self.spec.preview_of.and_then(|preview_of| {
            self.filtered()
                .get(self.selected)
                .and_then(|item| preview_of(item, app, budget))
        });
    }
}

impl<T> Layer for Picker<T> {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let items = self
            .filtered()
            .into_iter()
            .map(|item| {
                let (primary, secondary) = (self.spec.project)(item);
                PickerItem { primary, secondary }
            })
            .collect();
        picker::view(
            &PickerProps {
                title: self.spec.title,
                input: self.input.clone(),
                items,
                selected: self.selected,
                preview: self.preview.as_ref(),
            },
            area,
            &app.theme.sheet(),
        )
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> Handled {
        if let Some(action) = eggplant_core::editing::lookup(&app.input.layer_keys.picker, &key) {
            return match action {
                // Selecting an item performs its action, then the picker
                // closes and focus follows the outcome to the editor.
                PickerAction::Confirm => match self.filtered().get(self.selected) {
                    Some(item) => Handled::Acted(vec![
                        (self.spec.on_select)(item),
                        AppAction::CloseSelf,
                        AppAction::Unfocus,
                    ]),
                    None => Handled::one(AppAction::CloseSelf),
                },
                PickerAction::Close => Handled::one(AppAction::CloseSelf),
                PickerAction::MoveUp => {
                    self.move_selection(-1, app);
                    Handled::quiet()
                }
                PickerAction::MoveDown => {
                    self.move_selection(1, app);
                    Handled::quiet()
                }
            };
        }
        // Text entry (not bindings): chars filter, Backspace edits.
        match key.code {
            KeyCode::Backspace => {
                self.input.pop();
                self.input_changed(app);
                Handled::quiet()
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                self.input.push(c);
                self.input_changed(app);
                Handled::quiet()
            }
            _ => Handled::quiet(), // modal-ish
        }
    }

    fn resize(&mut self, area: Rect, app: &App) {
        if self.area != area {
            self.area = area;
            self.refresh_preview(app); // budget changed: re-derive
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "picker"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item(String);
    fn spec(source: PickerSource<Item>) -> PickerSpec<Item> {
        PickerSpec {
            title: "test",
            source,
            project: |item| (item.0.clone(), String::new()),
            on_select: |_| AppAction::CloseSelf,
            preview_of: None,
        }
    }

    fn app() -> App {
        App::new(eggplant_core::Editor::scratch().unwrap())
    }

    fn char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn list_source_fuzzy_filters() {
        let mut picker = Picker::new(spec(PickerSource::List {
            items: vec![Item("alpha".into()), Item("beta".into())],
            text_of: |item| &item.0,
        }));
        let app = app();
        picker.handle_key(char_key('b'), &app);
        assert_eq!(picker.filtered().len(), 1);
        assert_eq!(picker.filtered()[0].0, "beta");
    }

    #[test]
    fn confirm_emits_actions_as_data_without_performing_them() {
        // The state-flow seam: a layer's effect is its returned actions —
        // assert them directly, no App mutation involved.
        let mut picker = Picker::new(spec(PickerSource::List {
            items: vec![Item("a".into())],
            text_of: |item| &item.0,
        }));
        let app = app();
        let handled = picker.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &app);
        let Handled::Acted(actions) = handled else {
            panic!("confirm must act");
        };
        assert_eq!(actions.len(), 3, "select + close + focus-follows");
        assert!(matches!(actions[0], AppAction::CloseSelf)); // spec's on_select
        assert!(matches!(actions[1], AppAction::CloseSelf));
        assert!(matches!(actions[2], AppAction::Unfocus));
        // …and nothing happened to shared state in the meantime.
        assert_eq!(app.input.pending, Default::default());
    }

    #[test]
    fn query_source_re_runs_on_input_change() {
        let run = |input: &str, _: &App| -> Vec<Item> {
            (0..input.len()).map(|i| Item(format!("hit{i}"))).collect()
        };
        let mut picker = Picker::new(spec(PickerSource::Query { run }));
        let app = app();
        assert_eq!(picker.filtered().len(), 0, "queries start empty");
        picker.handle_key(char_key('a'), &app);
        assert_eq!(picker.filtered().len(), 1);
        picker.handle_key(char_key('b'), &app);
        assert_eq!(picker.filtered().len(), 2);
        picker.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE), &app);
        assert_eq!(picker.filtered().len(), 1, "backspace re-queries too");
        assert_eq!(picker.selected, 0, "selection resets on input change");
    }

    #[test]
    fn preview_materializes_on_selection_and_input() {
        let run = |_: &str, _: &App| -> Vec<Item> { vec![Item("one".into()), Item("two".into())] };
        let preview_of = |item: &Item, _: &App, _: usize| -> Option<PreviewProps> {
            Some(PreviewProps {
                title: item.0.clone(),
                first_line: 0,
                rows: vec![crate::components::preview::PreviewRow {
                    spans: vec![eggplant_core::HighlightedSpan {
                        text: item.0.clone(),
                        scope: None,
                    }],
                    search_marks: Vec::new(),
                }],
                focus_row: 0,
            })
        };
        let mut picker = Picker::new(PickerSpec {
            preview_of: Some(preview_of),
            ..spec(PickerSource::Query { run })
        });
        let app = app();
        assert!(picker.preview.is_none(), "no preview before any item");
        picker.handle_key(char_key('x'), &app);
        assert_eq!(picker.preview.as_ref().unwrap().title, "one");
        picker.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &app);
        assert_eq!(picker.preview.as_ref().unwrap().title, "two");
        picker.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &app);
        assert_eq!(picker.preview.as_ref().unwrap().title, "one");
    }
}
