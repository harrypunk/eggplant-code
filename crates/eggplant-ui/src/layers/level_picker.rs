//! The log level picker: the viewer's (arrow-key, Enter) level menu.
//! Opened with `L` inside the log buffer — the same widget as every
//! other picker (float, fuzzy, arrows), no new UI machinery.

use log::LevelFilter;

use crate::action::AppAction;
use crate::app::App;

use super::picker::{Picker, PickerSource, PickerSpec};

const LEVELS: [(LevelFilter, &str); 5] = [
    (LevelFilter::Trace, "verbose (everything)"),
    (LevelFilter::Debug, "debug"),
    (LevelFilter::Info, "info"),
    (LevelFilter::Warn, "warn"),
    (LevelFilter::Error, "errors only"),
];

pub fn log_level_picker(app: &App) -> Picker<(String, LevelFilter)> {
    // Labels carry the current marker — project fns are pure, so the
    // "●" is baked into the item at construction (event time).
    let items: Vec<(String, LevelFilter)> = LEVELS
        .iter()
        .map(|(level, name)| {
            let label = if *level == app.logs_level {
                format!("● {name}")
            } else {
                format!("  {name}")
            };
            (label, *level)
        })
        .collect();
    Picker::new(PickerSpec {
        title: "log level",
        source: PickerSource::List {
            items,
            text_of: |item| &item.0,
        },
        project: |item| (item.0.clone(), String::new()),
        on_select: |item| AppAction::OpenLogs { min: item.1 },
        preview_of: None,
    })
}
