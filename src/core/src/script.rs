//! Running startkit scripts and reading their NDJSON output.
//!
//! Scripts are the install and detection primitive: they receive their inputs as
//! environment variables and stream `{"event":"progress"}` lines while they
//! work, ending with a result object. This module owns spawning, streaming,
//! cancellation, and parsing, so both the desktop startkit runner and the
//! daemon's own lazy toolchain bootstrap drive scripts the same way.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::sleep;

/// The result object a script ends with.
#[derive(Debug, Clone, Deserialize)]
pub struct ScriptOutcome {
    pub status: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub manual_command: Option<String>,
    #[serde(default)]
    pub manual_url: Option<String>,
}

/// One line of a script's NDJSON stdout.
///
/// Scripts stream `{"event":"progress","message":"…"}` lines while they work and
/// finish with a result line. A result is either the explicit
/// `{"event":"result", …}` form or a bare `{"status":"…"}` object, so scripts
/// written before progress streaming keep working unchanged.
enum ScriptLine {
    Progress(Option<String>),
    Result,
}

fn classify_line(line: &str) -> Option<ScriptLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    match object.get("event").and_then(|value| value.as_str()) {
        Some("progress") => Some(ScriptLine::Progress(
            object
                .get("message")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        )),
        Some("result") => Some(ScriptLine::Result),
        // An unrecognized event kind is ignored rather than mistaken for a result.
        Some(_) => None,
        None => object.contains_key("status").then_some(ScriptLine::Result),
    }
}

/// Builds the interpreter invocation for a script, picking PowerShell on Windows
/// and `sh` everywhere else.
pub fn command_for(script: &Path, args: &[String], platform: &str) -> tokio::process::Command {
    let mut command = if platform == "windows" {
        let mut command = crate::process::env::silent_command("powershell.exe");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        command.arg(script);
        command
    } else {
        let mut command = crate::process::env::silent_command("sh");
        command.arg(script);
        command
    };
    command.args(args);
    command
}

/// Runs a script to completion, forwarding each progress message as it arrives
/// and returning the result line.
///
/// `redact_keys` is applied to progress messages and to any stderr surfaced in an
/// error, because installers echo command lines that can carry secrets.
pub async fn run(
    mut command: tokio::process::Command,
    env: &BTreeMap<String, String>,
    timeout: Duration,
    cancelled: Option<&Arc<AtomicBool>>,
    redact_keys: &[String],
    mut on_progress: impl FnMut(String),
) -> anyhow::Result<ScriptOutcome> {
    command.env_clear();
    command.envs(crate::process::env::enriched_env().clone());
    command.envs(env);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    // The last result line wins.
    let mut result_line: Option<String> = None;
    let stderr = stream_command(command, timeout, cancelled, |line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('{') {
            return;
        }
        match classify_line(trimmed) {
            Some(ScriptLine::Progress(message)) => {
                if let Some(message) = message {
                    on_progress(redact(&message, redact_keys));
                }
            }
            Some(ScriptLine::Result) => result_line = Some(trimmed.to_string()),
            None => {}
        }
    })
    .await?;

    let stderr = String::from_utf8_lossy(&stderr);
    let line = result_line.ok_or_else(|| {
        anyhow!(
            "script did not emit JSON{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", redact(&stderr, redact_keys))
            }
        )
    })?;

    serde_json::from_str(&line).with_context(|| format!("parsing script JSON: {line}"))
}

/// Runs the child, delivering each stdout line to `on_line` as it arrives, and
/// returns the collected stderr.
async fn stream_command(
    mut command: tokio::process::Command,
    max_duration: Duration,
    cancelled: Option<&Arc<AtomicBool>>,
    mut on_line: impl FnMut(&str),
) -> anyhow::Result<Vec<u8>> {
    let mut child =
        crate::process::spawn_tree_killable(&mut command).context("spawning script")?;
    let stdout = child
        .take_stdout()
        .ok_or_else(|| anyhow!("script stdout was not captured"))?;
    let mut stderr = child
        .take_stderr()
        .ok_or_else(|| anyhow!("script stderr was not captured"))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let started = Instant::now();
    loop {
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
        }
        if cancelled
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            let _ = child.terminate_tree().await;
            bail!("cancelled");
        }
        if started.elapsed() >= max_duration {
            let _ = child.terminate_tree().await;
            bail!("script timed out");
        }
        if child.try_wait().context("polling script")?.is_some() {
            break;
        }
        sleep(Duration::from_millis(200)).await;
    }

    // The reader may still hold buffered lines after the child exits; wait for it
    // so no progress or result line is dropped.
    let _ = stdout_task.await;
    while let Some(line) = rx.recv().await {
        on_line(&line);
    }

    stderr_task
        .await
        .context("joining script stderr reader")?
        .context("reading script stderr")
}

/// Masks the values of the named keys, leaving the keys themselves visible.
///
/// Script output is user-facing, and installers echo command lines that can
/// carry tokens, so everything read back from a script passes through here.
pub fn redact(value: &str, keys: &[String]) -> String {
    let mut out = value.to_string();
    for key in keys {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out = redact_key_values(&out, key);
    }
    out
}

fn redact_key_values(value: &str, key: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let key_lower = key.to_ascii_lowercase();
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut index = 0;

    while let Some(relative) = lower[index..].find(&key_lower) {
        let start = index + relative;
        let key_end = start + key.len();
        if !has_key_boundaries(bytes, start, key_end) {
            out.push_str(&value[index..key_end]);
            index = key_end;
            continue;
        }

        let Some((value_start, value_end)) = redaction_value_span(value, key_end) else {
            out.push_str(&value[index..key_end]);
            index = key_end;
            continue;
        };
        out.push_str(&value[index..value_start]);
        out.push_str("***");
        index = value_end;
    }

    out.push_str(&value[index..]);
    out
}

fn has_key_boundaries(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_key_char(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_key_char(bytes[end]);
    before_ok && after_ok
}

fn is_key_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn redaction_value_span(value: &str, key_end: usize) -> Option<(usize, usize)> {
    let bytes = value.as_bytes();
    let mut cursor = key_end;

    if matches!(bytes.get(cursor), Some(b'"' | b'\'')) {
        cursor += 1;
    }

    let whitespace_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let saw_whitespace = cursor > whitespace_start;

    if matches!(bytes.get(cursor), Some(b'=' | b':')) {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
    } else if !saw_whitespace {
        return None;
    }

    let quote = match bytes.get(cursor) {
        Some(b'"') => Some(b'"'),
        Some(b'\'') => Some(b'\''),
        _ => None,
    };
    if let Some(quote) = quote {
        let value_start = cursor + 1;
        let value_end = bytes[value_start..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|offset| value_start + offset)
            .unwrap_or(bytes.len());
        return (value_start < value_end).then_some((value_start, value_end));
    }

    let value_start = cursor;
    let mut value_end = cursor;
    while let Some(byte) = bytes.get(value_end) {
        if byte.is_ascii_whitespace() || matches!(byte, b',' | b';' | b'&') {
            break;
        }
        value_end += 1;
    }
    (value_start < value_end).then_some((value_start, value_end))
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn masks_secret_values_without_removing_keys() {
        let keys = vec!["token".to_string(), "api_key".to_string()];
        let redacted = redact(
            r#"token=abc123 api_key: "sk-test" {"token":"cloudflare-secret"} --token cli-secret tokenizer=kept"#,
            &keys,
        );

        assert!(redacted.contains("token=***"));
        assert!(redacted.contains("api_key: \"***\""));
        assert!(redacted.contains(r#""token":"***""#));
        assert!(redacted.contains("--token ***"));
        assert!(redacted.contains("tokenizer=kept"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("sk-test"));
        assert!(!redacted.contains("cloudflare-secret"));
        assert!(!redacted.contains("cli-secret"));
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::{classify_line, ScriptLine};

    #[test]
    fn legacy_bare_status_object_is_a_result() {
        assert!(matches!(
            classify_line(r#"{"status":"ok","version":"22.11.0"}"#),
            Some(ScriptLine::Result)
        ));
    }

    #[test]
    fn explicit_result_event_is_a_result() {
        assert!(matches!(
            classify_line(r#"{"event":"result","status":"ok"}"#),
            Some(ScriptLine::Result)
        ));
    }

    #[test]
    fn progress_event_carries_its_message() {
        let Some(ScriptLine::Progress(message)) =
            classify_line(r#"{"event":"progress","message":"Extracting"}"#)
        else {
            panic!("expected a progress line");
        };
        assert_eq!(message.as_deref(), Some("Extracting"));
    }

    #[test]
    fn progress_event_without_a_message_is_still_progress() {
        assert!(matches!(
            classify_line(r#"{"event":"progress"}"#),
            Some(ScriptLine::Progress(None))
        ));
    }

    #[test]
    fn unknown_event_kinds_are_ignored_rather_than_read_as_results() {
        assert!(classify_line(r#"{"event":"telemetry","status":"ok"}"#).is_none());
    }

    #[test]
    fn non_object_and_non_json_lines_are_ignored() {
        assert!(classify_line("Downloading...").is_none());
        assert!(classify_line(r#"["status"]"#).is_none());
        assert!(classify_line(r#"{"note":"no status here"}"#).is_none());
    }
}
