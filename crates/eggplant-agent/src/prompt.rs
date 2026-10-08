//! System prompt composition (pi's pattern): fixed preamble + each
//! tool's snippet and guidelines + the project's AGENTS.md if present.

use std::path::Path;

use crate::tool::Tool;

/// Build the system prompt for a run.
pub fn build_system_prompt(tools: &[Box<dyn Tool>], cwd: &Path, agents_md: Option<&str>) -> String {
    let tool_list = tools
        .iter()
        .map(|t| format!("- {}: {}", t.name(), t.snippet()))
        .collect::<Vec<_>>()
        .join("\n");
    let guidelines = tools
        .iter()
        .flat_map(|t| t.guidelines().iter().copied())
        .map(|g| format!("- {g}"))
        .collect::<Vec<_>>()
        .join("\n");
    let project = agents_md
        .map(|md| {
            format!(
                "\n\nProject instructions (AGENTS.md) — follow them exactly:\n\n<project-instructions>\n{md}\n</project-instructions>"
            )
        })
        .unwrap_or_default();
    format!(
        "You are the built-in editing agent of eggplant-code, working on the user's project at {}.\n\
         The user's editor is your hands: your read/write/edit/grep/find tools operate on the live \
         editor state — edits land in the user's buffers immediately and can be undone by the user. \
         Be precise, be brief, and prefer small focused changes.\n\n\
         Tools:\n{tool_list}\n\n\
         Guidelines:\n{guidelines}{project}",
        cwd.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_composes_tools_and_project() {
        let tools = crate::tool::default_tools();
        let prompt = build_system_prompt(&tools, Path::new("/tmp/x"), Some("RULE ONE"));
        assert!(prompt.contains("- read: Read file contents"));
        assert!(prompt.contains("- grep:"));
        assert!(prompt.contains("RULE ONE"));
        assert!(prompt.contains("/tmp/x"));
    }
}
