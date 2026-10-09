//! The auth layer (`Space a a`): pick a provider, paste its key, we
//! validate against the provider and save to `~/.eggplant/agent/auth.toml`.
//! Keys from the environment are shown but managed outside (the env var).

use eggplant_agent::KeySource;
use eggplant_core::input::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::action::{AppAction, Handled};
use crate::app::App;
use crate::compositor::{Layer, LayerKind};
use crate::element::Element;
use crate::stylesheet::{StyleClass, Stylesheet};

/// One provider row's auth status (computed at open / after a save).
struct ProviderRow {
    name: &'static str,
    default_model: &'static str,
    /// The working key's source + masked form; None = not configured.
    configured: Option<(KeySource, String)>,
}

fn compute_rows() -> Vec<ProviderRow> {
    let store = eggplant_agent::AuthStore::load_default();
    eggplant_agent::provider::PRESETS
        .iter()
        .map(|preset| {
            let configured = std::env::var(preset.api_key_env)
                .ok()
                .filter(|k| !k.is_empty())
                .map(|k| (KeySource::Env, eggplant_agent::mask(&k)))
                .or_else(|| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(preset.name))
                        .map(|k| (KeySource::File, eggplant_agent::mask(k)))
                });
            ProviderRow {
                name: preset.name,
                default_model: preset.default_model,
                configured,
            }
        })
        .collect()
}

/// Which form field is focused (Tab/Enter moves url → key).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Url,
    Key,
}

enum Mode {
    Select,
    /// The edit form: URL (prefilled with the effective endpoint) + key.
    Form {
        field: Field,
        url: String,
    },
}

pub struct AuthLayer {
    rows: Vec<ProviderRow>,
    selected: usize,
    mode: Mode,
    /// The key being pasted (masked on screen).
    input: String,
    /// Last auth generation we rendered (observe re-reads on bump only).
    seen_generation: u64,
}

impl AuthLayer {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            rows: compute_rows(),
            selected: 0,
            mode: Mode::Select,
            input: String::new(),
            seen_generation: 0,
        })
    }
}

impl Layer for AuthLayer {
    fn view(&self, area: Rect, app: &App, _focused: bool) -> Element {
        let sheet = Stylesheet::new(&app.theme.current);
        let width = 78u16.min(area.width.saturating_sub(4));
        // Form mode adds provider header + two fields.
        let extra = match self.mode {
            Mode::Select => 2u16,
            Mode::Form { .. } => 5,
        };
        let content_lines = self.rows.len() as u16 + extra;
        let height = (content_lines + 2).min(area.height.saturating_sub(2));
        let frame = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );

        let mut lines = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let status = match &row.configured {
                Some((KeySource::Env, masked)) => format!("✓ env  {masked}"),
                Some((KeySource::File, masked)) => format!("✓ key  {masked}"),
                None => "✗ not configured".to_string(),
            };
            let status_class = if row.configured.is_some() {
                StyleClass::Info
            } else {
                StyleClass::Muted
            };
            let selected = i == self.selected && matches!(self.mode, Mode::Select);
            let name_style = if selected {
                sheet.emphasized(StyleClass::Selected)
            } else {
                sheet.style(StyleClass::Text)
            };
            let env_var = eggplant_agent::preset(row.name)
                .map(|p| p.api_key_env)
                .unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled(if selected { " ▸ " } else { "   " }, name_style),
                Span::styled(format!("{:<10}", row.name), name_style),
                Span::styled(format!("{status:<24}"), sheet.style(status_class)),
                Span::styled(format!("{env_var:<22}"), sheet.style(StyleClass::Muted)),
                Span::styled(row.default_model, sheet.style(StyleClass::Muted)),
            ]));
        }
        lines.push(Line::default());

        let content = match &self.mode {
            Mode::Select => {
                lines.push(Line::from(Span::styled(
                    " Enter: edit key & endpoint · Esc: close · or edit ~/.eggplant/agent/auth.toml",
                    sheet.style(StyleClass::Muted),
                )));
                Element::Text {
                    lines,
                    style: Style::default(),
                    wrap: false,
                }
            }
            Mode::Form { field, url } => {
                let field_row = |label: &str, focused: bool, display: String| {
                    let label_style = if focused {
                        sheet.emphasized(StyleClass::Accent)
                    } else {
                        sheet.style(StyleClass::Muted)
                    };
                    // Only the focused field places a cursor (Input puts
                    // it after the text); the other is plain text.
                    if focused {
                        Element::Input {
                            prompt: Line::styled(format!(" {label:<9} "), label_style),
                            text: display,
                            style: sheet.style(StyleClass::Text),
                        }
                    } else {
                        Element::Text {
                            lines: vec![Line::from(vec![
                                Span::styled(format!(" {label:<9} "), label_style),
                                Span::styled(display, sheet.style(StyleClass::Text)),
                            ])],
                            style: Style::default(),
                            wrap: false,
                        }
                    }
                };
                let provider = self.rows[self.selected].name;
                lines.push(Line::from(Span::styled(
                    format!(" {provider} "),
                    sheet.style(StyleClass::Title),
                )));
                let key_display = "•".repeat(self.input.chars().count());
                Element::Layout {
                    direction: Direction::Vertical,
                    constraints: vec![
                        Constraint::Length(lines.len() as u16),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ],
                    children: vec![
                        Element::Text {
                            lines,
                            style: Style::default(),
                            wrap: false,
                        },
                        field_row("endpoint", *field == Field::Url, url.clone()),
                        field_row("key", *field == Field::Key, key_display),
                        Element::Text {
                            lines: vec![Line::from(Span::styled(
                                " Tab: next field · Enter: validate & save · Esc: back",
                                sheet.style(StyleClass::Muted),
                            ))],
                            style: Style::default(),
                            wrap: false,
                        },
                    ],
                }
            }
        };

        Element::fixed(
            frame,
            Element::cleared(Element::Bordered {
                title: Some(Line::from(" agent auth ")),
                border_style: sheet.style(StyleClass::Muted),
                style: sheet.style(StyleClass::Surface),
                child: Box::new(content),
            }),
        )
    }

    fn handle_key(&mut self, key: KeyEvent, _app: &App) -> Handled {
        match self.mode {
            Mode::Select => match key.code {
                KeyCode::Esc => Handled::one(AppAction::CloseSelf),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.selected = (self.selected + 1) % self.rows.len();
                    Handled::quiet()
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.selected = (self.selected + self.rows.len() - 1) % self.rows.len();
                    Handled::quiet()
                }
                KeyCode::Enter => {
                    let preset = eggplant_agent::preset(self.rows[self.selected].name)
                        .expect("rows come from PRESETS");
                    // Prefill with the effective endpoint: stored override,
                    // else the preset default — the user edits what is real.
                    let url = eggplant_agent::AuthStore::load_default()
                        .and_then(|store| store.url_for(preset.name).map(str::to_owned))
                        .unwrap_or_else(|| preset.base_url.to_string());
                    self.mode = Mode::Form {
                        field: Field::Url,
                        url,
                    };
                    self.input.clear();
                    Handled::quiet()
                }
                _ => Handled::quiet(),
            },
            Mode::Form {
                ref field,
                ref mut url,
            } => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::Select;
                    Handled::quiet()
                }
                KeyCode::Tab => {
                    let next = match field {
                        Field::Url => Field::Key,
                        Field::Key => Field::Url,
                    };
                    self.mode = Mode::Form {
                        field: next,
                        url: std::mem::take(url),
                    };
                    Handled::quiet()
                }
                KeyCode::Enter => match field {
                    Field::Url => {
                        self.mode = Mode::Form {
                            field: Field::Key,
                            url: std::mem::take(url),
                        };
                        Handled::quiet()
                    }
                    Field::Key => {
                        let key_text = self.input.trim().to_owned();
                        if key_text.is_empty() {
                            return Handled::quiet();
                        }
                        let provider = self.rows[self.selected].name.to_string();
                        let base_url = {
                            let url = std::mem::take(url).trim().to_owned();
                            if url.is_empty() {
                                eggplant_agent::preset(&provider)
                                    .expect("rows come from PRESETS")
                                    .base_url
                                    .to_string()
                            } else {
                                url
                            }
                        };
                        self.mode = Mode::Select;
                        self.input.clear();
                        Handled::Acted(vec![
                            AppAction::AuthSubmit {
                                provider,
                                key: key_text,
                                base_url,
                            },
                            AppAction::CloseSelf,
                        ])
                    }
                },
                KeyCode::Backspace => {
                    match field {
                        Field::Url => {
                            url.pop();
                        }
                        Field::Key => {
                            self.input.pop();
                        }
                    }
                    Handled::quiet()
                }
                KeyCode::Char(c)
                    if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
                {
                    match field {
                        Field::Url => url.push(c),
                        Field::Key => self.input.push(c),
                    }
                    Handled::quiet()
                }
                _ => Handled::quiet(),
            },
        }
    }

    fn observe(&mut self, _event: crate::action::ActionEvent, app: &App) {
        // Re-read auth.toml only when a save bumped the generation —
        // observe fires per action, so an unconditional read would be a
        // file read per keystroke.
        if app.agent.auth_generation != self.seen_generation {
            self.seen_generation = app.agent.auth_generation;
            self.rows = compute_rows();
        }
    }

    fn kind(&self) -> LayerKind {
        LayerKind::Float
    }

    fn id(&self) -> &'static str {
        "agent-auth"
    }
}
