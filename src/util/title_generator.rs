//! Session title and branch name generation using AI
//!
//! This module handles generating descriptive session titles and branch names
//! from the first user message in a session using Claude or Codex.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

use super::{Tool, ToolAvailability};

/// Result of title/branch generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedMetadata {
    /// One-line session title/description
    pub title: String,
    /// Short branch name suffix (kebab-case, no slashes)
    pub branch_suffix: String,
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
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Generate a session title and branch name from the first user message
pub async fn generate_title_and_branch(
    tools: &ToolAvailability,
    user_message: &str,
    working_dir: &PathBuf,
) -> Result<GeneratedMetadata, TitleGeneratorError> {
    // Prefer Claude (sonnet), fall back to Codex
    let (is_claude, tool_path) = if tools.is_available(Tool::Claude) {
        (true, tools.get_path(Tool::Claude).unwrap().clone())
    } else if tools.is_available(Tool::Codex) {
        (false, tools.get_path(Tool::Codex).unwrap().clone())
    } else {
        return Err(TitleGeneratorError::NoToolAvailable);
    };

    let prompt = format!(
        r#"Based on this user request, generate:
1. A concise one-line title (max 50 chars) describing the task
2. A short branch name suffix (3-4 words, kebab-case, no slashes, max 30 chars)

User request: "{}"

Respond ONLY with valid JSON (no markdown, no explanation):
{{"title": "...", "branch_suffix": "..."}}"#,
        truncate_message(user_message, 500)
    );

    if is_claude {
        call_claude(&tool_path, &prompt, working_dir).await
    } else {
        call_codex(&tool_path, &prompt, working_dir).await
    }
}

async fn call_claude(
    binary_path: &PathBuf,
    prompt: &str,
    working_dir: &PathBuf,
) -> Result<GeneratedMetadata, TitleGeneratorError> {
    let mut cmd = Command::new(binary_path);
    cmd.args([
        "-p",
        "--output-format",
        "text",
        "--model",
        "sonnet",
        "--max-tokens",
        "200",
    ]);
    cmd.arg("--").arg(prompt);
    cmd.current_dir(working_dir);
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
    let mut cmd = Command::new(binary_path);
    // Codex CLI uses different flags
    cmd.args(["--quiet", "--approval-mode", "full-auto"]);
    cmd.arg(prompt);
    cmd.current_dir(working_dir);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TitleGeneratorError::AiCallFailed(format!(
            "Codex process failed: {}",
            stderr
        )));
    }

    let response = String::from_utf8_lossy(&output.stdout);
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

    serde_json::from_str::<GeneratedMetadata>(json_str)
        .map_err(|e| TitleGeneratorError::ParseError(format!("Failed to parse JSON: {}", e)))
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
}
