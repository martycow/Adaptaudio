//! The Anthropic API key lives in the macOS Keychain, never in the project.
//!
//! The key never appears in a process argument list: reading uses `security … -w`, which prints
//! it on stdout, and storing feeds the command to `security -i` on stdin.

use crate::error::AssistantError;
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};

/// Keychain service name of the stored key.
pub const KEYCHAIN_SERVICE: &str = "adaptaudio-anthropic";
const KEYCHAIN_ACCOUNT: &str = "adaptaudio";
const SECURITY: &str = "/usr/bin/security";
/// Development override; wins over the Keychain when set and non-empty.
pub const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
/// `security` exit status for "item not found".
const ITEM_NOT_FOUND: i32 = 44;

/// The key to send: the env override if set, otherwise the Keychain item.
pub fn api_key() -> Result<String, AssistantError> {
    if let Some(key) = env_key() {
        return check_key_format(&key).map(|()| key);
    }
    let output = Command::new(SECURITY)
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .stdin(Stdio::null())
        .output()
        .map_err(security_spawn_error)?;
    if output.status.code() == Some(ITEM_NOT_FOUND) {
        return Err(AssistantError::NoApiKey);
    }
    if !output.status.success() {
        return Err(AssistantError::Keychain(stderr_text(&output.stderr)));
    }
    let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if key.is_empty() {
        return Err(AssistantError::NoApiKey);
    }
    check_key_format(&key)?;
    Ok(key)
}

/// True when a key is available (env override or Keychain item). Does not read the secret.
pub fn has_api_key() -> bool {
    if env_key().is_some() {
        return true;
    }
    Command::new(SECURITY)
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Saves (or replaces) the key in the login Keychain.
pub fn store_api_key(key: &str) -> Result<(), AssistantError> {
    let key = key.trim();
    check_key_format(key)?;
    let mut child = Command::new(SECURITY)
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(security_spawn_error)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(add_command(key).as_bytes())
            .map_err(|e| AssistantError::Keychain(e.to_string()))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| AssistantError::Keychain(e.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AssistantError::Keychain(stderr_text(&output.stderr)))
    }
}

/// The interactive-mode line that stores the key. Only safe for keys that passed
/// [`check_key_format`], which rules out quotes and whitespace.
fn add_command(key: &str) -> String {
    format!(
        "add-generic-password -U -a {KEYCHAIN_ACCOUNT} -s {KEYCHAIN_SERVICE} \
         -l \"Adaptaudio Anthropic API key\" -w \"{key}\"\n"
    )
}

/// Keys are ASCII letters, digits, `-` and `_`. Anything else would let the value break out of
/// the curl config or `security` command line it is placed in.
pub(crate) fn check_key_format(key: &str) -> Result<(), AssistantError> {
    let valid = !key.is_empty()
        && key.len() <= 512
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(AssistantError::InvalidApiKey)
    }
}

fn env_key() -> Option<String> {
    std::env::var(API_KEY_ENV)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

fn security_spawn_error(err: std::io::Error) -> AssistantError {
    if err.kind() == ErrorKind::NotFound {
        AssistantError::Keychain(format!("{SECURITY} not found; the Keychain needs macOS"))
    } else {
        AssistantError::Keychain(err.to_string())
    }
}

fn stderr_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    if text.is_empty() {
        "security exited with an error".to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_format_accepts_real_shapes_and_rejects_injection() {
        assert!(check_key_format("sk-ant-api03-AbC_123-xyz").is_ok());
        assert_eq!(check_key_format(""), Err(AssistantError::InvalidApiKey));
        assert_eq!(
            check_key_format("sk\"-x"),
            Err(AssistantError::InvalidApiKey)
        );
        assert_eq!(check_key_format("sk x"), Err(AssistantError::InvalidApiKey));
        assert_eq!(
            check_key_format("sk\nheader = evil"),
            Err(AssistantError::InvalidApiKey)
        );
    }

    #[test]
    fn add_command_is_one_line_with_the_service_name() {
        let line = add_command("sk-ant-test");
        assert!(line.starts_with("add-generic-password -U "));
        assert!(line.contains("-s adaptaudio-anthropic"));
        assert!(line.ends_with("-w \"sk-ant-test\"\n"));
        assert_eq!(line.matches('\n').count(), 1);
    }
}
