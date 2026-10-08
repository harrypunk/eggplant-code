//! The LLM-facing vocabulary: messages, tool calls, streaming events.
//! Provider adapters normalize their wire formats to these types.

use serde::{Deserialize, Serialize};

/// Conversation role. Tool results are their own role (mapped per provider:
/// Anthropic `user` with a `tool_result` block, OpenAI `role: "tool"`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[default]
    User,
    Assistant,
    Tool,
}

/// One message in the model-facing transcript.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Text content (assistant replies, user prompts, tool results).
    pub text: String,
    /// Tool calls the assistant requested (assistant messages only).
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// The call this message answers (tool messages only).
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// Provider flag: this tool result is an error.
    #[serde(default)]
    pub is_error: bool,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn tool_result(call_id: impl Into<String>, content: String, is_error: bool) -> Self {
        Self {
            role: Role::Tool,
            text: content,
            tool_call_id: Some(call_id.into()),
            is_error,
            ..Self::default()
        }
    }
}

/// A tool call requested by the assistant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Parsed JSON arguments (validated loosely; tools validate strictly).
    pub args: serde_json::Value,
}

/// A tool declaration for the provider (name + description + JSON schema).
#[derive(Clone, Debug)]
pub struct ToolDecl {
    pub name: &'static str,
    pub description: &'static str,
    pub params_schema: serde_json::Value,
}

/// One request to a provider: system prompt + transcript + tool decls.
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDecl>,
}

/// Normalized streaming event (pi's `AssistantMessageEvent`, reduced).
/// Text deltas arrive as they stream; tool calls arrive complete
/// (arguments are accumulated provider-side in the adapter).
#[derive(Clone, Debug, PartialEq)]
pub enum ChatEvent {
    TextDelta(String),
    ToolCall(ToolCall),
    /// The assistant turn ended cleanly (stop or tool-use boundary).
    Done,
    /// Transport or provider failure.
    Error(String),
}
