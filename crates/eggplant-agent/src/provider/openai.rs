//! OpenAI-compatible chat completions adapter (SSE streaming). Covers
//! OpenAI, OpenRouter, DeepSeek, and local servers (llama.cpp, Ollama,
//! LM Studio) — set `base_url` accordingly.

use futures::{Stream, StreamExt};
use serde_json::{Value, json};

use super::{Provider, ProviderConfig, sse};
use crate::types::{ChatEvent, ChatRequest, Message, Role, ToolCall};

/// A proper User-Agent — some providers (kimi) reject the reqwest
/// default. Proper = identifies the client, per their docs.
const USER_AGENT: &str = concat!("eggplant-code/", env!("CARGO_PKG_VERSION"));

pub struct OpenAi {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAi {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    fn body(&self, request: &ChatRequest) -> Value {
        let mut messages = vec![json!({ "role": "system", "content": request.system })];
        messages.extend(
            request
                .messages
                .iter()
                // An empty assistant turn (no text, no tool calls) is
                // invalid on strict providers — drop it from the wire
                // (older session files may still carry one).
                .filter(|m| {
                    !(m.role == Role::Assistant && m.text.is_empty() && m.tool_calls.is_empty())
                })
                .map(wire_message),
        );
        json!({
            "model": self.config.model,
            "stream": true,
            "messages": messages,
            "tools": request.tools.iter().map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.params_schema,
                },
            })).collect::<Vec<_>>(),
        })
    }
}

fn wire_message(message: &Message) -> Value {
    match message.role {
        Role::User => json!({ "role": "user", "content": message.text }),
        Role::Assistant => {
            // Empty content must be null (not "") when tool calls carry
            // the turn — kimi rejects an empty string here.
            let mut wire = if message.text.is_empty() {
                json!({ "role": "assistant", "content": null })
            } else {
                json!({ "role": "assistant", "content": message.text })
            };
            if !message.tool_calls.is_empty() {
                wire["tool_calls"] = message
                    .tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": c.args.to_string(),
                            },
                        })
                    })
                    .collect();
            }
            wire
        }
        Role::Tool => json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id,
            "content": message.text,
        }),
    }
}

impl Provider for OpenAi {
    fn stream(
        &self,
        request: &ChatRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = ChatEvent> + Send + '_>> {
        let future = self
            .client
            .post(format!("{}/chat/completions", self.config.base_url))
            .bearer_auth(&self.config.api_key)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
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
            // OpenAI streams tool calls as indexed fragments; accumulate.
            let mut tool_calls: std::collections::BTreeMap<usize, (String, String, String)> =
                std::collections::BTreeMap::new();
            let flush_calls = |calls: &mut std::collections::BTreeMap<usize, (String, String, String)>| {
                std::mem::take(calls).into_values().map(|(id, name, raw)| {
                    ChatEvent::ToolCall(ToolCall {
                        id,
                        name,
                        args: serde_json::from_str(&raw).unwrap_or_else(|_| json!({})),
                    })
                }).collect::<Vec<_>>()
            };
            while let Some(frame) = frames.next().await {
                if frame.data.trim() == "[DONE]" {
                    break;
                }
                let Ok(value) = serde_json::from_str::<Value>(&frame.data) else {
                    continue;
                };
                let delta = &value["choices"][0]["delta"];
                if let Some(text) = delta["content"].as_str()
                    && !text.is_empty()
                {
                    yield ChatEvent::TextDelta(text.to_owned());
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for call in calls {
                        let index = call["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_calls.entry(index).or_default();
                        if let Some(id) = call["id"].as_str() {
                            entry.0 = id.to_owned();
                        }
                        if let Some(name) = call["function"]["name"].as_str() {
                            entry.1 = name.to_owned();
                        }
                        if let Some(args) = call["function"]["arguments"].as_str() {
                            entry.2.push_str(args);
                        }
                    }
                }
                if value["choices"][0]["finish_reason"].is_string() {
                    for event in flush_calls(&mut tool_calls) {
                        yield event;
                    }
                    yield ChatEvent::Done;
                }
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> OpenAi {
        OpenAi::new(ProviderConfig {
            name: "test".to_string(),
            model: "m".to_string(),
            api_key: "k".to_string(),
            base_url: "http://localhost".to_string(),
        })
    }

    fn request(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            system: "sys".to_string(),
            messages,
            tools: Vec::new(),
        }
    }

    #[test]
    fn empty_assistant_turns_are_dropped_from_the_wire() {
        let body = provider().body(&request(vec![
            Message::user("hi"),
            Message {
                role: Role::Assistant,
                ..Message::default()
            },
            Message::user("again"),
        ]));
        let messages = body["messages"].as_array().unwrap();
        // system + user + user: the empty assistant message is gone.
        assert_eq!(messages.len(), 3);
        assert!(messages.iter().all(|m| m["role"] != "assistant"));
    }

    #[test]
    fn tool_call_turn_serializes_null_content_not_empty_string() {
        let mut assistant = Message {
            role: Role::Assistant,
            ..Message::default()
        };
        assistant.tool_calls.push(ToolCall {
            id: "c1".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({"path": "a.rs"}),
        });
        let body = provider().body(&request(vec![Message::user("hi"), assistant]));
        let wire = &body["messages"][2];
        assert_eq!(wire["role"], "assistant");
        assert!(wire["content"].is_null());
        assert_eq!(wire["tool_calls"][0]["function"]["name"], "read_file");
    }
}
