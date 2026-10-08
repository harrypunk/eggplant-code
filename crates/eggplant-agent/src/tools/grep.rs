//! `grep` — project content search (the host runs the same engine and
//! caps as the interactive live grep).

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::host::{HostCall, HostClient, HostReply};
use crate::tool::{Tool, ToolOutput};
use crate::tools::arg_str;

pub struct Grep;

#[async_trait]
impl Tool for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn snippet(&self) -> &'static str {
        "Search file contents across the project (regex)"
    }

    fn guidelines(&self) -> &'static [&'static str] {
        &["Use grep to find usages, definitions, and patterns by content."]
    }

    fn params_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, host: &HostClient) -> ToolOutput {
        let pattern = match arg_str(&args, "pattern") {
            Ok(p) => p.to_owned(),
            Err(e) => return ToolOutput::error(e),
        };
        match host.call(HostCall::Grep { pattern }).await {
            Ok(HostReply::Hits(hits)) => {
                if hits.is_empty() {
                    return ToolOutput::ok("no matches");
                }
                let body = hits
                    .iter()
                    .map(|(path, line, text)| format!("{}:{line}: {text}", path.display()))
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutput::ok(body)
            }
            Ok(_) => ToolOutput::error("unexpected host reply"),
            Err(e) => ToolOutput::error(e),
        }
    }
}
