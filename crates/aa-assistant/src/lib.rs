//! aa-assistant: the backend of the AI chat panel.
//!
//! Explains terms, suggests chords, melodies and rhythms, and writes patterns from a text
//! description. It only *proposes* edits as [`Action`]s; the app applies them through its
//! undoable commands. Requests go through the system `curl` ([`CurlTransport`]) to the
//! chosen [`Provider`]: the Claude Messages API, or the OpenAI-compatible Chat Completions
//! API of OpenAI and Qwen. API keys live in the macOS Keychain ([`store_api_key_for`]).
//!
//! Everything blocks: call [`Assistant::ask`] from a worker thread.

mod actions;
mod assistant;
mod error;
mod keychain;
mod openai;
mod prompt;
mod provider;
mod summary;
mod transport;

pub use actions::{
    Action, ActionValidator, NoteSpec, parse_key, parse_role, parse_scale, parse_target,
    tool_definitions,
};
pub use assistant::{
    Assistant, DEFAULT_EFFORT, DEFAULT_MAX_HISTORY_TURNS, DEFAULT_MAX_TOKENS, DEFAULT_MODEL,
    MAX_ROUNDS, Reply, Turn,
};
pub use error::AssistantError;
pub use keychain::{
    API_KEY_ENV, KEYCHAIN_SERVICE, api_key, api_key_for, has_api_key, has_api_key_for,
    store_api_key, store_api_key_for,
};
pub use prompt::SYSTEM_PROMPT;
pub use provider::Provider;
pub use summary::{clip_notes_text, project_summary};
pub use transport::{
    ANTHROPIC_VERSION, ApiRequest, CurlTransport, MESSAGES_URL, Transport, required_betas,
};

#[cfg(test)]
mod tests;
