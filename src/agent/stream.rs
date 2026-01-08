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
                    // We don't have tool_use_id from this path, so this is a fallback
                    results.push((String::new(), stdout.clone(), false));
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
