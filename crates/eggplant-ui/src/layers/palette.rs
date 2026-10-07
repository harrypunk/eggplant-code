//! The command palette: a picker over the command registry.

use crate::commands::Command;
use crate::compositor::KeyResult;

use super::picker::{Picker, PickerSource, PickerSpec};

pub fn command_palette(commands: Vec<Command>) -> Picker<Command> {
    Picker::new(PickerSpec {
        title: "palette",
        // Registered but palette-hidden commands (edit.*) stay out.
        source: PickerSource::List {
            items: commands.into_iter().filter(|c| c.palette).collect(),
            text_of: |command| command.id,
        },
        project: |command| (command.id.to_owned(), command.description.to_owned()),
        on_select: |command, _| KeyResult::Execute(*command),
        preview_of: None,
    })
}
