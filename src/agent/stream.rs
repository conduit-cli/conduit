use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdout;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;

/// Generic JSONL stream parser
pub struct JsonlStreamParser;

impl JsonlStreamParser {
    /// Parse JSONL from stdout and send to channel
    pub async fn parse_stream<T>(stdout: ChildStdout, tx: mpsc::Sender<T>) -> Result<(), AgentError>
    where
        T: for<'de> Deserialize<'de> + Send + 'static,
    {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            // Log raw JSONL lines at trace level for debugging
            tracing::trace!("JSONL raw line: {}", &line);

            match serde_json::from_str::<T>(&line) {
                Ok(event) => {
                    if tx.send(event).await.is_err() {
                        // Receiver dropped, exit gracefully
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse JSONL line: {e}. Line: {line}");
                    // Continue processing - don't fail on single parse error
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Claude Code specific JSONL events (raw from CLI)
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeRawEvent {
    #[serde(rename = "system")]
    System(ClaudeSystemEvent),

    #[serde(rename = "assistant")]
    Assistant(ClaudeAssistantEvent),

    #[serde(rename = "tool_use")]
    ToolUse(ClaudeToolUseEvent),

    #[serde(rename = "tool_result")]
    ToolResult(ClaudeToolResultEvent),

    /// User events contain tool results from Claude Code CLI
    #[serde(rename = "user")]
    User(ClaudeUserEvent),

    /// Control requests (permission checks, hooks) from Claude Code CLI
    #[serde(rename = "control_request")]
    ControlRequest(ClaudeControlRequestEvent),

    #[serde(rename = "result")]
    Result(ClaudeResultEvent),

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeSystemEvent {
    pub subtype: Option<String>,
    pub session_id: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeAssistantEvent {
    /// Nested message object (new format)
    pub message: Option<ClaudeMessageObject>,
    /// Direct text (older format)
    pub text: Option<String>,
    /// Session ID
    pub session_id: Option<String>,
    /// Error type (e.g., "authentication_failed")
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeMessageObject {
    pub model: Option<String>,
    pub id: Option<String>,
    pub role: Option<String>,
    pub content: Option<Vec<ClaudeContentBlock>>,
    pub stop_reason: Option<String>,
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

/// Extracted tool use information from a content block
#[derive(Debug, Clone)]
pub struct ExtractedToolUse {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

impl ClaudeAssistantEvent {
    /// Extract the text content from this event
    pub fn extract_text(&self) -> Option<String> {
        // Try new format first (message.content[].text)
        if let Some(ref msg) = self.message {
            if let Some(ref content) = msg.content {
                let texts: Vec<String> = content
                    .iter()
                    .filter_map(|block| match block {
                        ClaudeContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect();
                if !texts.is_empty() {
                    return Some(texts.join("\n"));
                }
            }
        }
        // Fall back to direct text field
        self.text.clone()
    }

    /// Extract tool_use blocks from this event's content
    pub fn extract_tool_uses(&self) -> Vec<ExtractedToolUse> {
        if let Some(ref msg) = self.message {
            if let Some(ref content) = msg.content {
                return content
                    .iter()
                    .filter_map(|block| match block {
                        ClaudeContentBlock::ToolUse { id, name, input } => Some(ExtractedToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        }),
                        _ => None,
                    })
                    .collect();
            }
        }
        Vec::new()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeToolUseEvent {
    pub tool: Option<String>,
    pub name: Option<String>,
    pub id: Option<String>,
    #[serde(default)]
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeToolResultEvent {
    pub tool_use_id: Option<String>,
    pub content: Option<String>,
    pub is_error: Option<bool>,
}

/// User event that contains tool results in Claude Code CLI output
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeUserEvent {
    pub message: Option<ClaudeUserMessage>,
    pub tool_use_result: Option<ClaudeToolUseResultData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeControlRequestEvent {
    pub request_id: String,
    pub request: ClaudeControlRequestType,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ClaudeControlRequestType {
    CanUseTool {
        tool_name: String,
        input: serde_json::Value,
        #[serde(default)]
        tool_use_id: Option<String>,
    },
    HookCallback {
        callback_id: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeUserMessage {
    pub role: Option<String>,
    pub content: Option<Vec<ClaudeUserContentBlock>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeUserContentBlock {
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

/// Additional tool result data from Claude Code CLI
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeToolUseResultData {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub interrupted: Option<bool>,
    #[serde(rename = "isImage")]
    pub is_image: Option<bool>,
}

impl ClaudeUserEvent {
    /// Extract tool results from this user event
    pub fn extract_tool_results(&self) -> Vec<(String, String, bool)> {
        let mut results = Vec::new();

        if let Some(ref msg) = self.message {
            if let Some(ref content) = msg.content {
                for block in content {
                    if let ClaudeUserContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = block
                    {
                        // Content can be a string or array of text blocks
                        let content_str = match content {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(arr) => arr
                                .iter()
                                .filter_map(|v| {
                                    v.get("text").and_then(|t| t.as_str()).map(String::from)
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                            _ => String::new(),
                        };
                        results.push((tool_use_id.clone(), content_str, *is_error));
                    }
                }
            }
        }

        // Also check tool_use_result for stdout/stderr
        if results.is_empty() {
            if let Some(ref tur) = self.tool_use_result {
                if let Some(ref stdout) = tur.stdout {
                    // Fallback path: tool_use_result exists but no tool_use_id available
                    // Use sentinel value to avoid downstream correlation issues with empty string
                    tracing::warn!(
                        "Tool result missing tool_use_id, using sentinel. stdout_len={}, stderr={:?}, is_image={:?}",
                        stdout.len(),
                        tur.stderr.as_ref().map(|s| s.len()),
                        tur.is_image
                    );
                    results.push(("unknown_tool_use_id".to_string(), stdout.clone(), false));
                }
            }
        }

        results
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeResultEvent {
    pub result: Option<String>,
    pub output: Option<String>,
    pub is_error: Option<bool>,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

// ============================================================================
// Codex CLI specific JSONL events (raw from CLI)
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum CodexRawEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted { thread_id: String },

    #[serde(rename = "turn.started")]
    TurnStarted,

    #[serde(rename = "turn.completed")]
    TurnCompleted { usage: CodexUsage },

    #[serde(rename = "turn.failed")]
    TurnFailed { error: CodexErrorInfo },

    #[serde(rename = "item.started")]
    ItemStarted { item: CodexThreadItem },

    #[serde(rename = "item.updated")]
    ItemUpdated { item: CodexThreadItem },

    #[serde(rename = "item.completed")]
    ItemCompleted { item: CodexThreadItem },

    #[serde(rename = "error")]
    Error { message: String },

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexUsage {
    pub input_tokens: i64,
    #[serde(default)]
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexErrorInfo {
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexThreadItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    #[serde(flatten)]
    pub details: serde_json::Value,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test parsing of authentication failure JSONL sequence.
    /// When Claude CLI returns an auth error, we receive:
    /// 1. system init event
    /// 2. assistant event with error field and error text
    /// 3. result event with is_error: true
    #[test]
    fn test_parse_auth_failure_system_init() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/home/fcoury/.conduit/workspaces/conduit/old-fox","session_id":"50884eed-28b7-431e-9ad8-78b326696ae7","tools":["Task","TaskOutput","Bash","Glob","Grep","ExitPlanMode","Read","Edit","Write","NotebookEdit","WebFetch","TodoWrite","WebSearch","KillShell","AskUserQuestion","Skill","EnterPlanMode"],"mcp_servers":[],"model":"claude-sonnet-4-5-20250929","permissionMode":"default","slash_commands":["compact","context","cost","init","pr-comments","release-notes","review","security-review"],"apiKeySource":"none","claude_code_version":"2.1.2","output_style":"default","agents":["Bash","general-purpose","statusline-setup","Explore","Plan"],"skills":[],"plugins":[],"uuid":"db0dbd6c-b06a-4009-8d9d-412a46693eed"}"#;

        let event: ClaudeRawEvent =
            serde_json::from_str(line).expect("Failed to parse system init");

        match event {
            ClaudeRawEvent::System(sys) => {
                assert_eq!(sys.subtype, Some("init".to_string()));
                assert_eq!(
                    sys.session_id,
                    Some("50884eed-28b7-431e-9ad8-78b326696ae7".to_string())
                );
                assert_eq!(sys.model, Some("claude-sonnet-4-5-20250929".to_string()));
            }
            other => panic!("Expected System event, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_auth_failure_assistant_event() {
        let line = r#"{"type":"assistant","message":{"id":"029c1c0f-6927-4a48-aae1-21a3e895456f","container":null,"model":"<synthetic>","role":"assistant","stop_reason":"stop_sequence","stop_sequence":"","type":"message","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":null,"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0}},"content":[{"type":"text","text":"Invalid API key · Please run /login"}],"context_management":null},"parent_tool_use_id":null,"session_id":"50884eed-28b7-431e-9ad8-78b326696ae7","uuid":"088cb8d8-a2b6-4633-860d-d2bb5562bfe2","error":"authentication_failed"}"#;

        let event: ClaudeRawEvent =
            serde_json::from_str(line).expect("Failed to parse assistant event");

        match event {
            ClaudeRawEvent::Assistant(assistant) => {
                // Verify the text extraction works
                let text = assistant.extract_text();
                assert_eq!(
                    text,
                    Some("Invalid API key · Please run /login".to_string())
                );

                // Verify session_id is captured
                assert_eq!(
                    assistant.session_id,
                    Some("50884eed-28b7-431e-9ad8-78b326696ae7".to_string())
                );

                assert_eq!(assistant.error, Some("authentication_failed".to_string()));
            }
            other => panic!("Expected Assistant event, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_auth_failure_result_event() {
        let line = r#"{"type":"result","subtype":"success","is_error":true,"duration_ms":262,"duration_api_ms":0,"num_turns":1,"result":"Invalid API key · Please run /login","session_id":"50884eed-28b7-431e-9ad8-78b326696ae7","total_cost_usd":0,"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":"standard","cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0}},"modelUsage":{},"permission_denials":[],"uuid":"88523c63-2f50-4d6c-ba86-fb32c6745527"}"#;

        let event: ClaudeRawEvent =
            serde_json::from_str(line).expect("Failed to parse result event");

        match event {
            ClaudeRawEvent::Result(result) => {
                assert_eq!(
                    result.result,
                    Some("Invalid API key · Please run /login".to_string())
                );
                assert_eq!(
                    result.session_id,
                    Some("50884eed-28b7-431e-9ad8-78b326696ae7".to_string())
                );
                assert_eq!(result.is_error, Some(true));
                // Verify usage is parsed (even with zero values)
                assert!(result.usage.is_some());
                let usage = result.usage.unwrap();
                assert_eq!(usage.input_tokens, Some(0));
                assert_eq!(usage.output_tokens, Some(0));
            }
            other => panic!("Expected Result event, got {:?}", other),
        }
    }

    /// Test that the full auth failure sequence parses correctly
    #[test]
    fn test_parse_auth_failure_full_sequence() {
        let lines = [
            r#"{"type":"system","subtype":"init","cwd":"/home/fcoury/.conduit/workspaces/conduit/old-fox","session_id":"50884eed-28b7-431e-9ad8-78b326696ae7","tools":["Task"],"mcp_servers":[],"model":"claude-sonnet-4-5-20250929","permissionMode":"default","slash_commands":[],"apiKeySource":"none","claude_code_version":"2.1.2","output_style":"default","agents":[],"skills":[],"plugins":[],"uuid":"db0dbd6c-b06a-4009-8d9d-412a46693eed"}"#,
            r#"{"type":"assistant","message":{"id":"029c1c0f-6927-4a48-aae1-21a3e895456f","container":null,"model":"<synthetic>","role":"assistant","stop_reason":"stop_sequence","stop_sequence":"","type":"message","usage":{"input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"Invalid API key · Please run /login"}],"context_management":null},"parent_tool_use_id":null,"session_id":"50884eed-28b7-431e-9ad8-78b326696ae7","uuid":"088cb8d8-a2b6-4633-860d-d2bb5562bfe2","error":"authentication_failed"}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"duration_ms":262,"duration_api_ms":0,"num_turns":1,"result":"Invalid API key · Please run /login","session_id":"50884eed-28b7-431e-9ad8-78b326696ae7","total_cost_usd":0,"usage":{"input_tokens":0,"output_tokens":0},"modelUsage":{},"permission_denials":[],"uuid":"88523c63-2f50-4d6c-ba86-fb32c6745527"}"#,
        ];

        let events: Vec<ClaudeRawEvent> = lines
            .iter()
            .map(|line| serde_json::from_str(line).expect("Failed to parse line"))
            .collect();

        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], ClaudeRawEvent::System(_)));
        assert!(matches!(events[1], ClaudeRawEvent::Assistant(_)));
        assert!(matches!(events[2], ClaudeRawEvent::Result(_)));
    }
}
