//! The leap-jump layer (`Space g c`): type a 2-char pattern, every match
//! gets a label chip (the rest of the text dims — see the editor
//! component's `dim`/`labels` props), type a label to jump there.
//!
//! Stateless itself: the leap state lives in `App::leap` because the
//! editor surface below renders it (Rule 5 — views derive from App).

use eggplant_core::input::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::{App, Leap, LeapLabel};
use crate::commands::KeyStroke;
use crate::components::prompt::{self, PromptProps};
use crate::compositor::{KeyResult, Layer, LayerKind};
use crate::element::Element;

/// Home-row-first label alphabet (type-able without looking).
const LEAP_LABELS: &str = "asdfjkl;qweruiopzxcvbnm";

/// Matches get labels in document order; extra matches stay unreachable.
pub(crate) fn assign_labels(matches: Vec<(usize, usize)>) -> Vec<LeapLabel> {
    matches
        .into_iter()
        .zip(LEAP_LABELS.chars())
        .map(|((line, col), label)| LeapLabel { label, line, col })
        .collect()
}

/// How far the pattern has been typed (2 chars total, kept simple).
const PATTERN_LEN: usize = 2;

/// What a keypress *means* to a leap in progress — resolved from state +
/// key, so `handle_key` applies rather than interprets (same split as
/// `editing.rs`'s resolve/interpret).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeapInput {
    /// Backspace: drop the last pattern char, forget the labels.
    DeleteChar,
    /// Phase 1 (labels not yet up): extend the pattern.
    PatternChar(char),
    /// Phase 2 (labels up): the key picked a jump target.
    Jump(usize, usize),
    /// Phase 2: the key matches no label — cancel the leap.
    Cancel,
}

/// Pure: leap state + key → semantic input. The phase (typing vs. picking)
/// is decided here, once.
fn resolve_input(leap: &Leap, key: &KeyEvent) -> Option<LeapInput> {
    match key.code {
        KeyCode::Backspace => Some(LeapInput::DeleteChar),
        KeyCode::Char(c) if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            if leap.labels.is_empty() {
                Some(LeapInput::PatternChar(c))
            } else {
                Some(
                    leap.labels
                        .iter()
                        .find(|label| label.label == c)
                        .map_or(LeapInput::Cancel, |label| {
                            LeapInput::Jump(label.line, label.col)
                        }),
                )
            }
        }
        _ => None,
    }
}

/// Leap's editor-decoration semantics — owned here, so the editor surface
/// stays decoration-agnostic (App's selectors aggregate overlay features).
impl Leap {
    /// Label chips on one document line: `(col, label)`.
    pub fn labels_on_line(&self, line: usize) -> Vec<(usize, char)> {
        self.labels
            .iter()
            .filter(|label| label.line == line)
            .map(|label| (label.col, label.label))
            .collect()
    }

    /// Phase 2 (labels assigned): dim the buffer text so chips stand out.
    pub fn dims_text(&self) -> bool {
        !self.labels.is_empty()
    }
}

/// Leap's closed action set (config: `[keys.leap]`). Pattern chars,
/// Backspace and label keys are input, not bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeapAction {
    Close,
}

impl LeapAction {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "close" => Some(Self::Close),
            _ => None,
        }
    }
}

/// Default leap bindings.
pub const DEFAULT_KEYS: &[(KeyStroke, LeapAction)] = &[(
    KeyStroke::new(KeyCode::Esc, KeyModifiers::NONE),
    LeapAction::Close,
)];

pub struct LeapLayer;

impl Layer for LeapLayer {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let pattern = app.leap.as_ref().map_or("", |leap| &leap.pattern);
        prompt::view(
            &PromptProps {
                label: "leap: ",
                input: pattern.to_owned(),
            },
            area,
            &app.theme,
        )
    }

    fn handle_key(&mut self, key: KeyEvent, app: &mut App) -> KeyResult {
        if crate::editing::lookup(&app.layer_keys.leap, &key) == Some(LeapAction::Close) {
            app.leap = None;
            return KeyResult::Close;
        }
        let Some(leap) = &app.leap else {
            return KeyResult::Close; // no leap in progress: nothing to do here
        };
        let Some(input) = resolve_input(leap, &key) else {
            return KeyResult::Consumed; // modal-ish
        };
        match input {
            LeapInput::DeleteChar => {
                if let Some(leap) = &mut app.leap {
                    leap.pattern.pop();
                    leap.labels.clear();
                }
                KeyResult::Consumed
            }
            LeapInput::PatternChar(c) => {
                let leap = app.leap.as_mut().expect("checked above");
                leap.pattern.push(c);
                if leap.pattern.chars().count() == PATTERN_LEN {
                    let labels = assign_labels(app.editor.find_matches(&leap.pattern));
                    if labels.is_empty() {
                        app.leap = None; // no match: done
                        return KeyResult::Close;
                    }
                    app.leap.as_mut().expect("checked above").labels = labels;
                }
                KeyResult::Consumed
            }
            LeapInput::Jump(line, col) => {
                app.editor.jump_to(line, col);
                app.leap = None;
                KeyResult::Close
            }
            LeapInput::Cancel => {
                app.leap = None;
                KeyResult::Close
            }
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "leap"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leap_with_labels() -> Leap {
        Leap {
            pattern: "ab".to_owned(),
            labels: vec![
                LeapLabel {
                    label: 'a',
                    line: 1,
                    col: 3,
                },
                LeapLabel {
                    label: 's',
                    line: 2,
                    col: 0,
                },
                LeapLabel {
                    label: 'd',
                    line: 1,
                    col: 7,
                },
            ],
        }
    }

    #[test]
    fn resolve_input_tracks_the_phase() {
        use eggplant_core::input::{KeyCode, KeyEvent, KeyModifiers};
        let key = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        // Phase 1 (no labels): chars extend the pattern.
        let mut leap = Leap::default();
        assert_eq!(
            resolve_input(&leap, &key('a')),
            Some(LeapInput::PatternChar('a'))
        );
        assert_eq!(
            resolve_input(&leap, &key('b')),
            Some(LeapInput::PatternChar('b'))
        );
        leap.pattern = "ab".to_owned();

        // Phase 2 (labels up): a label key jumps, an unknown key cancels.
        leap.labels = vec![LeapLabel {
            label: 's',
            line: 3,
            col: 5,
        }];
        assert_eq!(resolve_input(&leap, &key('s')), Some(LeapInput::Jump(3, 5)));
        assert_eq!(resolve_input(&leap, &key('x')), Some(LeapInput::Cancel));

        // Backspace edits the pattern in either phase.
        assert_eq!(
            resolve_input(
                &leap,
                &KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
            ),
            Some(LeapInput::DeleteChar)
        );
    }

    #[test]
    fn labels_on_line_filters_and_dims_only_with_labels() {
        let leap = leap_with_labels();
        assert_eq!(leap.labels_on_line(1), [(3, 'a'), (7, 'd')]);
        assert_eq!(leap.labels_on_line(2), [(0, 's')]);
        assert_eq!(leap.labels_on_line(9), []);
        assert!(leap.dims_text());

        let mut empty = leap;
        empty.labels.clear();
        assert!(!empty.dims_text(), "phase 1 (typing): no dimming");
    }

    #[test]
    fn labels_follow_document_order_capped_to_the_alphabet() {
        let labels = assign_labels(vec![(0, 0), (2, 5), (1, 3)]);
        assert_eq!(
            labels[0],
            LeapLabel {
                label: 'a',
                line: 0,
                col: 0
            }
        );
        assert_eq!(
            labels[1],
            LeapLabel {
                label: 's',
                line: 2,
                col: 5
            }
        );
        assert_eq!(
            labels[2],
            LeapLabel {
                label: 'd',
                line: 1,
                col: 3
            }
        );

        let many: Vec<(usize, usize)> = (0..50).map(|i| (i, 0)).collect();
        assert_eq!(assign_labels(many).len(), LEAP_LABELS.len());
    }
}
