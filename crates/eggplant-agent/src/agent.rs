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

/// How one provider stream ended.
enum End {
    /// The model finished the turn (stop or tool-use boundary).
    Finished,
    /// The user aborted mid-stream.
    Aborted,
    /// Transport/provider failure (error already emitted).
    Failed,
}

/// The outcome of one streamed turn.
enum Turn {
    /// The model asked for tools — execute and stream again.
    Calls(Vec<ToolCall>),
    /// Text-only reply: the run is done.
    Done,
    /// Aborted mid-stream; the (partial) calls need closing.
    Aborted(Vec<ToolCall>),
    /// Failure already reported to the UI.
    Failed,
}

/// How a run ended (drives `RunFinished`).
#[derive(PartialEq, Eq)]
enum RunOutcome {
    Finished,
    Aborted,
}

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
                AgentCommand::Prompt(text) => {
                    self.run_prompt(text, &mut commands).await;
                    // Prompts typed mid-run queue up; each gets its own run.
                    while let Some(next) = self.queued.pop_front() {
                        self.run_prompt(next, &mut commands).await;
                    }
                }
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

    /// Run one prompt: the tool-use loop until the model answers with
    /// text, the user aborts, or the turn budget ends.
    async fn run_prompt(
        &mut self,
        text: String,
        commands: &mut mpsc::UnboundedReceiver<AgentCommand>,
    ) {
        self.emit(AgentEvent::RunStarted).await;
        self.record(Message::user(text));
        let outcome = self.run_turns(commands).await;
        self.emit(AgentEvent::RunFinished {
            aborted: outcome == RunOutcome::Aborted,
        })
        .await;
    }

    /// The tool-use loop: stream a turn → execute its calls → stream
    /// again with the results, until the model stops calling tools.
    async fn run_turns(
        &mut self,
        commands: &mut mpsc::UnboundedReceiver<AgentCommand>,
    ) -> RunOutcome {
        for _ in 0..MAX_TURNS {
            match self.stream_turn(commands).await {
                Turn::Calls(calls) => self.execute_calls(calls).await,
                // Done: clean finish. Failed: error already emitted.
                Turn::Done | Turn::Failed => return RunOutcome::Finished,
                Turn::Aborted(calls) => {
                    self.close_pending_calls(calls, "aborted by user").await;
                    return RunOutcome::Aborted;
                }
            }
        }
        self.emit(AgentEvent::Error(format!(
            "turn budget exhausted ({MAX_TURNS} turns)"
        )))
        .await;
        RunOutcome::Finished
    }

    /// Execute one turn's calls sequentially, recording each result.
    async fn execute_calls(&mut self, calls: Vec<ToolCall>) {
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
            self.record(Message::tool_result(
                call.id,
                output.content,
                output.is_error,
            ));
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
    async fn stream_turn(&mut self, commands: &mut mpsc::UnboundedReceiver<AgentCommand>) -> Turn {
        let request = ChatRequest {
            system: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: self.tools.iter().map(|t| t.declaration()).collect(),
        };
        // The stream borrows `self.provider` for its whole scope — keep
        // that scope in a block so recording (a &mut self) can follow.
        let (text, calls, end) = {
            let stream = self.provider.stream(&request);
            futures::pin_mut!(stream);
            let mut text = String::new();
            let mut calls = Vec::new();
            // None = keep streaming; Some(end) = leave the loop with an
            // outcome. Abort breaks immediately: dropping the stream
            // cancels the HTTP request instead of draining it mutely.
            let end = loop {
                let end: Option<End> = tokio::select! {
                    event = stream.next() => match event {
                        Some(ChatEvent::TextDelta(delta)) => {
                            text.push_str(&delta);
                            self.emit(AgentEvent::TextDelta(delta)).await;
                            None
                        }
                        // Display-only: thinking is shown but never
                        // persisted or replayed to the model.
                        Some(ChatEvent::ThinkDelta(delta)) => {
                            self.emit(AgentEvent::ThinkDelta(delta)).await;
                            None
                        }
                        Some(ChatEvent::ToolCall(call)) => {
                            calls.push(call);
                            None
                        }
                        Some(ChatEvent::Done) | None => Some(End::Finished),
                        Some(ChatEvent::Error(message)) => {
                            self.emit(AgentEvent::Error(message)).await;
                            Some(End::Failed)
                        }
                    },
                    command = commands.recv() => match command {
                        Some(AgentCommand::Abort) => {
                            self.emit(AgentEvent::Aborted).await;
                            Some(End::Aborted)
                        }
                        Some(AgentCommand::Prompt(text)) => {
                            self.queued.push_back(text);
                            None
                        }
                        // Mid-run NewChat: queued prompts die with the old
                        // transcript; the clear happens when the run
                        // settles (abort first for an immediate reset).
                        Some(AgentCommand::NewChat) => {
                            self.queued.clear();
                            None
                        }
                        None => None,
                    },
                };
                if let Some(end) = end {
                    break end;
                }
            };
            (text, calls, end)
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
        match end {
            End::Finished if calls.is_empty() => Turn::Done,
            End::Finished => Turn::Calls(calls),
            End::Aborted => Turn::Aborted(calls),
            End::Failed => Turn::Failed,
        }
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
