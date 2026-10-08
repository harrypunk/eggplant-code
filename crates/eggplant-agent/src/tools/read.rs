//! `read` — file contents with line paging.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::host::{HostCall, HostClient, HostReply};
use crate::tool::{Tool, ToolOutput};
use crate::tools::{arg_path, arg_usize};

pub struct Read;

#[async_trait]
impl Tool for Read {
    fn name(&self) -> &'static str {
        "read"
    }

    fn snippet(&self) -> &'static str {
        "Read file contents"
    }

    fn guidelines(&self) -> &'static [&'static str] {
        &["Use read to examine files instead of guessing their contents."]
    }

    fn params_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file (relative or absolute)" },
                "offset": { "type": "integer", "description": "Line to start from (1-indexed)" },
                "limit": { "type": "integer", "description": "Maximum number of lines" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, host: &HostClient) -> ToolOutput {
        let call = || -> Result<HostCall, String> {
            Ok(HostCall::Read {
                path: arg_path(&args, "path")?,
                offset: arg_usize(&args, "offset")?,
                limit: arg_usize(&args, "limit")?,
            })
        };
        let call = match call() {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(e),
        };
        match host.call(call).await {
            Ok(HostReply::Text(text)) => ToolOutput::ok(text),
            Ok(_) => ToolOutput::error("unexpected host reply"),
            Err(e) => ToolOutput::error(e),
        }
    }
}
