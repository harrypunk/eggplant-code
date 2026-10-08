//! Anthropic Messages API adapter (SSE streaming).

use futures::{Stream, StreamExt};
use serde_json::{Value, json};

use super::{Provider, ProviderConfig, sse};
use crate::types::{ChatEvent, ChatRequest, Message, Role, ToolCall};

pub struct Anthropic {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl Anthropic {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    fn body(&self, request: &ChatRequest) -> Value {
        json!({
            "model": self.config.model,
            "max_tokens": 8192,
            "stream": true,
            "system": request.system,
            "messages": request.messages.iter().map(wire_message).collect::<Vec<_>>(),
            "tools": request.tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.params_schema,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Anthropic content blocks: text for user/assistant, tool_use blocks on
/// assistant messages, tool_result blocks (as a user message) for tools.
fn wire_message(message: &Message) -> Value {
    match message.role {
        Role::User => json!({
            "role": "user",
            "content": [{ "type": "text", "text": message.text }],
        }),
        Role::Assistant => {
            let mut content: Vec<Value> = Vec::new();
            if !message.text.is_empty() {
                content.push(json!({ "type": "text", "text": message.text }));
            }
            for call in &message.tool_calls {
                content.push(json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": call.args,
                }));
            }
            json!({ "role": "assistant", "content": content })
        }
        Role::Tool => json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": message.tool_call_id,
                "content": message.text,
                "is_error": message.is_error,
            }],
        }),
    }
}

impl Provider for Anthropic {
    fn stream(
        &self,
        request: &ChatRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = ChatEvent> + Send + '_>> {
        let future = self
            .client
            .post(format!("{}/v1/messages", self.config.base_url))
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.body(request))
            .send();

        Box::pin(async_stream::stream! {
            let response = match future.await {
                Ok(r) => r,
                Err(e) => {
                    yield ChatEvent::Error(format!("request failed: {e}"));
                    return;
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                yield ChatEvent::Error(format!("provider error {status}: {body}"));
                return;
            }
            let frames = sse::decode(response.bytes_stream());
            futures::pin_mut!(frames);
            let mut tool_blocks: std::collections::HashMap<usize, (String, String, String)> =
                std::collections::HashMap::new();
            while let Some(frame) = frames.next().await {
                let Ok(value) = serde_json::from_str::<Value>(&frame.data) else {
                    continue;
                };
                match frame.event.as_deref() {
                    Some("content_block_start") => {
                        let block = &value["content_block"];
                        let index = value["index"].as_u64().unwrap_or(0) as usize;
                        if block["type"] == "tool_use" {
                            tool_blocks.insert(
                                index,
                                (
                                    block["id"].as_str().unwrap_or_default().to_owned(),
                                    block["name"].as_str().unwrap_or_default().to_owned(),
                                    String::new(),
                                ),
                            );
                        }
                    }
                    Some("content_block_delta") => {
                        let index = value["index"].as_u64().unwrap_or(0) as usize;
                        let delta = &value["delta"];
                        match delta["type"].as_str() {
                            Some("text_delta") => {
                                yield ChatEvent::TextDelta(
                                    delta["text"].as_str().unwrap_or_default().to_owned(),
                                );
                            }
                            Some("input_json_delta") => {
                                if let Some(block) = tool_blocks.get_mut(&index) {
                                    block.2.push_str(
                                        delta["partial_json"].as_str().unwrap_or_default(),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    Some("content_block_stop") => {
                        let index = value["index"].as_u64().unwrap_or(0) as usize;
                        if let Some((id, name, raw_args)) = tool_blocks.remove(&index) {
                            yield ChatEvent::ToolCall(ToolCall {
                                id,
                                name,
                                args: serde_json::from_str(&raw_args)
                                    .unwrap_or_else(|_| json!({})),
                            });
                        }
                    }
                    Some("message_stop") => yield ChatEvent::Done,
                    Some("error") => {
                        yield ChatEvent::Error(
                            value["error"]["message"]
                                .as_str()
                                .unwrap_or("provider stream error")
                                .to_owned(),
                        );
                    }
                    _ => {}
                }
            }
        })
    }
}
