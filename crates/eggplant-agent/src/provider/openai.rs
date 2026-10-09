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
        let mut ids = IdRemap::default();
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
                .map(|m| wire_message(m, &mut ids)),
        );
        repair_tool_results(&mut messages);
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

/// Reassigns tool-call ids to be unique across the whole conversation.
/// Some providers (kimi) mint per-turn ids like `find:1`, which collide
/// once the history holds two turns — and strict providers reject the
/// request. Each call occurrence gets a fresh id; each tool result takes
/// the id of the latest matching call, so call↔result pairing survives.
#[derive(Default)]
struct IdRemap {
    latest: std::collections::HashMap<String, String>,
    next: usize,
}

impl IdRemap {
    fn call(&mut self, old: &str) -> String {
        self.next += 1;
        let fresh = format!("call_{}", self.next);
        self.latest.insert(old.to_string(), fresh.clone());
        fresh
    }

    fn result(&self, old: &str) -> String {
        self.latest
            .get(old)
            .cloned()
            .unwrap_or_else(|| old.to_string())
    }
}

/// Guarantee the provider invariant: every assistant `tool_calls` id is
/// answered by a following tool message. History can violate it (an
/// aborted run, a crash between records, an old session file) — insert a
/// placeholder result for each unanswered call rather than 400.
fn repair_tool_results(messages: &mut Vec<Value>) {
    let mut i = 0;
    while i < messages.len() {
        let is_calling_assistant = messages[i]["role"] == "assistant"
            && messages[i]["tool_calls"]
                .as_array()
                .is_some_and(|c| !c.is_empty());
        if !is_calling_assistant {
            i += 1;
            continue;
        }
        let wanted: Vec<String> = messages[i]["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|c| c["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        // The results that do follow (consecutive tool messages).
        let mut answered = std::collections::HashSet::new();
        let mut j = i + 1;
        while j < messages.len() && messages[j]["role"] == "tool" {
            if let Some(id) = messages[j]["tool_call_id"].as_str() {
                answered.insert(id.to_string());
            }
            j += 1;
        }
        let missing: Vec<String> = wanted
            .into_iter()
            .filter(|id| !answered.contains(id))
            .collect();
        for (k, id) in missing.iter().enumerate() {
            messages.insert(
                j + k,
                json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": "(no result recorded — the run was interrupted)",
                }),
            );
        }
        i = j + missing.len();
    }
}

fn wire_message(message: &Message, ids: &mut IdRemap) -> Value {
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
                            "id": ids.call(&c.id),
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
            "tool_call_id": message
                .tool_call_id
                .as_deref()
                .map(|id| ids.result(id)),
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
    fn unanswered_tool_calls_get_placeholder_results() {
        // An aborted run recorded the calls but only one result.
        let mut assistant = Message {
            role: Role::Assistant,
            ..Message::default()
        };
        for id in ["a:1", "a:2"] {
            assistant.tool_calls.push(ToolCall {
                id: id.to_string(),
                name: "read".to_string(),
                args: serde_json::json!({}),
            });
        }
        let mut result = Message::tool_result("a:1", "done".to_string(), false);
        result.tool_call_id = Some("a:1".to_string());
        let body = provider().body(&request(vec![Message::user("go"), assistant, result]));
        let wire = body["messages"].as_array().unwrap();
        // system, user, assistant(2 calls), real result, synthesized result.
        assert_eq!(wire.len(), 5);
        assert_eq!(wire[3]["role"], "tool");
        assert_eq!(wire[4]["role"], "tool");
        assert_eq!(wire[4]["tool_call_id"], wire[2]["tool_calls"][1]["id"]);
    }

    #[test]
    fn duplicated_provider_tool_ids_are_remapped_unique() {
        // Two turns where the provider reused the id "find:1" (kimi does).
        let turn = |text: &str| {
            let mut assistant = Message {
                role: Role::Assistant,
                ..Message::default()
            };
            assistant.text = text.to_string();
            assistant.tool_calls.push(ToolCall {
                id: "find:1".to_string(),
                name: "find".to_string(),
                args: serde_json::json!({}),
            });
            let mut result = Message {
                role: Role::Tool,
                ..Message::default()
            };
            result.tool_call_id = Some("find:1".to_string());
            result.text = "hits".to_string();
            vec![assistant, result]
        };
        let mut messages = vec![Message::user("go")];
        messages.extend(turn("first"));
        messages.extend(turn("second"));
        let body = provider().body(&request(messages));
        let wire = body["messages"].as_array().unwrap();
        let id_a = wire[2]["tool_calls"][0]["id"].as_str().unwrap();
        let id_b = wire[4]["tool_calls"][0]["id"].as_str().unwrap();
        assert_ne!(id_a, id_b, "duplicated provider ids must be remapped");
        assert_eq!(wire[3]["tool_call_id"], id_a, "result pairs its call");
        assert_eq!(wire[5]["tool_call_id"], id_b, "result pairs its call");
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
