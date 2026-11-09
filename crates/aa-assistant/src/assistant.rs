//! The conversation and the tool-use loop.

use crate::actions::{Action, ActionValidator, tool_definitions};
use crate::error::AssistantError;
use crate::openai;
use crate::prompt::SYSTEM_PROMPT;
use crate::provider::Provider;
use crate::summary::project_summary;
use crate::transport::{ApiRequest, Transport};
use aa_core::Project;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The most capable current Opus model; the default for [`Provider::Anthropic`].
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// Room for adaptive thinking plus a reply with a few clips of notes, without streaming.
pub const DEFAULT_MAX_TOKENS: u32 = 16_000;
/// Set explicitly: the model's own default is `medium`, but say so in code so a model swap
/// does not silently change it.
pub const DEFAULT_EFFORT: &str = "medium";
/// Past user questions kept; when exceeded, the oldest half is dropped at once.
pub const DEFAULT_MAX_HISTORY_TURNS: usize = 16;
/// Model calls per question. The last one forbids tools so the model always ends with text.
pub const MAX_ROUNDS: usize = 6;

const TOOL_OK: &str = "Proposed to the user as an undoable edit; not applied yet. Continue, then summarize your proposals.";
const PREFIX_MISMATCH: &str = "bound to a different conversation";
const TRUNCATED_CALL: &str = "the call was cut off by max_tokens; send it again, shorter";
const NO_REPLY: &str = "(no reply)";

/// One user question and everything the API exchanged for it, stored verbatim (thinking
/// blocks included) because the API checks that replayed history is unchanged.
///
/// `messages` are in the wire format of `provider`. The chat survives a provider switch: a
/// turn made in the other wire format is replayed as plain text (`user_text`, then
/// `reply_text`) without its project snapshot, thinking and tool calls; turns in the current
/// format are replayed verbatim. OpenAI and Qwen share one format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub user_text: String,
    pub reply_text: String,
    /// The user message, then assistant / tool-result messages, as sent to `provider`.
    pub messages: Vec<Value>,
    /// Who answered. Turns saved before providers existed are Anthropic turns.
    #[serde(default)]
    pub provider: Provider,
}

/// What one question produced. The app shows `text`, offers `actions` as undoable edits and
/// may show `warnings` (dropped tool calls) in a muted style.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub text: String,
    pub actions: Vec<Action>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assistant {
    /// Who answers. Changing it mid-chat is fine (see [`Turn`]); set `model` to match, or use
    /// [`Assistant::set_provider`].
    #[serde(default)]
    pub provider: Provider,
    /// Model id for `provider`. Ids change often, so the app should let the user edit it.
    pub model: String,
    /// API address in the form of [`Provider::default_base_url`]; `None` = that default.
    #[serde(default)]
    pub base_url: Option<String>,
    pub max_tokens: u32,
    /// `low`, `medium`, `high`, `xhigh` or `max`. Anthropic only.
    pub effort: String,
    /// Ask the API to retry a safety-classifier refusal on a fallback model (beta).
    /// Anthropic only.
    pub refusal_fallback: bool,
    pub max_history_turns: usize,
    pub history: Vec<Turn>,
    /// Set after the API rejected the beta fields; later requests go without them.
    #[serde(default)]
    plain_requests: bool,
}

impl Default for Assistant {
    fn default() -> Self {
        Assistant {
            provider: Provider::Anthropic,
            model: DEFAULT_MODEL.to_string(),
            base_url: None,
            max_tokens: DEFAULT_MAX_TOKENS,
            effort: DEFAULT_EFFORT.to_string(),
            refusal_fallback: true,
            max_history_turns: DEFAULT_MAX_HISTORY_TURNS,
            history: Vec::new(),
            plain_requests: false,
        }
    }
}

impl Assistant {
    pub fn new() -> Self {
        Self::default()
    }

    /// An assistant for `provider` with that provider's default model and address.
    pub fn with_provider(provider: Provider) -> Self {
        let mut assistant = Self::default();
        assistant.set_provider(provider);
        assistant
    }

    /// Switches provider and resets `model` and `base_url` to the provider's defaults. The
    /// conversation is kept.
    pub fn set_provider(&mut self, provider: Provider) {
        self.provider = provider;
        self.model = provider.default_model().to_string();
        self.base_url = None;
    }

    /// Forgets the conversation.
    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Asks one question. Blocks until the model is done (call it from a worker thread).
    ///
    /// `selection_context` describes what the user has selected (e.g. a track and the notes of
    /// the open clip, see [`crate::clip_notes_text`]); it may be empty. On error the
    /// conversation is left as it was before the call.
    pub fn ask(
        &mut self,
        transport: &dyn Transport,
        project: &Project,
        selection_context: &str,
        user_text: &str,
    ) -> Result<Reply, AssistantError> {
        let user_text = user_text.trim();
        if user_text.is_empty() {
            return Err(AssistantError::EmptyMessage);
        }
        if self.provider.is_openai_compatible() {
            self.ask_openai(transport, project, selection_context, user_text)
        } else {
            self.ask_anthropic(transport, project, selection_context, user_text)
        }
    }

    fn ask_anthropic(
        &mut self,
        transport: &dyn Transport,
        project: &Project,
        selection_context: &str,
        user_text: &str,
    ) -> Result<Reply, AssistantError> {
        let mut turn = vec![user_message(project, selection_context, user_text)];
        let mut validator = ActionValidator::new(project);
        let mut texts: Vec<String> = Vec::new();
        let mut reply = Reply::default();

        for round in 0..MAX_ROUNDS {
            let allow_tools = round + 1 < MAX_ROUNDS;
            let response = self.send(transport, &mut turn, allow_tools)?;
            let stop_reason = response
                .get("stop_reason")
                .and_then(Value::as_str)
                .unwrap_or("");
            if stop_reason == "refusal" {
                let category = response
                    .pointer("/stop_details/category")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                return Err(AssistantError::Refused { category });
            }
            let content = echo_content(response.get("content").and_then(Value::as_array));
            texts.extend(
                content
                    .iter()
                    .filter(|b| block_type(b) == "text")
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string),
            );
            let tool_uses: Vec<(String, String, Value)> = content
                .iter()
                .filter(|b| block_type(b) == "tool_use")
                .map(|b| {
                    let field =
                        |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    (
                        field("id"),
                        field("name"),
                        b.get("input").cloned().unwrap_or(Value::Null),
                    )
                })
                .collect();
            turn.push(json!({"role": "assistant", "content": content_or_placeholder(content)}));

            if tool_uses.is_empty() {
                if stop_reason == "max_tokens" {
                    reply
                        .warnings
                        .push("The reply was cut off (max_tokens).".to_string());
                }
                break;
            }
            let truncated = stop_reason == "max_tokens";
            let mut results = Vec::with_capacity(tool_uses.len());
            for (id, name, input) in tool_uses {
                let outcome = if truncated {
                    Err(TRUNCATED_CALL.to_string())
                } else {
                    validator.check(&name, &input)
                };
                match outcome {
                    Ok(action) => {
                        reply.actions.push(action);
                        results.push(
                            json!({"type": "tool_result", "tool_use_id": id, "content": TOOL_OK}),
                        );
                    }
                    Err(reason) => {
                        reply.warnings.push(format!("Dropped {name}: {reason}"));
                        results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "is_error": true,
                            "content": format!("Rejected, nothing was proposed: {reason}")
                        }));
                    }
                }
            }
            turn.push(json!({"role": "user", "content": results}));
        }

        // Only reachable if the tool-free last round still returned tool calls: close the turn
        // with an assistant message so the next question keeps roles alternating.
        if turn
            .last()
            .and_then(|m| m.get("role"))
            .and_then(Value::as_str)
            == Some("user")
        {
            turn.push(
                json!({"role": "assistant", "content": [{"type": "text", "text": "(stopped)"}]}),
            );
        }

        reply.text = texts.join("\n\n");
        self.history.push(Turn {
            user_text: user_text.to_string(),
            reply_text: reply.text.clone(),
            messages: turn,
            provider: self.provider,
        });
        self.trim_history();
        Ok(reply)
    }

    /// The same loop over OpenAI-compatible Chat Completions: tool calls arrive in
    /// `message.tool_calls` and every result goes back as its own `role: "tool"` message.
    fn ask_openai(
        &mut self,
        transport: &dyn Transport,
        project: &Project,
        selection_context: &str,
        user_text: &str,
    ) -> Result<Reply, AssistantError> {
        let context = context_text(project, selection_context);
        let mut turn = vec![openai::user_message(&format!("{context}\n\n{user_text}"))];
        let mut validator = ActionValidator::new(project);
        let mut texts: Vec<String> = Vec::new();
        let mut reply = Reply::default();

        for round in 0..MAX_ROUNDS {
            let allow_tools = round + 1 < MAX_ROUNDS;
            let body = openai::request_body(
                self.provider,
                &self.model,
                self.max_tokens,
                SYSTEM_PROMPT,
                self.conversation(&turn),
                allow_tools,
            );
            let answer = openai::parse_answer(&self.post(transport, &body)?)?;
            if !answer.text.is_empty() {
                texts.push(answer.text);
            }
            turn.push(answer.message);

            if answer.tool_calls.is_empty() {
                if answer.truncated {
                    reply
                        .warnings
                        .push("The reply was cut off (max_tokens).".to_string());
                }
                break;
            }
            for call in answer.tool_calls {
                let outcome = if answer.truncated {
                    Err(TRUNCATED_CALL.to_string())
                } else {
                    call.input
                        .and_then(|input| validator.check(&call.name, &input))
                };
                match outcome {
                    Ok(action) => {
                        reply.actions.push(action);
                        turn.push(openai::tool_result(&call.id, TOOL_OK));
                    }
                    Err(reason) => {
                        reply
                            .warnings
                            .push(format!("Dropped {}: {reason}", call.name));
                        turn.push(openai::tool_result(
                            &call.id,
                            &format!("Rejected, nothing was proposed: {reason}"),
                        ));
                    }
                }
            }
        }

        // As in the Anthropic loop: a turn must end with an assistant message.
        if turn
            .last()
            .and_then(|m| m.get("role"))
            .and_then(Value::as_str)
            != Some("assistant")
        {
            turn.push(openai::assistant_message("(stopped)"));
        }

        reply.text = texts.join("\n\n");
        self.history.push(Turn {
            user_text: user_text.to_string(),
            reply_text: reply.text.clone(),
            messages: turn,
            provider: self.provider,
        });
        self.trim_history();
        Ok(reply)
    }

    fn post(&self, transport: &dyn Transport, body: &Value) -> Result<Value, AssistantError> {
        let url = self
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
            .map(|base| self.provider.endpoint(base));
        transport.send_request(&ApiRequest {
            provider: self.provider,
            url: url.as_deref(),
            body,
        })
    }

    /// History plus the turn in progress, in the current provider's wire format (see [`Turn`]
    /// for what happens to turns another provider answered).
    fn conversation(&self, turn: &[Value]) -> Vec<Value> {
        let openai = self.provider.is_openai_compatible();
        let mut messages = Vec::new();
        for past in &self.history {
            if past.provider.is_openai_compatible() == openai {
                messages.extend(past.messages.iter().cloned());
                continue;
            }
            let reply = if past.reply_text.trim().is_empty() {
                NO_REPLY
            } else {
                &past.reply_text
            };
            if openai {
                messages.push(openai::user_message(&past.user_text));
                messages.push(openai::assistant_message(reply));
            } else {
                messages.push(json!({
                    "role": "user",
                    "content": [{"type": "text", "text": past.user_text}]
                }));
                messages.push(json!({
                    "role": "assistant",
                    "content": [{"type": "text", "text": reply}]
                }));
            }
        }
        messages.extend(turn.iter().cloned());
        messages
    }

    /// One API call with the whole conversation, recovering once from two known 400s.
    fn send(
        &mut self,
        transport: &dyn Transport,
        turn: &mut [Value],
        allow_tools: bool,
    ) -> Result<Value, AssistantError> {
        let mut retried_plain = false;
        let mut retried_stripped = false;
        loop {
            let body = self.request_body(turn, allow_tools);
            match self.post(transport, &body) {
                Err(AssistantError::Http {
                    status: 400,
                    message,
                    ..
                }) if !retried_plain && !self.plain_requests && mentions_beta_field(&message) => {
                    // The account does not have the betas: go on without fallback and binding.
                    self.plain_requests = true;
                    retried_plain = true;
                }
                Err(AssistantError::Http {
                    status: 400,
                    message,
                    ..
                }) if !retried_stripped && message.contains(PREFIX_MISMATCH) => {
                    // History no longer matches its thinking signatures: drop the thinking.
                    for t in &mut self.history {
                        strip_thinking(&mut t.messages);
                    }
                    strip_thinking(turn);
                    retried_stripped = true;
                }
                other => return other,
            }
        }
    }

    fn request_body(&self, turn: &[Value], allow_tools: bool) -> Value {
        let messages = self.conversation(turn);
        let mut thinking = json!({"type": "adaptive"});
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": [{"type": "text", "text": SYSTEM_PROMPT}],
            "tools": tool_definitions(),
            "tool_choice": {"type": if allow_tools { "auto" } else { "none" }},
            "output_config": {"effort": self.effort},
            "cache_control": {"type": "ephemeral"},
            "messages": messages,
        });
        if !self.plain_requests {
            // Degrade instead of failing if a replayed thinking block no longer matches.
            thinking["block_binding"] = json!({"prefix_mismatch_behavior": "drop_block"});
            if self.refusal_fallback {
                body["fallbacks"] = json!("default");
            }
        }
        body["thinking"] = thinking;
        body
    }

    /// Drops the oldest half once the limit is passed (rarely, so the prompt cache survives
    /// most turns). Thinking blocks of kept turns were made with the dropped turns in front of
    /// them, so they are removed too; text and tool calls stay.
    fn trim_history(&mut self) {
        if self.history.len() <= self.max_history_turns {
            return;
        }
        let keep = self.max_history_turns / 2;
        let drop = self.history.len() - keep;
        self.history.drain(..drop);
        for turn in &mut self.history {
            strip_thinking(&mut turn.messages);
        }
    }
}

fn context_text(project: &Project, selection_context: &str) -> String {
    let selection = selection_context.trim();
    let selection = if selection.is_empty() {
        "nothing selected"
    } else {
        selection
    };
    format!(
        "<project_state>\n{}\n</project_state>\n<selection>\n{selection}\n</selection>",
        project_summary(project)
    )
}

fn user_message(project: &Project, selection_context: &str, user_text: &str) -> Value {
    let context = context_text(project, selection_context);
    json!({
        "role": "user",
        "content": [
            {"type": "text", "text": context},
            {"type": "text", "text": user_text}
        ]
    })
}

fn block_type(block: &Value) -> &str {
    block.get("type").and_then(Value::as_str).unwrap_or("")
}

/// The response content as it must be sent back. After a server-side fallback, model-internal
/// blocks from before the last `fallback` marker belong to the declined attempt and are omitted.
fn echo_content(content: Option<&Vec<Value>>) -> Vec<Value> {
    let blocks = content.cloned().unwrap_or_default();
    let Some(boundary) = blocks.iter().rposition(|b| block_type(b) == "fallback") else {
        return blocks;
    };
    blocks
        .into_iter()
        .enumerate()
        .filter(|(i, b)| {
            *i >= boundary
                || !matches!(block_type(b), "thinking" | "redacted_thinking" | "tool_use")
        })
        .map(|(_, b)| b)
        .collect()
}

/// An assistant message cannot be empty (a refusal fallback can leave nothing to echo).
fn content_or_placeholder(content: Vec<Value>) -> Vec<Value> {
    if content.is_empty() {
        vec![json!({"type": "text", "text": NO_REPLY})]
    } else {
        content
    }
}

fn strip_thinking(messages: &mut [Value]) {
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
            blocks.retain(|b| !matches!(block_type(b), "thinking" | "redacted_thinking"));
            if blocks.is_empty() {
                blocks.push(json!({"type": "text", "text": NO_REPLY}));
            }
        }
    }
}

fn mentions_beta_field(message: &str) -> bool {
    ["anthropic-beta", "block_binding", "fallbacks"]
        .iter()
        .any(|needle| message.contains(needle))
}
