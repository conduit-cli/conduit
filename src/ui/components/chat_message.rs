//! Chat message types and helpers.

use super::TurnSummary;

/// Role of a chat message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
    Error,
    Summary,
}

/// A single chat message
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub is_streaming: bool,
    /// Pre-rendered summary (for Summary role)
    pub summary: Option<TurnSummary>,
    /// Whether this tool message is collapsed (only for Tool role)
    pub is_collapsed: bool,
    /// Exit code for tool execution (e.g., shell commands)
    pub exit_code: Option<i32>,
    /// Cached file size for Read tool on images (avoids fs lookup on session restore)
    pub file_size: Option<u64>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_name: None,
            tool_args: None,
            is_streaming: false,
            summary: None,
            is_collapsed: false,
            exit_code: None,
            file_size: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_name: None,
            tool_args: None,
            is_streaming: false,
            summary: None,
            is_collapsed: false,
            exit_code: None,
            file_size: None,
        }
    }

    pub fn tool(
        name: impl Into<String>,
        args: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_name: Some(name.into()),
            tool_args: Some(args.into()),
            is_streaming: false,
            summary: None,
            is_collapsed: false, // Default to expanded
            exit_code: None,
            file_size: None,
        }
    }

    /// Create a tool message with exit code
    pub fn tool_with_exit(
        name: impl Into<String>,
        args: impl Into<String>,
        content: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_name: Some(name.into()),
            tool_args: Some(args.into()),
            is_streaming: false,
            summary: None,
            is_collapsed: false,
            exit_code,
            file_size: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            tool_name: None,
            tool_args: None,
            is_streaming: false,
            summary: None,
            is_collapsed: false,
            exit_code: None,
            file_size: None,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Error,
            content: content.into(),
            tool_name: None,
            tool_args: None,
            is_streaming: false,
            summary: None,
            is_collapsed: false,
            exit_code: None,
            file_size: None,
        }
    }

    pub fn streaming(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_name: None,
            tool_args: None,
            is_streaming: true,
            summary: None,
            is_collapsed: false,
            exit_code: None,
            file_size: None,
        }
    }

    pub fn turn_summary(summary: TurnSummary) -> Self {
        Self {
            role: MessageRole::Summary,
            content: String::new(),
            tool_name: None,
            tool_args: None,
            is_streaming: false,
            summary: Some(summary),
            is_collapsed: false,
            exit_code: None,
            file_size: None,
        }
    }

    /// Toggle collapsed state for tool messages
    pub fn toggle_collapsed(&mut self) {
        if self.role == MessageRole::Tool {
            self.is_collapsed = !self.is_collapsed;
        }
    }
}
