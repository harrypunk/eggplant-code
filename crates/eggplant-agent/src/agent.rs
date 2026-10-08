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
    /// The model-facing transcript.
    messages: Vec<Message>,
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
        _cwd: PathBuf,
    ) -> Self {
        Self {
            provider,
            tools,
            system_prompt,
            host,
            events,
            messages: Vec::new(),
            queued: VecDeque::new(),
        }
    }

    /// The main task: process commands forever.
    pub async fn run(mut self, mut commands: mpsc::UnboundedReceiver<AgentCommand>) {
        while let Some(command) = commands.recv().await {
            match command {
                AgentCommand::Prompt(text) => self.run_prompt(text, &mut commands).await,
                AgentCommand::Abort => {} // idle: nothing to abort
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
        self.messages.push(Message::user(text));
        let mut aborted = false;
        for _turn in 0..MAX_TURNS {
            let Some(calls) = self.stream_turn(commands, &mut aborted).await else {
                break; // error event already emitted; run ends
            };
            if aborted {
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
        let stream = self.provider.stream(&request);
        futures::pin_mut!(stream);
        let mut text = String::new();
        let mut calls = Vec::new();
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
                ChatEvent::ToolCall(call) => calls.push(call),
                ChatEvent::Done => break,
                ChatEvent::Error(message) => {
                    self.emit(AgentEvent::Error(message.clone())).await;
                    self.messages.push(Message {
                        role: Role::Assistant,
                        text,
                        ..Message::default()
                    });
                    return None;
                }
            }
        }
        let tool_calls = calls.clone();
        self.messages.push(Message {
            role: Role::Assistant,
            text,
            tool_calls: calls,
            ..Message::default()
        });
        Some(tool_calls)
    }
}

/// A short human-facing summary of a tool call for the UI chips.
fn summarize_call(call: &ToolCall) -> String {
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
