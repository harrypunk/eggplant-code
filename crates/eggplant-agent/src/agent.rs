//! The agent loop: a turn is one provider stream plus the tool calls it
//! requested; the run ends when a turn makes no tool calls. Events flow
//! out over the session channel; host access flows through `HostClient`.
//!
//! Sequential tool execution by design: tools mutate a shared editor,
//! and sequential is correct-by-construction (docs/design/agent.md).

use std::collections::VecDeque;
use std::path::PathBuf;

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::host::HostClient;
use crate::provider::Provider;
use crate::session::{AgentCommand, AgentEvent};
use crate::store::SessionStore;
use crate::tool::Tool;
use crate::types::{ChatEvent, ChatRequest, Message, Role, ToolCall};

/// Safety cap on turns per run (pi-style guard against runaway loops).
const MAX_TURNS: usize = 25;

pub struct AgentRuntime {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    system_prompt: String,
    host: HostClient,
    events: mpsc::UnboundedSender<AgentEvent>,
    /// The model-facing transcript (persisted to `store` as it grows).
    messages: Vec<Message>,
    store: Option<SessionStore>,
    /// Prompts that arrived mid-run; drained when the run settles.
    queued: VecDeque<String>,
}

impl AgentRuntime {
    pub fn new(
        provider: Box<dyn Provider>,
        tools: Vec<Box<dyn Tool>>,
        system_prompt: String,
        host: HostClient,
        events: mpsc::UnboundedSender<AgentEvent>,
        store: Option<SessionStore>,
        _cwd: PathBuf,
    ) -> Self {
        // Continue the persisted session if there is one.
        let messages = store.as_ref().map(|s| s.load()).unwrap_or_default();
        Self {
            messages,
            provider,
            tools,
            system_prompt,
            host,
            events,
            store,
            queued: VecDeque::new(),
        }
    }

    /// Transcript append + persist (write at event time; a failed write
    /// never breaks the run — persistence is best-effort).
    fn record(&mut self, message: Message) {
        if let Some(store) = &self.store {
            let _ = store.append(&message);
        }
        self.messages.push(message);
    }

    /// The main task: process commands forever.
    pub async fn run(mut self, mut commands: mpsc::UnboundedReceiver<AgentCommand>) {
        // Continue-UX: tell the UI what we loaded so it can rebuild the
        // transcript before the user types.
        if !self.messages.is_empty() {
            self.emit(AgentEvent::Restored {
                messages: self.messages.clone(),
            })
            .await;
        }
        while let Some(command) = commands.recv().await {
            match command {
                AgentCommand::Prompt(text) => self.run_prompt(text, &mut commands).await,
                AgentCommand::Abort => {} // idle: nothing to abort
                AgentCommand::NewChat => {
                    self.messages.clear();
                    self.queued.clear();
                    if let Some(store) = &self.store {
                        store.clear();
                    }
                    self.emit(AgentEvent::Cleared).await;
                }
            }
        }
    }

    async fn emit(&self, event: AgentEvent) {
        let _ = self.events.send(event);
    }

    async fn run_prompt(
        &mut self,
        text: String,
        commands: &mut mpsc::UnboundedReceiver<AgentCommand>,
    ) {
        self.emit(AgentEvent::RunStarted).await;
        self.record(Message::user(text));
        let mut aborted = false;
        for _turn in 0..MAX_TURNS {
            let Some(calls) = self.stream_turn(commands, &mut aborted).await else {
                break; // error event already emitted; run ends
            };
            if aborted {
                self.close_pending_calls(calls, "aborted by user").await;
                break;
            }
            if calls.is_empty() {
                break; // clean finish
            }
            for call in calls {
                let summary = summarize_call(&call);
                self.emit(AgentEvent::ToolStarted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    summary,
                })
                .await;
                let output = match self.tools.iter().find(|t| t.name() == call.name) {
                    Some(tool) => tool.execute(call.args.clone(), &self.host).await,
                    None => crate::tool::ToolOutput::error(format!("unknown tool '{}'", call.name)),
                };
                self.emit(AgentEvent::ToolFinished {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    is_error: output.is_error,
                })
                .await;
                self.messages.push(Message::tool_result(
                    call.id,
                    output.content,
                    output.is_error,
                ));
            }
        }
        self.emit(if aborted {
            AgentEvent::RunFinished { aborted: true }
        } else {
            AgentEvent::RunFinished { aborted: false }
        })
        .await;
        // Drain queued prompts into the next runs.
        while let Some(text) = self.queued.pop_front() {
            Box::pin(self.run_prompt(text, commands)).await;
        }
    }

    /// An abort left these calls unanswered: close each with an error
    /// result so the stored history stays valid — an assistant message
    /// with tool_calls must be followed by their tool messages (strict
    /// providers 400 otherwise).
    async fn close_pending_calls(&mut self, calls: Vec<ToolCall>, reason: &str) {
        for call in calls {
            self.emit(AgentEvent::ToolFinished {
                id: call.id.clone(),
                name: call.name.clone(),
                is_error: true,
            })
            .await;
            self.record(Message::tool_result(call.id, reason.to_string(), true));
        }
    }

    /// One provider stream. Returns the tool calls requested, or `None`
    /// on error/abort (both already reported). Handles mid-run commands:
    /// Abort cancels the stream; Prompt queues for after the run.
    async fn stream_turn(
        &mut self,
        commands: &mut mpsc::UnboundedReceiver<AgentCommand>,
        aborted: &mut bool,
    ) -> Option<Vec<ToolCall>> {
        let request = ChatRequest {
            system: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: self.tools.iter().map(|t| t.declaration()).collect(),
        };
        // The stream borrows `self.provider` for its whole scope — keep
        // that scope in a block so recording (a &mut self) can follow.
        let (text, calls, failed) = {
            let stream = self.provider.stream(&request);
            futures::pin_mut!(stream);
            let mut text = String::new();
            let mut calls = Vec::new();
            let mut failed = false;
            loop {
                let event = tokio::select! {
                    event = stream.next() => event,
                    command = commands.recv() => {
                        match command {
                            Some(AgentCommand::Abort) => {
                                *aborted = true;
                                self.emit(AgentEvent::Aborted).await;
                            }
                            Some(AgentCommand::Prompt(text)) => self.queued.push_back(text),
                            // Mid-run NewChat: queued prompts die with the old
                            // transcript; the clear happens when the run
                            // settles (abort first for an immediate reset).
                            Some(AgentCommand::NewChat) => self.queued.clear(),
                            None => {}
                        }
                        continue;
                    }
                };
                let Some(event) = event else { break };
                match event {
                    ChatEvent::TextDelta(delta) => {
                        text.push_str(&delta);
                        self.emit(AgentEvent::TextDelta(delta)).await;
                    }
                    // Display-only: thinking is shown but not persisted —
                    // it must never be replayed to the model.
                    ChatEvent::ThinkDelta(delta) => {
                        self.emit(AgentEvent::ThinkDelta(delta)).await;
                    }
                    ChatEvent::ToolCall(call) => calls.push(call),
                    ChatEvent::Done => break,
                    ChatEvent::Error(message) => {
                        self.emit(AgentEvent::Error(message)).await;
                        failed = true;
                        break;
                    }
                }
            }
            (text, calls, failed)
        };
        // Never persist an empty assistant message — strict providers
        // (kimi) reject `role: assistant` with empty content on the
        // next request, and an empty turn carries no information.
        if !(text.is_empty() && calls.is_empty()) {
            self.record(Message {
                role: Role::Assistant,
                text,
                tool_calls: calls.clone(),
                ..Message::default()
            });
        }
        if failed {
            return None;
        }
        Some(calls)
    }
}

/// A short human-facing summary of a tool call for the UI chips.
pub fn summarize_call(call: &ToolCall) -> String {
    let path = call
        .args
        .get("path")
        .and_then(|p| p.as_str())
        .or_else(|| call.args.get("pattern").and_then(|p| p.as_str()))
        .or_else(|| call.args.get("query").and_then(|p| p.as_str()));
    match path {
        Some(p) => format!("{} {p}", call.name),
        None => call.name.clone(),
    }
}
