//! `find` — project file discovery (the host runs the same walk and
//! fuzzy matcher as the file picker).

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::host::{HostCall, HostClient, HostReply};
use crate::tool::{Tool, ToolOutput};
use crate::tools::arg_str;

pub struct Find;

#[async_trait]
impl Tool for Find {
    fn name(&self) -> &'static str {
        "find"
    }

    fn snippet(&self) -> &'static str {
        "Find files by name across the project (fuzzy)"
    }

    fn guidelines(&self) -> &'static [&'static str] {
        &["Use find to locate files by path fragments; use grep for content."]
    }

    fn params_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Fuzzy path fragment, e.g. 'edit rs'" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value, host: &HostClient) -> ToolOutput {
        let query = match arg_str(&args, "query") {
            Ok(q) => q.to_owned(),
            Err(e) => return ToolOutput::error(e),
        };
        match host.call(HostCall::Find { query }).await {
            Ok(HostReply::Paths(paths)) => {
                if paths.is_empty() {
                    return ToolOutput::ok("no files match");
                }
                let body = paths
                    .iter()
                    .take(50)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutput::ok(body)
            }
            Ok(_) => ToolOutput::error("unexpected host reply"),
            Err(e) => ToolOutput::error(e),
        }
    }
}
