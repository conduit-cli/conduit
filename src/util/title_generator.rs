//! Session title and branch name generation using AI
//!
//! This module handles generating descriptive session titles and branch names
//! from the first user message in a session using Claude or Codex.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;

use crate::agent::{AgentEvent, AgentRunner, AgentStartConfig, CodexCliRunner};

use super::{Tool, ToolAvailability};

/// Timeout for AI title generation calls
const AI_CALL_TIMEOUT_SECS: u64 = 20;

/// Result of title/branch generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedMetadata {
    /// One-line session title/description
    pub title: String,
    /// Short branch name suffix (kebab-case, no slashes)
    pub branch_suffix: String,
    /// Tool used to generate the metadata (set by generate_title_and_branch for GeneratedMetadata::tool_used)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_used: Option<String>,
    /// Whether the result came from a fallback tool
    #[serde(default)]
    pub used_fallback: bool,
}

/// Error during title generation
#[derive(Debug, Error)]
pub enum TitleGeneratorError {
    #[error("No AI tool available")]
    NoToolAvailable,
    #[error("AI call failed: {0}")]
    AiCallFailed(String),
    #[error("Failed to parse response: {0}")]
    ParseError(String),
    #[error("AI call timed out after {0} seconds")]
    Timeout(u64),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Generate a session title and branch name from the first user message
pub async fn generate_title_and_branch(
    tools: &ToolAvailability,
    user_message: &str,
    working_dir: &PathBuf,
) -> Result<GeneratedMetadata, TitleGeneratorError> {
    let prompt = format!(
        r#"Based on this user request, generate:
1. A concise one-line title (max 50 chars) describing the task
2. A short branch name suffix (3-4 words, kebab-case, no slashes, max 30 chars)

User request: "{}"

Respond ONLY with valid JSON (no markdown, no explanation):
{{"title": "...", "branch_suffix": "..."}}"#,
        truncate_message(user_message, 500)
    );

    let mut failures: Vec<(Tool, TitleGeneratorError)> = Vec::new();

    if tools.is_available(Tool::Claude) {
        if let Some(tool_path) = tools.get_path(Tool::Claude) {
            let result = tokio::time::timeout(
                Duration::from_secs(AI_CALL_TIMEOUT_SECS),
                call_claude(tool_path, &prompt, working_dir),
            )
            .await;
            match result {
                Ok(Ok(mut metadata)) => {
                    metadata.tool_used = Some(Tool::Claude.display_name().to_string());
                    metadata.used_fallback = false;
                    return Ok(metadata);
                }
                Ok(Err(err)) => failures.push((Tool::Claude, err)),
                Err(_) => failures.push((
                    Tool::Claude,
                    TitleGeneratorError::Timeout(AI_CALL_TIMEOUT_SECS),
                )),
            }
        } else {
            failures.push((
                Tool::Claude,
                TitleGeneratorError::AiCallFailed("Claude tool path missing".to_string()),
            ));
        }
    }

    if tools.is_available(Tool::Codex) {
        if let Some(tool_path) = tools.get_path(Tool::Codex) {
            let result = tokio::time::timeout(
                Duration::from_secs(AI_CALL_TIMEOUT_SECS),
                call_codex(tool_path, &prompt, working_dir),
            )
            .await;
            match result {
                Ok(Ok(mut metadata)) => {
                    metadata.tool_used = Some(Tool::Codex.display_name().to_string());
                    metadata.used_fallback = !failures.is_empty();
                    return Ok(metadata);
                }
                Ok(Err(err)) => failures.push((Tool::Codex, err)),
                Err(_) => failures.push((
                    Tool::Codex,
                    TitleGeneratorError::Timeout(AI_CALL_TIMEOUT_SECS),
                )),
            }
        } else {
            failures.push((
                Tool::Codex,
                TitleGeneratorError::AiCallFailed("Codex tool path missing".to_string()),
            ));
        }
    }

    if failures.is_empty() {
        return Err(TitleGeneratorError::NoToolAvailable);
    }
    if failures.len() == 1 {
        return Err(failures.remove(0).1);
    }

    let details = failures
        .into_iter()
        .map(|(tool, err)| format!("{}: {}", tool.display_name(), err))
        .collect::<Vec<_>>()
        .join("; ");

    Err(TitleGeneratorError::AiCallFailed(details))
}

async fn call_claude(
    binary_path: &PathBuf,
    prompt: &str,
    working_dir: &PathBuf,
) -> Result<GeneratedMetadata, TitleGeneratorError> {
    let mut cmd = Command::new(binary_path);
    cmd.args(["-p", "--output-format", "text", "--model", "sonnet"]);
    cmd.arg("--").arg(prompt);
    cmd.current_dir(working_dir);
    cmd.kill_on_drop(true);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TitleGeneratorError::AiCallFailed(format!(
            "Claude process failed: {}",
            stderr
        )));
    }

    let response = String::from_utf8_lossy(&output.stdout);
    parse_json_response(&response)
}

async fn call_codex(
    binary_path: &PathBuf,
    prompt: &str,
    working_dir: &PathBuf,
) -> Result<GeneratedMetadata, TitleGeneratorError> {
    let runner = CodexCliRunner::with_path(binary_path.clone());
    let config = AgentStartConfig::new(prompt, working_dir.clone());
    let mut handle = runner
        .start(config)
        .await
        .map_err(|err| TitleGeneratorError::AiCallFailed(err.to_string()))?;

    let mut response = String::new();

    while let Some(event) = handle.events.recv().await {
        match event {
            AgentEvent::AssistantMessage(msg) => {
                response.push_str(&msg.text);
                if msg.is_final {
                    // Continue; we still wait for TurnCompleted to avoid truncation.
                }
            }
            AgentEvent::TurnCompleted(_) => break,
            AgentEvent::Error(err) => {
                if let Err(stop_err) = runner.stop(&handle).await {
                    tracing::debug!(
                        error = %stop_err,
                        "Failed to stop Codex app-server after title generation error"
                    );
                }
                return Err(TitleGeneratorError::AiCallFailed(err.message));
            }
            _ => {}
        }
    }

    if let Err(stop_err) = runner.stop(&handle).await {
        tracing::debug!(
            error = %stop_err,
            "Failed to stop Codex app-server after title generation"
        );
    }
    parse_json_response(&response)
}

fn parse_json_response(response: &str) -> Result<GeneratedMetadata, TitleGeneratorError> {
    // Extract JSON from response (may have markdown or extra text)
    let json_start = response.find('{').ok_or_else(|| {
        TitleGeneratorError::ParseError("No JSON object found in response".into())
    })?;
    let json_end = response.rfind('}').ok_or_else(|| {
        TitleGeneratorError::ParseError("No JSON object found in response".into())
    })?;

    let json_str = &response[json_start..=json_end];

    let metadata: GeneratedMetadata = serde_json::from_str(json_str)
        .map_err(|e| TitleGeneratorError::ParseError(format!("Failed to parse JSON: {}", e)))?;

    // Validate that title and branch_suffix are non-empty
    if metadata.title.trim().is_empty() {
        return Err(TitleGeneratorError::ParseError(
            "Empty title from AI".to_string(),
        ));
    }
    if metadata.branch_suffix.trim().is_empty() {
        return Err(TitleGeneratorError::ParseError(
            "Empty branch_suffix from AI".to_string(),
        ));
    }

    Ok(metadata)
}

/// UTF-8 safe message truncation that respects character boundaries
fn truncate_message(msg: &str, max_chars: usize) -> String {
    let char_count = msg.chars().count();
    if char_count <= max_chars {
        msg.to_string()
    } else {
        let truncated: String = msg.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// Sanitize a branch suffix for use in git branch names
///
/// - Converts to lowercase
/// - Keeps only ASCII alphanumeric characters (non-ASCII removed, not replaced)
/// - Replaces non-alphanumeric with hyphens
/// - Collapses consecutive hyphens
/// - Removes leading/trailing hyphens
/// - Limits to 30 characters
/// - Returns "task" as fallback if result is empty
pub fn sanitize_branch_suffix(input: &str) -> String {
    let sanitized: String = input
        .to_lowercase()
        .chars()
        // Only keep ASCII characters, replace non-alphanumeric with hyphen
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c)
            } else if c.is_ascii() {
                Some('-') // Non-alphanumeric ASCII becomes hyphen
            } else {
                None // Non-ASCII dropped entirely
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // Take up to 30 chars, ensuring we don't cut mid-word if possible
    let result: String = sanitized.chars().take(30).collect();
    let result = result.trim_end_matches('-').to_string();

    // Fallback if empty
    if result.is_empty() {
        "task".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that Claude CLI works without the invalid --max-tokens option.
    /// This test verifies the fix for the bug where we were using --max-tokens
    /// which is not a valid Claude CLI option.
    #[tokio::test]
    #[ignore = "requires claude CLI binary - run with --ignored to verify fix"]
    async fn test_call_claude_without_max_tokens() {
        use std::process::Stdio;
        use tokio::process::Command;

        // Find claude binary (same logic as ToolAvailability)
        let claude_path = which::which("claude").expect("claude CLI not found in PATH");

        // Test with the FIXED arguments (no --max-tokens)
        let mut cmd = Command::new(&claude_path);
        cmd.args(["-p", "--output-format", "text", "--model", "sonnet"]);
        cmd.arg("--")
            .arg("Reply with just: {\"title\": \"test\", \"branch_suffix\": \"test\"}");
        cmd.current_dir(std::env::current_dir().unwrap());
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().await.expect("Failed to run claude");

        // This should succeed now that --max-tokens is removed
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "Claude CLI should succeed without --max-tokens. stderr: {}",
            stderr
        );

        // Also verify the old buggy command still fails (demonstrates the bug existed)
        let mut buggy_cmd = Command::new(&claude_path);
        buggy_cmd.args([
            "-p",
            "--output-format",
            "text",
            "--model",
            "sonnet",
            "--max-tokens", // This was the bug
            "200",
        ]);
        buggy_cmd.arg("--").arg("test");
        buggy_cmd.current_dir(std::env::current_dir().unwrap());
        buggy_cmd.stdin(Stdio::null());
        buggy_cmd.stdout(Stdio::piped());
        buggy_cmd.stderr(Stdio::piped());

        let buggy_output = buggy_cmd.output().await.expect("Failed to run claude");
        assert!(
            !buggy_output.status.success(),
            "Buggy command with --max-tokens should fail"
        );
        let buggy_stderr = String::from_utf8_lossy(&buggy_output.stderr);
        assert!(
            buggy_stderr.contains("unknown option '--max-tokens'"),
            "Buggy command should fail with 'unknown option' error, got: {}",
            buggy_stderr
        );
    }

    /// Test that TitleGeneratorError::AiCallFailed produces the correct error message
    /// that will be displayed in the chat warning.
    #[test]
    fn test_title_generator_error_display_for_warning() {
        let error = TitleGeneratorError::AiCallFailed(
            "Claude process failed: error: unknown option '--max-tokens'".to_string(),
        );

        // This is the format that will be displayed in the chat
        let warning_message = format!("⚠️ Failed to generate session title: {}", error);

        // Verify the error message contains the critical information
        assert!(
            warning_message.contains("Failed to generate session title"),
            "Warning should explain the failure"
        );
        assert!(
            warning_message.contains("--max-tokens"),
            "Warning should contain the specific error about --max-tokens"
        );
        assert!(
            warning_message.contains("⚠️"),
            "Warning should have warning emoji prefix"
        );
    }

    #[test]
    fn test_sanitize_branch_suffix() {
        assert_eq!(sanitize_branch_suffix("hello world"), "hello-world");
        assert_eq!(sanitize_branch_suffix("Hello_World"), "hello-world");
        assert_eq!(
            sanitize_branch_suffix("Add user authentication"),
            "add-user-authentication"
        );
        assert_eq!(
            sanitize_branch_suffix("fix: bug in login"),
            "fix-bug-in-login"
        );
        assert_eq!(
            sanitize_branch_suffix("  multiple   spaces  "),
            "multiple-spaces"
        );
    }

    #[test]
    fn test_sanitize_branch_suffix_non_ascii() {
        // Non-ASCII characters should be dropped, not converted to hyphens
        assert_eq!(sanitize_branch_suffix("héllo wörld"), "hllo-wrld");
        assert_eq!(sanitize_branch_suffix("日本語テスト"), "task"); // All non-ASCII -> empty -> fallback
        assert_eq!(sanitize_branch_suffix("café-fix"), "caf-fix");
    }

    #[test]
    fn test_sanitize_branch_suffix_empty_fallback() {
        assert_eq!(sanitize_branch_suffix(""), "task");
        assert_eq!(sanitize_branch_suffix("   "), "task");
        assert_eq!(sanitize_branch_suffix("---"), "task");
    }

    #[test]
    fn test_sanitize_branch_suffix_length_limit() {
        let long_input = "this-is-a-very-long-branch-name-that-exceeds-limit";
        let result = sanitize_branch_suffix(long_input);
        assert!(result.len() <= 30);
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn test_truncate_message() {
        assert_eq!(truncate_message("hello", 10), "hello");
        assert_eq!(truncate_message("hello world", 5), "hello...");
    }

    #[test]
    fn test_truncate_message_utf8() {
        // UTF-8 safe: should count chars, not bytes
        assert_eq!(truncate_message("héllo", 10), "héllo");
        assert_eq!(truncate_message("héllo wörld", 5), "héllo...");
        // Japanese text - should truncate by character count
        let japanese = "日本語テスト";
        assert_eq!(truncate_message(japanese, 3), "日本語...");
    }

    #[test]
    fn test_parse_json_response() {
        let response = r#"{"title": "Add login", "branch_suffix": "add-login"}"#;
        let result = parse_json_response(response).unwrap();
        assert_eq!(result.title, "Add login");
        assert_eq!(result.branch_suffix, "add-login");

        // With extra text
        let response = r#"Here's the JSON:
{"title": "Fix bug", "branch_suffix": "fix-bug"}
Done!"#;
        let result = parse_json_response(response).unwrap();
        assert_eq!(result.title, "Fix bug");
        assert_eq!(result.branch_suffix, "fix-bug");
    }

    #[test]
    fn test_parse_json_response_empty_title() {
        let response = r#"{"title": "", "branch_suffix": "add-login"}"#;
        let result = parse_json_response(response);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Empty title from AI"),
            "Expected error about empty title, got: {}",
            err
        );
    }

    #[test]
    fn test_parse_json_response_whitespace_title() {
        let response = r#"{"title": "   ", "branch_suffix": "add-login"}"#;
        let result = parse_json_response(response);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Empty title from AI"),
            "Expected error about empty title, got: {}",
            err
        );
    }

    #[test]
    fn test_parse_json_response_empty_branch_suffix() {
        let response = r#"{"title": "Add login", "branch_suffix": ""}"#;
        let result = parse_json_response(response);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Empty branch_suffix from AI"),
            "Expected error about empty branch_suffix, got: {}",
            err
        );
    }

    #[test]
    fn test_timeout_error_display() {
        let error = TitleGeneratorError::Timeout(10);
        let msg = error.to_string();
        assert!(
            msg.contains("10 seconds"),
            "Timeout error should show duration: {}",
            msg
        );
    }
}
