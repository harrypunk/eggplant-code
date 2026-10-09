//! The model picker (`Space a m`): authenticated providers as section
//! titles, their models (fetched from `GET /models`) as items; Enter
//! saves the choice as the provider's default model in auth.toml.

use eggplant_core::input::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::action::{AppAction, Handled};
use crate::agent::ModelListState;
use crate::app::App;
use crate::compositor::{Layer, LayerKind};
use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// A display row: a provider section header or a selectable model.
struct Row {
    /// Set for model rows: the provider it belongs to.
    provider: Option<String>,
    label: String,
    /// This model is the provider's saved default.
    is_default: bool,
}

pub struct ModelPicker {
    selected: usize,
}

impl ModelPicker {
    pub fn new() -> Box<Self> {
        Box::new(Self { selected: 0 })
    }

    /// Flatten providers → section headers + model rows, in preset order.
    fn rows(&self, app: &App) -> Vec<Row> {
        let mut rows = Vec::new();
        for (provider, list) in &app.agent.model_lists {
            rows.push(Row {
                provider: None,
                label: provider.clone(),
                is_default: false,
            });
            match list {
                ModelListState::Loading => rows.push(Row {
                    provider: Some(provider.clone()),
                    label: "  loading…".to_string(),
                    is_default: false,
                }),
                ModelListState::Error(e) => rows.push(Row {
                    provider: Some(provider.clone()),
                    label: format!("  ⚠ {e}"),
                    is_default: false,
                }),
                ModelListState::Ready(models) => {
                    let default = app.agent.default_models.get(provider);
                    for model in models {
                        rows.push(Row {
                            provider: Some(provider.clone()),
                            label: format!("  {model}"),
                            is_default: default == Some(model),
                        });
                    }
                }
            }
        }
        rows
    }

    /// Move the selection to the next/previous *selectable* row.
    fn move_selection(&mut self, rows: &[Row], delta: isize) {
        if rows.is_empty() {
            return;
        }
        let len = rows.len() as isize;
        let mut next = self.selected as isize;
        for _ in 0..len {
            next = (next + delta).rem_euclid(len);
            if rows[next as usize].provider.is_some()
                && !rows[next as usize].label.starts_with("  loading")
                && !rows[next as usize].label.starts_with("  ⚠")
            {
                self.selected = next as usize;
                return;
            }
        }
    }
}

impl Layer for ModelPicker {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let sheet = Stylesheet::new(&app.theme.current);
        let rows = self.rows(app);
        let width = 64u16.min(area.width.saturating_sub(4));
        let height = ((rows.len() as u16 + 2) + 2).min(area.height.saturating_sub(2));
        let frame = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );

        let row_lines: Vec<Line<'static>> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                if row.provider.is_none() {
                    // Section header = provider name.
                    return Line::from(Span::styled(
                        format!(" {}", row.label),
                        sheet.style(StyleClass::Title),
                    ));
                }
                let selected = i == self.selected;
                let marker = if selected {
                    "▸"
                } else if row.is_default {
                    "✓"
                } else {
                    " "
                };
                let style = if selected {
                    sheet.emphasized(StyleClass::Selected)
                } else if row.is_default {
                    sheet.style(StyleClass::Info)
                } else if row.label.starts_with("  ⚠") || row.label.starts_with("  loading") {
                    sheet.style(StyleClass::Muted)
                } else {
                    sheet.style(StyleClass::Text)
                };
                Line::from(vec![
                    Span::styled(format!(" {marker}"), style),
                    Span::styled(row.label.clone(), style),
                ])
            })
            .collect();

        // Keep the selection inside the visible window (derived from the
        // selection, not cached).
        let visible = height.saturating_sub(4) as usize;
        let scroll = self
            .selected
            .saturating_sub(visible / 2)
            .min(row_lines.len().saturating_sub(visible));
        let mut lines: Vec<Line<'static>> = row_lines.into_iter().skip(scroll).collect();
        if rows.is_empty() {
            lines.push(Line::from(Span::styled(
                " no authenticated providers — Space a a",
                sheet.style(StyleClass::Muted),
            )));
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            " Enter: set default · Esc: close",
            sheet.style(StyleClass::Muted),
        )));

        Element::fixed(
            frame,
            Element::cleared(Element::Bordered {
                title: Some(Line::from(" models ")),
                border_style: sheet.style(StyleClass::Muted),
                style: sheet.style(StyleClass::Surface),
                child: Box::new(Element::Text {
                    lines,
                    style: Style::default(),
                    wrap: false,
                }),
            }),
        )
    }

    fn handle_key(&mut self, key: KeyEvent, app: &App) -> Handled {
        let rows = self.rows(app);
        match key.code {
            KeyCode::Esc => Handled::one(AppAction::CloseSelf),
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(&rows, 1);
                Handled::quiet()
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(&rows, -1);
                Handled::quiet()
            }
            KeyCode::Enter => {
                let Some(row) = rows.get(self.selected) else {
                    return Handled::quiet();
                };
                let Some(provider) = &row.provider else {
                    return Handled::quiet();
                };
                if row.label.starts_with("  loading") || row.label.starts_with("  ⚠") {
                    return Handled::quiet();
                }
                let model = row.label.trim().to_string();
                Handled::Acted(vec![
                    AppAction::SetModel {
                        provider: provider.clone(),
                        model,
                    },
                    AppAction::CloseSelf,
                ])
            }
            _ => Handled::quiet(),
        }
    }

    fn observe(&mut self, _event: crate::action::ActionEvent, _app: &App) {
        // The rows are derived from App state each render/press; nothing
        // to cache. Keep the selection pointing at a real row.
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "agent-models"
    }
}
