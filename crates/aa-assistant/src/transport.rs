//! Sending an API request. The [`Transport`] trait keeps tests off the network.

use crate::error::AssistantError;
use crate::keychain;
use crate::provider::Provider;
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

/// One request: which provider it is for, where it goes and what it carries.
#[derive(Clone, Copy, Debug)]
pub struct ApiRequest<'a> {
    pub provider: Provider,
    /// Full URL to post to (see [`Provider::endpoint`]); `None` = the transport's default for
    /// the provider.
    pub url: Option<&'a str>,
    pub body: &'a Value,
}

/// Posts one request body and returns the response JSON.
pub trait Transport {
    /// Posts an Anthropic Messages API body to the transport's default URL.
    fn send(&self, body: &Value) -> Result<Value, AssistantError>;

    /// Posts a request for any provider. [`crate::Assistant`] only calls this method. The
    /// default forwards the body to [`Transport::send`], which is enough for scripted test
    /// transports; a real transport must override it to honour the provider and URL.
    fn send_request(&self, request: &ApiRequest<'_>) -> Result<Value, AssistantError> {
        self.send(request.body)
    }
}

/// HTTPS through the system `curl`. The whole request, API key included, goes to curl as a
/// config file on stdin, so the key never shows up in `ps`.
#[derive(Clone, Debug)]
pub struct CurlTransport {
    /// Anthropic Messages URL, used when a request names no URL. The other providers default
    /// to [`Provider::default_base_url`].
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
        self.send_request(&ApiRequest {
            provider: Provider::Anthropic,
            url: None,
            body,
        })
    }

    fn send_request(&self, request: &ApiRequest<'_>) -> Result<Value, AssistantError> {
        let key = keychain::api_key_for(request.provider)?;
        self.send_with_key(&key, request)
    }
}

impl CurlTransport {
    /// Errors never contain the key: providers like to quote it back in a 401 message.
    fn send_with_key(&self, key: &str, request: &ApiRequest<'_>) -> Result<Value, AssistantError> {
        self.send_unredacted(key, request)
            .map_err(|err| redact_error(err, key))
    }

    fn send_unredacted(
        &self,
        key: &str,
        request: &ApiRequest<'_>,
    ) -> Result<Value, AssistantError> {
        keychain::check_key_format(key)?;
        let config = self.curl_config(key, request)?;
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
        parse_response(request.provider, status, payload)
    }

    fn request_url(&self, request: &ApiRequest<'_>) -> Result<String, AssistantError> {
        let url = match (request.url, request.provider) {
            (Some(url), _) => url.trim().to_string(),
            (None, Provider::Anthropic) => self.url.clone(),
            (None, provider) => provider.endpoint(provider.default_base_url()),
        };
        // curl also speaks file://, ftp:// and friends; the key must only go out over HTTP(S).
        if url.starts_with("https://") || url.starts_with("http://") {
            Ok(url)
        } else {
            Err(AssistantError::Network(
                "the API address must start with https://".to_string(),
            ))
        }
    }

    fn curl_config(&self, key: &str, request: &ApiRequest<'_>) -> Result<String, AssistantError> {
        let body = request.body;
        let mut lines = vec![
            format!("url = {}", quote(&self.request_url(request)?)),
            "request = \"POST\"".to_string(),
            "silent".to_string(),
            "show-error".to_string(),
            format!("max-time = {}", self.timeout_secs),
            "connect-timeout = 30".to_string(),
            "header = \"content-type: application/json\"".to_string(),
        ];
        let mut header = |text: String| lines.push(format!("header = {}", quote(&text)));
        if request.provider.is_openai_compatible() {
            header(format!("authorization: Bearer {key}"));
        } else {
            header(format!("x-api-key: {key}"));
            header(format!("anthropic-version: {ANTHROPIC_VERSION}"));
            let betas = required_betas(body);
            if !betas.is_empty() {
                header(format!("anthropic-beta: {}", betas.join(",")));
            }
        }
        lines.push(format!(
            "write-out = {}",
            quote(&format!("{STATUS_MARKER}%{{http_code}}"))
        ));
        lines.push(format!("data-binary = {}", quote(&body.to_string())));
        let mut config = lines.join("\n");
        config.push('\n');
        Ok(config)
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

/// Removes the key, and anything shaped like a secret key (`sk-…`, even partly masked), from
/// text that will be shown to the user.
pub(crate) fn redact(text: &str, key: &str) -> String {
    const MASK: &str = "[key hidden]";
    let text = if key.is_empty() {
        text.to_string()
    } else {
        text.replace(key, MASK)
    };
    let is_key_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '*' | '.');
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("sk-") {
        let starts_word = !rest[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        let len = rest[at..]
            .find(|c| !is_key_char(c))
            .unwrap_or(rest.len() - at);
        out.push_str(&rest[..at]);
        if starts_word && len >= 8 {
            out.push_str(MASK);
        } else {
            out.push_str(&rest[at..at + len]);
        }
        rest = &rest[at + len..];
    }
    out.push_str(rest);
    out
}

fn redact_error(err: AssistantError, key: &str) -> AssistantError {
    match err {
        AssistantError::Http {
            status,
            kind,
            message,
        } => AssistantError::Http {
            status,
            kind: redact(&kind, key),
            message: redact(&message, key),
        },
        AssistantError::Network(msg) => AssistantError::Network(redact(&msg, key)),
        AssistantError::BadJson(msg) => AssistantError::BadJson(redact(&msg, key)),
        other => other,
    }
}

/// Turns an HTTP status and body into the response JSON or a readable error. Anthropic and
/// the OpenAI-compatible APIs both report errors as `{"error": {"type", "message"}}`.
pub(crate) fn parse_response(
    provider: Provider,
    status: u16,
    payload: &str,
) -> Result<Value, AssistantError> {
    let parsed: Result<Value, _> = serde_json::from_str(payload);
    if (200..300).contains(&status) {
        let value = parsed.map_err(|e| AssistantError::BadJson(e.to_string()))?;
        let expected = if provider.is_openai_compatible() {
            "choices"
        } else {
            "content"
        };
        if value.get(expected).and_then(Value::as_array).is_none() {
            return Err(AssistantError::BadJson(format!(
                "response has no {expected} array"
            )));
        }
        return Ok(value);
    }
    let (kind, message) = match &parsed {
        Ok(value) => (
            value
                .pointer("/error/type")
                .and_then(Value::as_str)
                .or_else(|| value.pointer("/error/code").and_then(Value::as_str))
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

    fn anthropic(body: &Value) -> ApiRequest<'_> {
        ApiRequest {
            provider: Provider::Anthropic,
            url: None,
            body,
        }
    }

    /// Answers one HTTP request on 127.0.0.1 with `status_line` and `reply`; returns the
    /// request line + headers and the body it received.
    fn one_shot_server(
        status_line: &'static str,
        reply: &'static str,
    ) -> (u16, std::thread::JoinHandle<(Vec<String>, String)>) {
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
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );
            reader.get_mut().write_all(response.as_bytes()).unwrap();
            (headers, String::from_utf8(body).unwrap())
        });
        (port, server)
    }

    #[test]
    fn quote_escapes_backslashes_and_quotes() {
        assert_eq!(quote(r#"{"a":"x\"y\\n"}"#), r#""{\"a\":\"x\\\"y\\\\n\"}""#);
        assert_eq!(quote("plain"), "\"plain\"");
    }

    #[test]
    fn config_carries_key_in_a_header_and_body_last() {
        let body = json!({"model": "m", "fallbacks": "default", "messages": []});
        let config = CurlTransport::default()
            .curl_config("sk-test", &anthropic(&body))
            .unwrap();
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
        let (port, server) = one_shot_server(
            "200 OK",
            r#"{"type":"message","stop_reason":"end_turn","content":[{"type":"text","text":"ок"}]}"#,
        );

        let transport = CurlTransport {
            url: format!("http://127.0.0.1:{port}/v1/messages"),
            timeout_secs: 10,
        };
        let body = json!({"messages": [{"role": "user", "content": "кавычка \" и \\ слэш"}], "fallbacks": "default"});
        let answer = transport
            .send_with_key("sk-loopback-test", &anthropic(&body))
            .unwrap();
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
        let err = parse_response(Provider::Anthropic, 401, body).unwrap_err();
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
            parse_response(Provider::Anthropic, 200, "not json"),
            Err(AssistantError::BadJson(_))
        ));
        assert!(matches!(
            parse_response(Provider::Anthropic, 200, "{}"),
            Err(AssistantError::BadJson(_))
        ));
        assert!(parse_response(Provider::Anthropic, 200, r#"{"content":[]}"#).is_ok());
    }

    #[test]
    fn openai_config_uses_a_bearer_header_and_the_chat_completions_url() {
        let body = json!({"model": "m", "messages": []});
        let request = |provider, url| ApiRequest {
            provider,
            url,
            body: &body,
        };
        let transport = CurlTransport::default();
        let config = transport
            .curl_config("sk-test", &request(Provider::OpenAi, None))
            .unwrap();
        assert!(config.contains("url = \"https://api.openai.com/v1/chat/completions\""));
        assert!(config.contains("header = \"authorization: Bearer sk-test\""));
        assert!(!config.contains("x-api-key") && !config.contains("anthropic"));

        let config = transport
            .curl_config("sk-test", &request(Provider::Qwen, None))
            .unwrap();
        assert!(config.contains(
            "url = \"https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions\""
        ));

        let custom = Provider::Qwen.endpoint("https://example.test/v1/");
        let config = transport
            .curl_config("sk-test", &request(Provider::Qwen, Some(&custom)))
            .unwrap();
        assert!(config.contains("url = \"https://example.test/v1/chat/completions\""));

        let err = transport
            .curl_config(
                "sk-test",
                &request(Provider::OpenAi, Some("file:///etc/passwd")),
            )
            .unwrap_err();
        assert!(matches!(err, AssistantError::Network(_)));
    }

    #[test]
    fn openai_responses_and_errors_are_recognised() {
        let ok = r#"{"choices":[{"message":{"role":"assistant","content":"hi"}}]}"#;
        assert!(parse_response(Provider::OpenAi, 200, ok).is_ok());
        assert!(matches!(
            parse_response(Provider::OpenAi, 200, r#"{"content":[]}"#),
            Err(AssistantError::BadJson(_))
        ));
        let body = r#"{"error":{"message":"The model `x` does not exist","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
        assert_eq!(
            parse_response(Provider::OpenAi, 404, body),
            Err(AssistantError::Http {
                status: 404,
                kind: "invalid_request_error".into(),
                message: "The model `x` does not exist".into()
            })
        );
        let no_type = r#"{"error":{"message":"Invalid API-key provided.","type":null,"code":"invalid_api_key"}}"#;
        assert!(matches!(
            parse_response(Provider::Qwen, 401, no_type),
            Err(AssistantError::Http { kind, .. }) if kind == "invalid_api_key"
        ));
    }

    #[test]
    fn redact_hides_the_key_and_key_shaped_text() {
        let text =
            "Incorrect API key provided: sk-proj-abcdef123456. Also sk-pr*******3456, task-based.";
        let clean = redact(text, "sk-proj-abcdef123456");
        assert!(
            !clean.contains("abcdef") && !clean.contains("3456"),
            "{clean}"
        );
        assert!(clean.contains("Incorrect API key provided") && clean.contains("task-based"));
        assert_eq!(redact("plain words", "secretkey"), "plain words");
        assert_eq!(
            redact("got secretkey back", "secretkey"),
            "got [key hidden] back"
        );
    }

    /// Real curl against a loopback server that answers 401 and quotes the key back.
    #[test]
    fn openai_error_on_loopback_never_leaks_the_key() {
        let (port, server) = one_shot_server(
            "401 Unauthorized",
            r#"{"error":{"message":"Incorrect API key provided: sk-loopback-secret-1234.","type":"invalid_request_error","code":"invalid_api_key"}}"#,
        );
        let base = format!("http://127.0.0.1:{port}/v1");
        let url = Provider::OpenAi.endpoint(&base);
        let body = json!({"model": "m", "messages": [{"role": "user", "content": "привет"}]});
        let request = ApiRequest {
            provider: Provider::OpenAi,
            url: Some(&url),
            body: &body,
        };
        let err = CurlTransport::default()
            .send_with_key("sk-loopback-secret-1234", &request)
            .unwrap_err();
        assert!(
            matches!(err, AssistantError::Http { status: 401, .. }),
            "{err:?}"
        );
        assert!(!format!("{err} {err:?}").contains("secret"), "{err:?}");

        let (headers, sent) = server.join().unwrap();
        assert!(
            headers[0].starts_with("POST /v1/chat/completions "),
            "{headers:?}"
        );
        assert!(
            headers
                .iter()
                .any(|h| h.eq_ignore_ascii_case("authorization: Bearer sk-loopback-secret-1234")),
            "{headers:?}"
        );
        assert!(
            !headers
                .iter()
                .any(|h| h.to_ascii_lowercase().starts_with("x-api-key"))
        );
        assert_eq!(serde_json::from_str::<Value>(&sent).unwrap(), body);
    }
}
