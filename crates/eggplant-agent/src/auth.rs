//! API-key auth: stored at `~/.eggplant/agent/auth.toml` (hand-editable),
//! validated against the provider before saving.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store;

/// The stored auth: `[keys] qwen = "sk-…"`, `[urls] qwen = "https://…"`.
#[derive(Default, Serialize, Deserialize)]
struct AuthFile {
    #[serde(default)]
    keys: BTreeMap<String, String>,
    /// Endpoint overrides (absent = the preset's default base_url).
    #[serde(default)]
    urls: BTreeMap<String, String>,
    /// The chosen default model per provider (`Space a m`, or the model
    /// discovered at login).
    #[serde(default)]
    models: BTreeMap<String, String>,
}

pub struct AuthStore {
    path: PathBuf,
    keys: BTreeMap<String, String>,
    urls: BTreeMap<String, String>,
    models: BTreeMap<String, String>,
}

/// Where a working key came from (displayed in the auth view).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Env,
    File,
}

impl AuthStore {
    /// The default location: `$EGGPLANT_HOME/agent/auth.toml`.
    pub fn load_default() -> Option<Self> {
        Some(Self::load(store::agent_dir()?.join("auth.toml")))
    }

    pub fn load(path: PathBuf) -> Self {
        let file = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str::<AuthFile>(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            keys: file.keys,
            urls: file.urls,
            models: file.models,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, provider: &str) -> Option<&str> {
        self.keys.get(provider).map(String::as_str)
    }

    /// The stored endpoint override, if any.
    pub fn url_for(&self, provider: &str) -> Option<&str> {
        self.urls.get(provider).map(String::as_str)
    }

    /// The chosen default model, if any.
    pub fn model_for(&self, provider: &str) -> Option<&str> {
        self.models.get(provider).map(String::as_str)
    }

    /// A snapshot of all stored default models (provider → model).
    pub fn default_models(&self) -> BTreeMap<String, String> {
        self.models.clone()
    }

    /// Set + persist (one call; the file is the truth). `url` is stored
    /// only when it overrides something (None keeps any prior override).
    pub fn set(&mut self, provider: &str, key: &str, url: Option<&str>) -> io::Result<()> {
        self.keys.insert(provider.to_string(), key.to_string());
        if let Some(url) = url {
            self.urls.insert(provider.to_string(), url.to_string());
        }
        self.save()
    }

    /// Set the default model + persist.
    pub fn set_model(&mut self, provider: &str, model: &str) -> io::Result<()> {
        self.models.insert(provider.to_string(), model.to_string());
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&AuthFile {
            keys: self.keys.clone(),
            urls: self.urls.clone(),
            models: self.models.clone(),
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(&self.path, text)
    }

    /// Env var wins; the file is the fallback. Returns key + source.
    pub fn resolve_key(&self, env_var: &str, provider: &str) -> Option<(String, KeySource)> {
        if let Ok(key) = std::env::var(env_var)
            && !key.is_empty()
        {
            return Some((key, KeySource::Env));
        }
        self.get(provider).map(|k| (k.to_string(), KeySource::File))
    }
}

/// List the models a key can see: `GET {base_url}/models` (blocking —
/// call from a background thread).
pub fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(format!("{base_url}/models"))
        .bearer_auth(api_key)
        .send()
        .map_err(|e| format!("cannot reach {base_url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("GET /models → HTTP {}", response.status()));
    }
    let body: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    Ok(parse_model_ids(&body))
}

/// The model ids out of an OpenAI-style `/models` body (sorted, deduped).
fn parse_model_ids(body: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .map(|data| {
            data.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

/// Mask a key for display: `sk-…wxyz`.
pub fn mask(key: &str) -> String {
    let head: String = key.chars().take(3).collect();
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Connectivity check: GET {base_url}/models with the key. Ok = the
/// provider accepted the key (2xx); Err carries a readable reason.
pub fn validate_key(base_url: &str, api_key: &str) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(format!("{base_url}/models"))
        .bearer_auth(api_key)
        .send()
        .map_err(|e| format!("cannot reach {base_url}: {e}"))?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err("key rejected (401/403) — check it".to_string()),
        status => Err(format!("unexpected status {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_persists_and_loads_back() {
        let dir = std::env::temp_dir().join(format!("eggplant-auth-{}", std::process::id()));
        let path = dir.join("auth.toml");
        let mut store = AuthStore::load(path.clone());
        store
            .set("qwen", "sk-test123", Some("https://proxy.example/v1"))
            .unwrap();
        store.set_model("qwen", "qwen3-coder-plus").unwrap();
        let reloaded = AuthStore::load(path);
        assert_eq!(reloaded.get("qwen"), Some("sk-test123"));
        assert_eq!(reloaded.url_for("qwen"), Some("https://proxy.example/v1"));
        assert_eq!(reloaded.model_for("qwen"), Some("qwen3-coder-plus"));
        assert!(reloaded.get("kimi").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_wins_over_file() {
        let dir = std::env::temp_dir().join(format!("eggplant-auth2-{}", std::process::id()));
        let path = dir.join("auth.toml");
        let mut store = AuthStore::load(path);
        store.set("qwen", "from-file", None).unwrap();
        // SAFETY: test process, single-threaded access to this var.
        unsafe { std::env::set_var("EGGPLANT_TEST_AUTHKEY", "from-env") };
        let (key, source) = store.resolve_key("EGGPLANT_TEST_AUTHKEY", "qwen").unwrap();
        assert_eq!((key.as_str(), source), ("from-env", KeySource::Env));
        unsafe { std::env::remove_var("EGGPLANT_TEST_AUTHKEY") };
        let (key, source) = store.resolve_key("EGGPLANT_TEST_AUTHKEY", "qwen").unwrap();
        assert_eq!((key.as_str(), source), ("from-file", KeySource::File));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mask_shows_edges_only() {
        assert_eq!(mask("sk-abcdefghijk"), "sk-…hijk");
    }
}
