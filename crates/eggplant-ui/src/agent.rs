//! The agent slice: session handle + UI transcript, the host-call
//! serving side, and prompt/abort plumbing. The runtime lives in
//! `eggplant-agent` (headless); this module is where its messages meet
//! the editor (docs/design/agent.md).

use std::path::{Path, PathBuf};

use eggplant_agent::{AgentEvent, AgentSession, HostCall, HostReply, ProviderConfig};
use eggplant_core::grep;

use crate::app::App;

/// `[agent]` from config.toml.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentSettings {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
}

impl AgentSettings {
    /// Resolve into a provider config; `Err` is user-facing guidance.
    /// Every preset speaks OpenAI-compatible completions (one adapter);
    /// `custom` = any compatible endpoint with explicit base_url.
    pub fn resolve(&self) -> Result<ProviderConfig, String> {
        let name = self.provider.clone().ok_or_else(|| {
            format!(
                "agent: set [agent] provider ({}, or \"custom\")",
                eggplant_agent::preset_names()
            )
        })?;
        let (base_url, api_key_env, default_model) = match eggplant_agent::preset(&name) {
            Some(p) => (
                p.base_url.to_string(),
                p.api_key_env.to_string(),
                p.default_model.to_string(),
            ),
            None if name == "custom" => {
                let base_url = self
                    .base_url
                    .clone()
                    .ok_or("agent: provider \"custom\" needs [agent] base_url")?;
                let api_key_env = self
                    .api_key_env
                    .clone()
                    .ok_or("agent: provider \"custom\" needs [agent] api_key_env")?;
                let model = self
                    .model
                    .clone()
                    .ok_or("agent: provider \"custom\" needs [agent] model")?;
                (base_url, api_key_env, model)
            }
            None => {
                return Err(format!(
                    "agent: unknown provider '{name}' ({}, or \"custom\")",
                    eggplant_agent::preset_names()
                ));
            }
        };
        // Field-level overrides win over preset conventions.
        let base_url = self.base_url.clone().unwrap_or(base_url);
        let api_key_env = self.api_key_env.clone().unwrap_or(api_key_env);
        let model = self.model.clone().unwrap_or(default_model);
        let api_key = std::env::var(&api_key_env)
            .map_err(|_| format!("agent: set ${api_key_env} (or [agent] api_key_env)"))?;
        Ok(ProviderConfig {
            name,
            model,
            api_key,
            base_url,
        })
    }
}

/// One block in the UI transcript (render-facing; the model-facing
/// transcript lives in the runtime).
#[derive(Clone, Debug)]
pub enum ChatItem {
    User(String),
    /// Streamed assistant text (deltas append while streaming).
    Assistant(String),
    /// `is_error`: None = running, Some(false) = ok, Some(true) = failed.
    Tool {
        id: String,
        summary: String,
        is_error: Option<bool>,
    },
}

/// The agent slice of `App` (like `theme`, `input`).
pub struct AgentState {
    pub settings: AgentSettings,
    session: Option<AgentSession>,
    pub transcript: Vec<ChatItem>,
    pub running: bool,
}

impl AgentState {
    pub fn new(settings: AgentSettings) -> Self {
        Self {
            settings,
            session: None,
            transcript: Vec::new(),
            running: false,
        }
    }

    /// Lazily spawn the session (first prompt).
    pub fn ensure_session(&mut self, cwd: PathBuf) -> Result<&AgentSession, String> {
        if self.session.is_none() {
            let config = self.settings.resolve()?;
            self.session = Some(AgentSession::spawn(config, cwd));
        }
        Ok(self.session.as_ref().expect("just spawned"))
    }

    pub fn session(&self) -> Option<&AgentSession> {
        self.session.as_ref()
    }

    /// Apply one runtime event to the transcript (dispatch-side).
    pub fn apply(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::RunStarted => self.running = true,
            AgentEvent::TextDelta(delta) => {
                // Append to the tail assistant block; a tool call since the
                // last text starts a new block.
                match self.transcript.last_mut() {
                    Some(ChatItem::Assistant(text)) => text.push_str(delta),
                    _ => self.transcript.push(ChatItem::Assistant(delta.clone())),
                }
            }
            AgentEvent::ToolStarted { id, summary, .. } => {
                self.transcript.push(ChatItem::Tool {
                    id: id.clone(),
                    summary: summary.clone(),
                    is_error: None,
                });
            }
            AgentEvent::ToolFinished { id, is_error, .. } => {
                if let Some(ChatItem::Tool { is_error: flag, .. }) = self
                    .transcript
                    .iter_mut()
                    .rev()
                    .find(|item| matches!(item, ChatItem::Tool { id: tid, .. } if tid == id))
                {
                    *flag = Some(*is_error);
                }
            }
            AgentEvent::Error(message) => {
                self.transcript
                    .push(ChatItem::Assistant(format!("⚠ {message}")));
            }
            AgentEvent::Aborted => {
                self.transcript
                    .push(ChatItem::Assistant("— aborted".into()));
            }
            AgentEvent::RunFinished { .. } => self.running = false,
            AgentEvent::Restored { messages } => {
                self.transcript = rebuild_transcript(messages);
            }
            AgentEvent::Cleared => self.transcript.clear(),
        }
    }
}

/// Project the persisted model-facing transcript back into UI items.
fn rebuild_transcript(messages: &[eggplant_agent::Message]) -> Vec<ChatItem> {
    // Tool results keyed by call id (for the chip's ✓/✗).
    let results: std::collections::HashMap<&str, bool> = messages
        .iter()
        .filter_map(|m| m.tool_call_id.as_deref().map(|id| (id, m.is_error)))
        .collect();
    messages
        .iter()
        .flat_map(|message| {
            use eggplant_agent::Role;
            match message.role {
                Role::User => vec![ChatItem::User(message.text.clone())],
                Role::Assistant => {
                    let mut items = Vec::new();
                    if !message.text.is_empty() {
                        items.push(ChatItem::Assistant(message.text.clone()));
                    }
                    for call in &message.tool_calls {
                        items.push(ChatItem::Tool {
                            id: call.id.clone(),
                            summary: eggplant_agent::agent::summarize_call(call),
                            is_error: results.get(call.id.as_str()).copied().or(Some(false)),
                        });
                    }
                    items
                }
                Role::Tool => vec![], // folded into the chip's status
            }
        })
        .collect()
}

/// Serve one host call against the live editor + workspace. Runs on the
/// UI thread during the agent drain — mutation is single-threaded.
pub fn serve_host(app: &mut App, call: &HostCall) -> Result<HostReply, String> {
    match call {
        HostCall::Read {
            path,
            offset,
            limit,
        } => serve_read(app, path, *offset, *limit),
        HostCall::Write { path, content } => {
            let path = resolve(app, path);
            open_in_editor(app, &path)?;
            app.editor.replace_text(content.clone()).map_err(err)?;
            app.editor.save().map_err(err)?;
            Ok(HostReply::Ok)
        }
        HostCall::Edit { path, edits } => {
            let path = resolve(app, path);
            open_in_editor(app, &path)?;
            let new_text = eggplant_agent::tools::edit::apply_edits(&app.editor.text(), edits)?;
            app.editor.replace_text(new_text).map_err(err)?;
            app.editor.save().map_err(err)?;
            Ok(HostReply::Text(format!(
                "applied {} edit(s) to {}",
                edits.len(),
                path.display()
            )))
        }
        HostCall::Grep { pattern } => {
            let hits = grep::search_workspace(&app.workspace, pattern);
            Ok(HostReply::Hits(
                hits.iter()
                    .map(|h| (PathBuf::from(&h.rel), h.line + 1, h.text.clone()))
                    .collect(),
            ))
        }
        HostCall::Find { query } => {
            const FILE_CAP: usize = 20_000;
            let files = app.workspace.collect_files(FILE_CAP);
            let ranked = eggplant_core::fuzzy::filter(&query.to_lowercase(), &files, |f| &f.rel);
            Ok(HostReply::Paths(
                ranked
                    .into_iter()
                    .take(50)
                    .map(|(_, f)| f.abs.clone())
                    .collect(),
            ))
        }
    }
}

/// Read with line paging: the open buffer's text if the file is open
/// (the live truth), else disk. Caps: 2000 lines / 100 KB per call.
fn serve_read(
    app: &App,
    path: &Path,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<HostReply, String> {
    const MAX_LINES: usize = 2000;
    const MAX_BYTES: usize = 100 * 1024;
    let path = resolve(app, path);
    // The current buffer's live text wins when it IS this path (the user
    // may have unsaved edits); everything else reads from disk. (The
    // facade exposes text() for the current buffer only — v1 scope.)
    let text = if app.editor.current_path().as_deref() == Some(path.as_path()) {
        app.editor.text()
    } else {
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = offset.unwrap_or(1).saturating_sub(1);
    let slice: Vec<&str> = lines
        .iter()
        .skip(start)
        .take(limit.unwrap_or(MAX_LINES).min(MAX_LINES))
        .copied()
        .collect();
    let mut body = slice.join("\n");
    if body.len() > MAX_BYTES {
        body.truncate(MAX_BYTES);
        body.push_str("\n… [truncated at 100 KB]");
    }
    if start + slice.len() < lines.len() {
        body.push_str(&format!(
            "\n… [{} more lines; use offset/limit]",
            lines.len() - start - slice.len()
        ));
    }
    Ok(HostReply::Text(body))
}

/// Resolve a possibly-relative path against the workspace root.
fn resolve(app: &App, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        app.workspace.root.join(path)
    }
}

/// Bring a path into the editor (agent edits are visible: the buffer
/// opens and becomes current — the user sees what changed).
fn open_in_editor(app: &mut App, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    if !path.exists() {
        std::fs::write(path, "").map_err(err)?;
    }
    app.editor.open_buffer(path).map_err(err)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggplant_agent::{Message, ToolCall};

    #[test]
    fn restored_transcript_rebuilds_ui_items() {
        let messages = vec![
            Message::user("fix the test"),
            Message {
                role: eggplant_agent::Role::Assistant,
                text: "on it".into(),
                tool_calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "edit".into(),
                    args: serde_json::json!({"path": "a.rs", "edits": []}),
                }],
                ..Message::default()
            },
            Message::tool_result("t1", "applied 1 edit".into(), false),
            Message::user("thanks"),
        ];
        let items = rebuild_transcript(&messages);
        assert!(matches!(&items[0], ChatItem::User(t) if t == "fix the test"));
        assert!(matches!(&items[1], ChatItem::Assistant(t) if t == "on it"));
        assert!(
            matches!(&items[2], ChatItem::Tool { summary, is_error: Some(false), .. } if summary == "edit a.rs")
        );
        assert!(matches!(&items[3], ChatItem::User(t) if t == "thanks"));
        assert_eq!(items.len(), 4, "tool results fold into chips, not items");
    }
}
