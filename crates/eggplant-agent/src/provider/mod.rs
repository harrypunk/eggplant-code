//! Provider presets and the adapter. Every supported platform speaks the
//! OpenAI-compatible chat-completions API (pi's finding: each provider is
//! just `baseUrl` + an env-var API key over one wire format), so there is
//! exactly one adapter and a table of presets.

pub mod openai;
pub mod sse;

use futures::Stream;

use crate::types::{ChatEvent, ChatRequest};

/// A named platform preset (pi's provider data, ported). The model
/// default and the env var holding the API key are per-platform
/// conventions; both are overridable in `[agent]`.
pub struct ProviderPreset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub api_key_env: &'static str,
    pub default_model: &'static str,
}

pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "qwen",
        // Alibaba Cloud DashScope, OpenAI-compatible mode.
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        api_key_env: "DASHSCOPE_API_KEY",
        default_model: "qwen3-coder-plus",
    },
    ProviderPreset {
        name: "kimi",
        // Moonshot AI (pi's moonshotai provider).
        base_url: "https://api.moonshot.ai/v1",
        api_key_env: "MOONSHOT_API_KEY",
        default_model: "kimi-k2-0905-preview",
    },
    ProviderPreset {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        api_key_env: "OPENAI_API_KEY",
        default_model: "gpt-4o",
    },
];

/// Look up a preset by name.
pub fn preset(name: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// The comma-separated preset names (for error messages).
pub fn preset_names() -> String {
    PRESETS
        .iter()
        .map(|p| p.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Provider configuration, resolved from `[agent]` (ui side) or built
/// directly (tests, headless use).
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    /// The preset name (or "custom") — display only.
    pub name: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

/// A provider streams normalized chat events for one request.
/// Object-safe via a boxed stream.
pub trait Provider: Send + Sync {
    fn stream(
        &self,
        request: &ChatRequest,
    ) -> std::pin::Pin<Box<dyn Stream<Item = ChatEvent> + Send + '_>>;
}

/// The single adapter: OpenAI-compatible completions.
pub fn provider_for(config: ProviderConfig) -> Box<dyn Provider> {
    Box::new(openai::OpenAi::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_consistent_fields() {
        for preset in PRESETS {
            assert!(preset.base_url.starts_with("https://"), "{}", preset.name);
            assert!(preset.api_key_env.ends_with("API_KEY"), "{}", preset.name);
            assert!(!preset.default_model.is_empty(), "{}", preset.name);
        }
        assert_eq!(preset("qwen").unwrap().api_key_env, "DASHSCOPE_API_KEY");
        assert!(preset_names().contains("kimi"));
    }
}
