//! `AgentSession` — the UI-facing handle. Owns the runtime thread
//! (tokio), the command channel in, and the single message channel out
//! (events + host calls; the UI drains both each tick).

use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;

use tokio::sync::mpsc;

use crate::agent::AgentRuntime;
use crate::host::HostRequest;
use crate::provider::{ProviderConfig, provider_for};
use crate::tool::default_tools;

/// What the UI can tell a running session.
#[derive(Clone, Debug)]
pub enum AgentCommand {
    Prompt(String),
    Abort,
}

/// Everything the runtime tells the UI.
#[derive(Clone, Debug)]
pub enum AgentEvent {
    RunStarted,
    /// A streamed text fragment of the assistant's reply.
    TextDelta(String),
    ToolStarted {
        id: String,
        name: String,
        /// One-line human summary, e.g. "edit src/main.rs".
        summary: String,
    },
    ToolFinished {
        id: String,
        name: String,
        is_error: bool,
    },
    /// The model signaled failure (transport or provider error).
    Error(String),
    Aborted,
    RunFinished {
        aborted: bool,
    },
}

/// One item on the UI drain channel: an agent event, or a host call the
/// UI must serve (apply via the editor, then `respond`).
pub enum SessionMsg {
    Event(AgentEvent),
    Host(HostRequest),
}

pub struct AgentSession {
    commands: mpsc::UnboundedSender<AgentCommand>,
    incoming: std_mpsc::Receiver<SessionMsg>,
}

impl AgentSession {
    /// Spawn the runtime thread. `cwd` scopes the system prompt; the
    /// host channel is created here and returned inside the session —
    /// the UI serves `SessionMsg::Host` requests from `try_recv`.
    pub fn spawn(config: ProviderConfig, cwd: PathBuf) -> Self {
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (host_tx, mut host_rx) = mpsc::unbounded_channel::<HostRequest>();
        let (msg_tx, msg_rx) = std_mpsc::channel::<SessionMsg>();

        // The event forwarder: runtime events → the UI drain channel.
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let forward_tx = msg_tx.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            runtime.block_on(async move {
                loop {
                    tokio::select! {
                        event = event_rx.recv() => {
                            match event {
                                Some(e) => {
                                    if forward_tx.send(SessionMsg::Event(e)).is_err() {
                                        return;
                                    }
                                }
                                None => return,
                            }
                        }
                        host = host_rx.recv() => {
                            match host {
                                Some(h) => {
                                    if forward_tx.send(SessionMsg::Host(h)).is_err() {
                                        return;
                                    }
                                }
                                None => return,
                            }
                        }
                    }
                }
            });
        });

        let tools = default_tools();
        let system_prompt = {
            let agents_md = std::fs::read_to_string(cwd.join("AGENTS.md")).ok();
            crate::prompt::build_system_prompt(&tools, &cwd, agents_md.as_deref())
        };
        let runtime = AgentRuntime::new(
            provider_for(config),
            tools,
            system_prompt,
            crate::host::HostClient::new(host_tx),
            event_tx,
            cwd,
        );
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(runtime.run(command_rx));
        });

        Self {
            commands,
            incoming: msg_rx,
        }
    }

    pub fn prompt(&self, text: impl Into<String>) {
        let _ = self.commands.send(AgentCommand::Prompt(text.into()));
    }

    pub fn abort(&self) {
        let _ = self.commands.send(AgentCommand::Abort);
    }

    /// Non-blocking drain for the UI event loop.
    pub fn try_recv(&self) -> Option<SessionMsg> {
        self.incoming.try_recv().ok()
    }
}
