//! System prompt composition (pi's pattern): fixed preamble + each
//! tool's snippet and guidelines + the project's AGENTS.md if present.

use std::path::Path;

use crate::tool::Tool;

/// The prompt template, embedded. Placeholders: `{cwd}`, `{tools}`,
/// `{guidelines}`, `{project}`. Override at runtime without rebuilding:
/// `$EGGPLANT_HOME/system.md` shadows this file (edit + restart).
const DEFAULT_TEMPLATE: &str = include_str!("system_prompt.md");

/// The effective template: the runtime override when present, else the
/// embedded default.
fn template() -> std::borrow::Cow<'static, str> {
    let override_path = crate::store::data_root()
        .map(|root| root.join("system.md"))
        .and_then(|path| std::fs::read_to_string(path).ok());
    match override_path {
        Some(text) => std::borrow::Cow::Owned(text),
        None => std::borrow::Cow::Borrowed(DEFAULT_TEMPLATE),
    }
}

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
                "\nProject instructions (AGENTS.md) — follow them exactly:\n\n<project-instructions>\n{md}\n</project-instructions>\n"
            )
        })
        .unwrap_or_default();
    template()
        .replace("{cwd}", &cwd.display().to_string())
        .replace("{tools}", &tool_list)
        .replace("{guidelines}", &guidelines)
        .replace("{project}", &project)
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
