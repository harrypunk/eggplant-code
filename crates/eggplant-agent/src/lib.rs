//! eggplant-agent — the headless AI agent (M10).
//!
//! Design: `docs/design/agent.md`. One session, an event stream out, an
//! `AgentHost` bridge into the editor. Never imports ratatui/crossterm;
//! owns its own tokio runtime on a dedicated thread.

pub mod agent;
pub mod host;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod store;
pub mod tool;
pub mod tools;
pub mod types;

pub use host::{HostCall, HostClient, HostReply, HostRequest, TextEdit};
pub use provider::{Provider, ProviderConfig, preset, preset_names, provider_for};
pub use session::{AgentCommand, AgentEvent, AgentSession, SessionMsg};
pub use tool::Tool;
pub use types::{ChatEvent, ChatRequest, Message, Role, ToolCall, ToolDecl};
