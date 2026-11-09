//! Which model service answers the chat.

use crate::assistant::DEFAULT_MODEL;
use crate::transport::MESSAGES_URL;
use serde::{Deserialize, Serialize};

/// Model ids change often: [`Provider::default_model`] is only a starting value for
/// `Assistant::model`, and `Assistant::base_url` overrides [`Provider::default_base_url`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Provider {
    /// Claude through the Anthropic Messages API.
    #[default]
    #[serde(rename = "anthropic")]
    Anthropic,
    /// OpenAI ("Codex" models) through Chat Completions.
    #[serde(rename = "openai")]
    OpenAi,
    /// Qwen through Alibaba DashScope's OpenAI-compatible Chat Completions.
    #[serde(rename = "qwen")]
    Qwen,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Anthropic, Provider::OpenAi, Provider::Qwen];

    /// Label for the UI.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Claude",
            Provider::OpenAi => "Codex (OpenAI)",
            Provider::Qwen => "Qwen",
        }
    }

    /// Stable string for settings files; also the serde form.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::Qwen => "qwen",
        }
    }

    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Anthropic => DEFAULT_MODEL,
            Provider::OpenAi => "gpt-5-codex",
            Provider::Qwen => "qwen3-coder-plus",
        }
    }

    /// For Anthropic this is the full Messages URL; for the others the API root, to which
    /// `/chat/completions` is appended.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Provider::Anthropic => MESSAGES_URL,
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Qwen => "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        }
    }

    /// Placeholder for the key field in Settings.
    pub fn key_hint(self) -> &'static str {
        match self {
            Provider::Anthropic => "sk-ant-…",
            Provider::OpenAi | Provider::Qwen => "sk-…",
        }
    }

    /// Environment variable that overrides the Keychain (development).
    pub fn key_env(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::Qwen => "DASHSCOPE_API_KEY",
        }
    }

    /// Keychain service name of this provider's stored key.
    pub fn keychain_service(self) -> &'static str {
        match self {
            Provider::Anthropic => "adaptaudio-anthropic",
            Provider::OpenAi => "adaptaudio-openai",
            Provider::Qwen => "adaptaudio-qwen",
        }
    }

    /// True for the providers that speak OpenAI-compatible Chat Completions.
    pub fn is_openai_compatible(self) -> bool {
        !matches!(self, Provider::Anthropic)
    }

    /// The URL a request is posted to, given a base URL in the form of
    /// [`Provider::default_base_url`].
    pub fn endpoint(self, base_url: &str) -> String {
        let base = base_url.trim().trim_end_matches('/');
        if self.is_openai_compatible() && !base.ends_with("/chat/completions") {
            format!("{base}/chat/completions")
        } else {
            base.to_string()
        }
    }
}
