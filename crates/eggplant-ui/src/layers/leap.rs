//! The leap-jump layer (`Space g c`): type a 2-char pattern, every match
//! gets a label chip (the rest of the text dims — see the editor
//! component's `dim`/`labels` props), type a label to jump there.
//!
//! Stateless itself: the leap state lives in `App::leap` because the
//! editor surface below renders it (Rule 5 — views derive from App).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::{App, LeapLabel};
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
        match key.code {
            KeyCode::Esc => {
                app.leap = None;
                KeyResult::Close
            }
            KeyCode::Backspace => {
                if let Some(leap) = &mut app.leap {
                    leap.pattern.pop();
                    leap.labels.clear();
                }
                KeyResult::Consumed
            }
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                let Some(leap) = &mut app.leap else {
                    return KeyResult::Close;
                };
                // Labels up: a key resolves a jump target (unknown cancels).
                if !leap.labels.is_empty() {
                    let target = leap
                        .labels
                        .iter()
                        .find(|label| label.label == c)
                        .map(|label| (label.line, label.col));
                    if let Some((line, col)) = target {
                        app.editor.jump_to(line, col);
                    }
                    app.leap = None;
                    return KeyResult::Close;
                }
                leap.pattern.push(c);
                if leap.pattern.chars().count() == PATTERN_LEN {
                    let matches = app.editor.find_matches(&leap.pattern);
                    leap.labels = assign_labels(matches);
                    if leap.labels.is_empty() {
                        app.leap = None; // no match: done
                        return KeyResult::Close;
                    }
                }
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed, // modal-ish
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
