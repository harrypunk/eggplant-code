//! Session persistence: one JSONL file per workspace under
//! `$EGGPLANT_HOME/agent/sessions/` (default `~/.eggplant`). Append-only
//! (pi's session format); messages are written as they complete, so the
//! file survives crashes and editor restarts.

use std::io;
use std::path::{Path, PathBuf};

use crate::types::Message;

pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    /// The store for a workspace root; `None` when there is no home.
    pub fn for_workspace(root: &Path) -> Option<Self> {
        Some(Self::at(
            data_root()?.join("sessions").join(file_name(root)),
        ))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one completed message (one JSON line).
    pub fn append(&self, message: &Message) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(message)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        file.write_all(line.as_bytes())
    }

    /// Load the transcript; corrupt lines are skipped, never fatal.
    pub fn load(&self) -> Vec<Message> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Start a new session: drop the file.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The data root: `$EGGPLANT_HOME` else `~/.eggplant`.
pub fn data_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("EGGPLANT_HOME") {
        return Some(PathBuf::from(dir));
    }
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".eggplant"))
}

/// `<slug>-<fnv64hex>.jsonl` — readable AND collision-safe enough.
fn file_name(root: &Path) -> String {
    let slug = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into());
    format!("{slug}-{:016x}.jsonl", fnv1a64(&root.to_string_lossy()))
}

/// FNV-1a 64 — stable across runs and versions (unlike DefaultHasher).
fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_load_roundtrips() {
        let dir = std::env::temp_dir().join(format!("eggplant-store-{}", std::process::id()));
        let store = SessionStore::at(dir.join("s.jsonl"));
        store.append(&Message::user("hello")).unwrap();
        store
            .append(&Message::tool_result("1", "done".into(), false))
            .unwrap();
        let messages = store.load();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "hello");
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("1"));
        store.clear();
        assert!(store.load().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = std::env::temp_dir().join(format!("eggplant-store2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let good = serde_json::to_string(&Message::user("ok")).unwrap();
        std::fs::write(&path, format!("{good}\nnot json\n{{}}\n")).unwrap();
        let messages = SessionStore::at(path).load();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "ok");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_name_is_stable_and_slugged() {
        let a = file_name(Path::new("/home/x/proj"));
        assert!(a.starts_with("proj-"), "{a}");
        assert_eq!(a, file_name(Path::new("/home/x/proj")), "deterministic");
        assert_ne!(a, file_name(Path::new("/home/y/proj")), "path-sensitive");
    }
}
