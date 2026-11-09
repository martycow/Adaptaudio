//! The OpenAI-compatible Chat Completions wire format (OpenAI and Qwen/DashScope): request
//! bodies, tool definitions and reading the answer. No I/O here.

use crate::actions::tool_definitions;
use crate::error::AssistantError;
use crate::provider::Provider;
use serde_json::{Value, json};

/// One tool call of an answer. `input` is the parsed arguments, or why they could not be read.
pub(crate) struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Result<Value, String>,
}

/// One model answer, reduced to what the tool loop needs.
pub(crate) struct Answer {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    /// The answer hit the token limit (`finish_reason: "length"`).
    pub truncated: bool,
    /// The assistant message to replay in later requests.
    pub message: Value,
}

/// The crate's tool list as Chat Completions `tools`. Same names, descriptions and JSON
/// schemas as [`tool_definitions`], so [`crate::ActionValidator`] checks both alike.
pub(crate) fn tools() -> Value {
    let converted = tool_definitions()
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool["name"],
                            "description": tool["description"],
                            "parameters": tool["input_schema"],
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(converted)
}

pub(crate) fn request_body(
    provider: Provider,
    model: &str,
    max_tokens: u32,
    system: &str,
    conversation: Vec<Value>,
    allow_tools: bool,
) -> Value {
    let mut messages = Vec::with_capacity(conversation.len() + 1);
    messages.push(json!({"role": "system", "content": system}));
    messages.extend(conversation);
    let mut body = json!({
        "model": model,
        "messages": messages,
        "tools": tools(),
        "tool_choice": if allow_tools { "auto" } else { "none" },
    });
    // OpenAI's current models reject `max_tokens`; DashScope only documents `max_tokens`.
    let limit = match provider {
        Provider::OpenAi => "max_completion_tokens",
        Provider::Anthropic | Provider::Qwen => "max_tokens",
    };
    body[limit] = json!(max_tokens);
    body
}

pub(crate) fn user_message(text: &str) -> Value {
    json!({"role": "user", "content": text})
}

pub(crate) fn assistant_message(text: &str) -> Value {
    json!({"role": "assistant", "content": text})
}

pub(crate) fn tool_result(tool_call_id: &str, content: &str) -> Value {
    json!({"role": "tool", "tool_call_id": tool_call_id, "content": content})
}

/// Reads `choices[0]` of a Chat Completions response.
pub(crate) fn parse_answer(response: &Value) -> Result<Answer, AssistantError> {
    let choice = response
        .pointer("/choices/0")
        .ok_or_else(|| AssistantError::BadJson("response has no choices".to_string()))?;
    let message = choice
        .get("message")
        .filter(|m| m.is_object())
        .ok_or_else(|| AssistantError::BadJson("the choice has no message".to_string()))?;
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let refusal = message
        .get("refusal")
        .and_then(Value::as_str)
        .is_some_and(|r| !r.trim().is_empty());
    if refusal || finish_reason == "content_filter" {
        return Err(AssistantError::Refused { category: None });
    }

    let text = content_text(message.get("content"));
    let tool_calls: Vec<ToolCall> = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter(|call| call.get("function").is_some())
                .enumerate()
                .map(|(index, call)| tool_call(index, call))
                .collect()
        })
        .unwrap_or_default();

    let mut echo = assistant_message(&text);
    if tool_calls.is_empty() {
        if text.trim().is_empty() {
            echo["content"] = json!("(no reply)");
        }
    } else {
        let calls: Vec<Value> = tool_calls
            .iter()
            .map(|call| {
                // Unreadable arguments are replayed as `{}`: some servers reject a history
                // whose arguments are not valid JSON.
                let arguments = call
                    .input
                    .as_ref()
                    .map_or_else(|_| "{}".to_string(), Value::to_string);
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": arguments}
                })
            })
            .collect();
        echo["tool_calls"] = Value::Array(calls);
    }

    Ok(Answer {
        text: text.trim().to_string(),
        tool_calls,
        truncated: finish_reason == "length",
        message: echo,
    })
}

fn tool_call(index: usize, call: &Value) -> ToolCall {
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(|| format!("call_{index}"), str::to_string);
    let name = call
        .pointer("/function/name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let input = match call.pointer("/function/arguments") {
        // The standard form: a JSON document in a string. Empty means "no arguments".
        Some(Value::String(raw)) if raw.trim().is_empty() => Ok(json!({})),
        Some(Value::String(raw)) => {
            serde_json::from_str(raw).map_err(|e| format!("the arguments are not valid JSON ({e})"))
        }
        Some(Value::Object(map)) => Ok(Value::Object(map.clone())),
        None | Some(Value::Null) => Ok(json!({})),
        Some(_) => Err("the arguments are not a JSON object".to_string()),
    };
    ToolCall { id, name, input }
}

/// Message content is a string, or (some servers) a list of `{"type": "text", "text"}` parts.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}
