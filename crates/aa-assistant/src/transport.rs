//! Sending a Messages API request. The [`Transport`] trait keeps tests off the network.

use crate::error::AssistantError;
use crate::keychain;
use serde_json::Value;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Beta that enables `"fallbacks": "default"` (server-side refusal fallback).
pub const BETA_FALLBACK: &str = "server-side-fallback-2026-07-01";
/// Beta that enables `thinking.block_binding`.
pub const BETA_THINKING_BINDING: &str = "thinking-binding-controls-2026-08-01";

const STATUS_MARKER: &str = "\n__adaptaudio_http_status__:";

/// Posts one Messages API request body and returns the response message JSON.
pub trait Transport {
    fn send(&self, body: &Value) -> Result<Value, AssistantError>;
}

/// HTTPS through the system `curl`. The whole request, API key included, goes to curl as a
/// config file on stdin, so the key never shows up in `ps`.
#[derive(Clone, Debug)]
pub struct CurlTransport {
    pub url: String,
    pub timeout_secs: u32,
}

impl Default for CurlTransport {
    fn default() -> Self {
        CurlTransport {
            url: MESSAGES_URL.to_string(),
            timeout_secs: 600,
        }
    }
}

impl Transport for CurlTransport {
    fn send(&self, body: &Value) -> Result<Value, AssistantError> {
        let key = keychain::api_key()?;
        self.send_with_key(&key, body)
    }
}

impl CurlTransport {
    fn send_with_key(&self, key: &str, body: &Value) -> Result<Value, AssistantError> {
        keychain::check_key_format(key)?;
        let config = self.curl_config(key, body);
        let output = run_curl(&config)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some((payload, status)) = split_status(&stdout) else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let reason = if stderr.is_empty() {
                format!("curl exited with {}", output.status)
            } else {
                stderr
            };
            return Err(AssistantError::Network(reason));
        };
        if status == 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AssistantError::Network(stderr));
        }
        parse_response(status, payload)
    }

    fn curl_config(&self, key: &str, body: &Value) -> String {
        let mut lines = vec![
            format!("url = {}", quote(&self.url)),
            "request = \"POST\"".to_string(),
            "silent".to_string(),
            "show-error".to_string(),
            format!("max-time = {}", self.timeout_secs),
            "connect-timeout = 30".to_string(),
            format!("header = {}", quote(&format!("x-api-key: {key}"))),
            format!(
                "header = {}",
                quote(&format!("anthropic-version: {ANTHROPIC_VERSION}"))
            ),
            "header = \"content-type: application/json\"".to_string(),
        ];
        let betas = required_betas(body);
        if !betas.is_empty() {
            lines.push(format!(
                "header = {}",
                quote(&format!("anthropic-beta: {}", betas.join(",")))
            ));
        }
        lines.push(format!(
            "write-out = {}",
            quote(&format!("{STATUS_MARKER}%{{http_code}}"))
        ));
        lines.push(format!("data-binary = {}", quote(&body.to_string())));
        let mut config = lines.join("\n");
        config.push('\n');
        config
    }
}

/// The beta headers the body's fields need. Derived from the body so the two never disagree.
pub fn required_betas(body: &Value) -> Vec<&'static str> {
    let mut betas = Vec::new();
    if body.get("fallbacks").is_some() {
        betas.push(BETA_FALLBACK);
    }
    if body.pointer("/thinking/block_binding").is_some() {
        betas.push(BETA_THINKING_BINDING);
    }
    betas
}

fn run_curl(config: &str) -> Result<std::process::Output, AssistantError> {
    let program = if Path::new("/usr/bin/curl").exists() {
        "/usr/bin/curl"
    } else {
        "curl"
    };
    let mut child = Command::new(program)
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => AssistantError::CurlMissing,
            _ => AssistantError::Network(format!("could not start curl: {e}")),
        })?;
    // curl reads the whole config before it connects, so writing it all first cannot deadlock.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(config.as_bytes())
            .map_err(|e| AssistantError::Network(format!("could not talk to curl: {e}")))?;
    }
    child
        .wait_with_output()
        .map_err(|e| AssistantError::Network(format!("curl failed: {e}")))
}

/// A curl config string value: quoted, with `\` and `"` escaped. JSON never contains raw
/// newlines, so these two escapes are enough.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Splits curl's stdout into the response body and the status code from `write-out`.
fn split_status(stdout: &str) -> Option<(&str, u16)> {
    let at = stdout.rfind(STATUS_MARKER)?;
    let status = stdout[at + STATUS_MARKER.len()..].trim().parse().ok()?;
    Some((&stdout[..at], status))
}

/// Turns an HTTP status and body into the message JSON or a readable error.
pub(crate) fn parse_response(status: u16, payload: &str) -> Result<Value, AssistantError> {
    let parsed: Result<Value, _> = serde_json::from_str(payload);
    if (200..300).contains(&status) {
        let value = parsed.map_err(|e| AssistantError::BadJson(e.to_string()))?;
        if value.get("content").and_then(Value::as_array).is_none() {
            return Err(AssistantError::BadJson(
                "response has no content array".to_string(),
            ));
        }
        return Ok(value);
    }
    let (kind, message) = match &parsed {
        Ok(value) => (
            value
                .pointer("/error/type")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string(),
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
        Err(_) => ("error".to_string(), String::new()),
    };
    let message = if message.is_empty() {
        let snippet: String = payload.trim().chars().take(200).collect();
        if snippet.is_empty() {
            "no details".to_string()
        } else {
            snippet
        }
    } else {
        message
    };
    Err(AssistantError::Http {
        status,
        kind,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quote_escapes_backslashes_and_quotes() {
        assert_eq!(quote(r#"{"a":"x\"y\\n"}"#), r#""{\"a\":\"x\\\"y\\\\n\"}""#);
        assert_eq!(quote("plain"), "\"plain\"");
    }

    #[test]
    fn config_carries_key_in_a_header_and_body_last() {
        let body = json!({"model": "m", "fallbacks": "default", "messages": []});
        let config = CurlTransport::default().curl_config("sk-test", &body);
        assert!(config.contains("header = \"x-api-key: sk-test\""));
        assert!(config.contains(&format!("anthropic-version: {ANTHROPIC_VERSION}")));
        assert!(config.contains(&format!("anthropic-beta: {BETA_FALLBACK}\"")));
        let last = config.trim_end().lines().last().unwrap_or_default();
        assert!(last.starts_with("data-binary = \"{"));
    }

    #[test]
    fn betas_follow_body_fields() {
        assert!(required_betas(&json!({})).is_empty());
        let body = json!({"fallbacks": "default", "thinking": {"block_binding": {}}});
        assert_eq!(
            required_betas(&body),
            vec![BETA_FALLBACK, BETA_THINKING_BINDING]
        );
    }

    #[test]
    fn status_is_split_from_body() {
        let out = format!("{{\"ok\":1}}{STATUS_MARKER}200");
        assert_eq!(split_status(&out), Some(("{\"ok\":1}", 200)));
        assert_eq!(split_status("no marker"), None);
    }

    /// Runs the real curl against a one-shot server on 127.0.0.1 (no outside network).
    #[test]
    fn curl_round_trip_on_loopback() {
        use std::io::{BufRead, BufReader, Read};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut headers = Vec::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end().to_string();
                if line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                headers.push(line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let reply = r#"{"type":"message","stop_reason":"end_turn","content":[{"type":"text","text":"ок"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );
            reader.get_mut().write_all(response.as_bytes()).unwrap();
            (headers, String::from_utf8(body).unwrap())
        });

        let transport = CurlTransport {
            url: format!("http://127.0.0.1:{port}/v1/messages"),
            timeout_secs: 10,
        };
        let body = json!({"messages": [{"role": "user", "content": "кавычка \" и \\ слэш"}], "fallbacks": "default"});
        let answer = transport.send_with_key("sk-loopback-test", &body).unwrap();
        assert_eq!(answer["content"][0]["text"], "ок");

        let (headers, sent) = server.join().unwrap();
        assert!(
            headers.iter().any(|h| h == "x-api-key: sk-loopback-test"),
            "{headers:?}"
        );
        assert!(
            headers
                .iter()
                .any(|h| h == &format!("anthropic-beta: {BETA_FALLBACK}"))
        );
        let sent: Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(sent, body, "body arrives byte-for-byte equivalent");
    }

    #[test]
    fn http_errors_carry_the_api_message() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let err = parse_response(401, body).unwrap_err();
        assert_eq!(
            err,
            AssistantError::Http {
                status: 401,
                kind: "authentication_error".into(),
                message: "invalid x-api-key".into()
            }
        );
        assert!(err.to_string().contains("invalid x-api-key"));
        assert!(matches!(
            parse_response(200, "not json"),
            Err(AssistantError::BadJson(_))
        ));
        assert!(matches!(
            parse_response(200, "{}"),
            Err(AssistantError::BadJson(_))
        ));
        assert!(parse_response(200, r#"{"content":[]}"#).is_ok());
    }
}
