//! The bridge between agent tools and the editor.
//!
//! Tools run on the agent's tokio runtime; every host access crosses to
//! the UI thread as a [`HostCall`] and is applied there — the same rule
//! as keyboard edits (mutation at event time, on one thread). The tool
//! awaits the reply on a oneshot. See `docs/design/agent.md`.

use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

/// One exact-text replacement (pi's edit contract): `old_text` must
/// occur exactly once in the original; matched non-incrementally.
#[derive(Clone, Debug)]
pub struct TextEdit {
    pub old_text: String,
    pub new_text: String,
}

/// Everything a tool can ask of the editor. Served by the UI thread.
#[derive(Clone, Debug)]
pub enum HostCall {
    /// Read a file with line paging (1-indexed offset, limit lines).
    Read {
        path: PathBuf,
        offset: Option<usize>,
        limit: Option<usize>,
    },
    /// Create/overwrite a file. Open buffers are updated via a
    /// transaction so the change is visible and undoable.
    Write { path: PathBuf, content: String },
    /// Exact-text edits against the document, applied as one
    /// transaction and saved.
    Edit { path: PathBuf, edits: Vec<TextEdit> },
    /// Project content search (same engine and caps as live grep).
    Grep { pattern: String },
    /// Project file find (same walk as the file picker).
    Find { query: String },
}

/// The reply to a [`HostCall`]; `Err` carries a human/model-readable
/// message the tool forwards to the model as an error result.
#[derive(Clone, Debug)]
pub enum HostReply {
    Text(String),
    Paths(Vec<PathBuf>),
    /// grep hits: `(path, line_number, line_text)` — compact, model-ready.
    Hits(Vec<(PathBuf, usize, String)>),
    Ok,
}

pub struct HostRequest {
    pub call: HostCall,
    reply: oneshot::Sender<Result<HostReply, String>>,
}

impl HostRequest {
    /// The serving side answers the request.
    pub fn respond(self, result: Result<HostReply, String>) {
        let _ = self.reply.send(result);
    }
}

/// The tool-side handle: send a call, await the reply.
#[derive(Clone)]
pub struct HostClient {
    tx: mpsc::UnboundedSender<HostRequest>,
}

impl HostClient {
    pub fn new(tx: mpsc::UnboundedSender<HostRequest>) -> Self {
        Self { tx }
    }

    pub async fn call(&self, call: HostCall) -> Result<HostReply, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostRequest { call, reply })
            .map_err(|_| "editor is gone".to_string())?;
        rx.await
            .map_err(|_| "editor dropped the request".to_string())?
    }
}
