//! The `Tool` trait: self-describing, model-facing. Each tool declares
//! its params schema, a one-line snippet + guidelines for the system
//! prompt (pi's composition pattern), and `execute`.

use serde_json::Value;

use crate::host::HostClient;
use crate::types::ToolDecl;

/// The outcome of a tool call: model-facing text + error flag.
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
        }
    }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    /// One line for the system prompt's tool list.
    fn snippet(&self) -> &'static str;
    /// Guideline bullets for the system prompt.
    fn guidelines(&self) -> &'static [&'static str] {
        &[]
    }
    /// JSON Schema for the parameters object.
    fn params_schema(&self) -> Value;

    fn declaration(&self) -> ToolDecl {
        ToolDecl {
            name: self.name(),
            description: self.snippet(),
            params_schema: self.params_schema(),
        }
    }

    async fn execute(&self, args: Value, host: &HostClient) -> ToolOutput;
}

/// The v1 tool set: read / write / edit / grep / find.
pub fn default_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(crate::tools::read::Read),
        Box::new(crate::tools::write::Write),
        Box::new(crate::tools::edit::Edit),
        Box::new(crate::tools::grep::Grep),
        Box::new(crate::tools::find::Find),
    ]
}
