//! `write` — create or overwrite a file.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::host::{HostCall, HostClient, HostReply};
use crate::tool::{Tool, ToolOutput};
use crate::tools::{arg_path, arg_str};

pub struct Write;

#[async_trait]
impl Tool for Write {
    fn name(&self) -> &'static str {
        "write"
    }

    fn snippet(&self) -> &'static str {
        "Create or overwrite a file"
    }

    fn guidelines(&self) -> &'static [&'static str] {
        &[
            "Prefer edit over write for existing files — write replaces the whole content.",
            "Use write for new files or complete rewrites.",
        ]
    }

    fn params_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file (relative or absolute)" },
                "content": { "type": "string", "description": "The full file content" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value, host: &HostClient) -> ToolOutput {
        let call = || -> Result<HostCall, String> {
            Ok(HostCall::Write {
                path: arg_path(&args, "path")?,
                content: arg_str(&args, "content")?.to_owned(),
            })
        };
        let call = match call() {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(e),
        };
        match host.call(call).await {
            Ok(HostReply::Ok) => ToolOutput::ok("file written"),
            Ok(_) => ToolOutput::error("unexpected host reply"),
            Err(e) => ToolOutput::error(e),
        }
    }
}
