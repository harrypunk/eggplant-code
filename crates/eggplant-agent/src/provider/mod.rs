//! Provider adapters: one per API flavor, each normalizing its SSE wire
//! format to the `ChatEvent` stream. V1 ships Anthropic Messages and
//! OpenAI-compatible chat completions (covers OpenAI, OpenRouter,
//! DeepSeek, llama.cpp, Ollama, LM Studio…).

pub mod anthropic;
pub mod openai;
pub mod sse;

use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::types::{ChatEvent, ChatRequest};

/// Which API flavor to speak.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    Anthropic,
    OpenAiCompatible,
}

/// Provider configuration (from `[agent]` in config.toml).
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub model: String,
    /// The API key value (resolved from the env var at startup).
    pub api_key: String,
    pub base_url: String,
}

impl ProviderConfig {
    pub fn default_base_url(kind: &ProviderKind) -> &'static str {
        match kind {
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::OpenAiCompatible => "https://api.openai.com/v1",
        }
    }
}

/// A provider streams normalized chat events for one request.
/// Object-safe via a boxed stream.
pub trait Provider: Send + Sync {
    fn stream(
        &self,
        request: &ChatRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = ChatEvent> + Send + '_>>;
}

/// Build the concrete adapter for a config.
pub fn provider_for(config: ProviderConfig) -> Box<dyn Provider> {
    match config.kind {
        ProviderKind::Anthropic => Box::new(anthropic::Anthropic::new(config)),
        ProviderKind::OpenAiCompatible => Box::new(openai::OpenAi::new(config)),
    }
}
