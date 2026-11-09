//! Errors the assistant reports to the chat panel.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum AssistantError {
    /// No key in the Keychain and no `ANTHROPIC_API_KEY` override.
    NoApiKey,
    /// The stored key has characters an Anthropic key never has.
    InvalidApiKey,
    /// The macOS `security` tool failed or is missing.
    Keychain(String),
    /// `curl` is not installed or could not be started.
    CurlMissing,
    /// The request never got an HTTP answer (offline, DNS, timeout).
    Network(String),
    /// The API answered with an error status.
    Http {
        status: u16,
        kind: String,
        message: String,
    },
    /// The API answered with something that is not the JSON we expect.
    BadJson(String),
    /// The model (or a safety classifier) declined the request.
    Refused { category: Option<String> },
    /// The user message was empty.
    EmptyMessage,
}

impl fmt::Display for AssistantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssistantError::NoApiKey => write!(
                f,
                "No Anthropic API key. Add one in Settings → AI (it is stored in the macOS Keychain)."
            ),
            AssistantError::InvalidApiKey => {
                write!(
                    f,
                    "The stored API key looks malformed. Paste it again in Settings → AI."
                )
            }
            AssistantError::Keychain(msg) => write!(f, "Keychain error: {msg}"),
            AssistantError::CurlMissing => {
                write!(
                    f,
                    "`curl` was not found; the assistant needs it to reach the API."
                )
            }
            AssistantError::Network(msg) => write!(f, "Could not reach the Anthropic API: {msg}"),
            AssistantError::Http {
                status,
                kind,
                message,
            } => {
                let hint = match status {
                    401 => " Check the API key in Settings → AI.",
                    429 => " Too many requests; wait a moment and try again.",
                    500 | 529 => " The API is busy; try again shortly.",
                    _ => "",
                };
                let message = message.trim_end_matches('.');
                write!(f, "API error {status} ({kind}): {message}.{hint}")
            }
            AssistantError::BadJson(msg) => write!(f, "Unexpected answer from the API: {msg}"),
            AssistantError::Refused {
                category: Some(category),
            } => {
                write!(f, "The model declined this request (category: {category}).")
            }
            AssistantError::Refused { category: None } => {
                write!(f, "The model declined this request.")
            }
            AssistantError::EmptyMessage => write!(f, "Type a question first."),
        }
    }
}

impl std::error::Error for AssistantError {}

impl AssistantError {
    /// True when trying the same request again later may succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            AssistantError::Network(_) => true,
            AssistantError::Http { status, .. } => matches!(status, 408 | 409 | 429 | 500..=599),
            _ => false,
        }
    }
}
