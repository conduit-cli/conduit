use serde::{Deserialize, Serialize};

use crate::agent::session::SessionId;

/// Unified event type emitted by all agents
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// Session initialized with session ID
    SessionInit(SessionInitEvent),

    /// Turn/task started
    TurnStarted,

    /// Turn/task completed
    TurnCompleted(TurnCompletedEvent),

    /// Turn failed with error
    TurnFailed(TurnFailedEvent),

    /// Assistant text message
    AssistantMessage(AssistantMessageEvent),

    /// Assistant reasoning/thinking
    AssistantReasoning(ReasoningEvent),

    /// Tool use started
    ToolStarted(ToolStartedEvent),

    /// Tool use completed
    ToolCompleted(ToolCompletedEvent),

    /// Control request (permission prompt) from agent runtime
    ControlRequest(ControlRequestEvent),

    /// File operation
    FileChanged(FileChangedEvent),

    /// Command execution output
    CommandOutput(CommandOutputEvent),

    /// Token usage update
    TokenUsage(TokenUsageEvent),

    /// Context compaction triggered
    ContextCompaction(ContextCompactionEvent),

    /// Error event
    Error(ErrorEvent),

    /// Raw/unknown event (for forward compatibility)
    Raw { data: serde_json::Value },
}

impl AgentEvent {
    /// Get a human-readable event type name for display
    pub fn event_type_name(&self) -> &'static str {
        match self {
            AgentEvent::SessionInit(_) => "SessionInit",
            AgentEvent::TurnStarted => "TurnStarted",
            AgentEvent::TurnCompleted(_) => "TurnCompleted",
            AgentEvent::TurnFailed(_) => "TurnFailed",
            AgentEvent::AssistantMessage(_) => "AssistantMessage",
            AgentEvent::AssistantReasoning(_) => "AssistantReasoning",
            AgentEvent::ToolStarted(_) => "ToolStarted",
            AgentEvent::ToolCompleted(_) => "ToolCompleted",
            AgentEvent::ControlRequest(_) => "ControlRequest",
            AgentEvent::FileChanged(_) => "FileChanged",
            AgentEvent::CommandOutput(_) => "CommandOutput",
            AgentEvent::TokenUsage(_) => "TokenUsage",
            AgentEvent::ContextCompaction(_) => "ContextCompaction",
            AgentEvent::Error(_) => "Error",
            AgentEvent::Raw { .. } => "Raw",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInitEvent {
    pub session_id: SessionId,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCompletedEvent {
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnFailedEvent {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessageEvent {
    pub text: String,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningEvent {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStartedEvent {
    pub tool_name: String,
    pub tool_id: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCompletedEvent {
    pub tool_id: String,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlRequestEvent {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: Option<String>,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedEvent {
    pub path: String,
    pub operation: FileOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOperation {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutputEvent {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub is_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageEvent {
    pub usage: TokenUsage,
    pub context_window: Option<i64>,
    pub usage_percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactionEvent {
    pub reason: String,
    pub tokens_before: i64,
    pub tokens_after: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub message: String,
    pub is_fatal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ============================================================================
// AskUserQuestion and ExitPlanMode data structures
// ============================================================================

/// A single question in an AskUserQuestion tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestion {
    /// Short label for the question (max 12 chars), used in tab bar
    #[serde(default)]
    pub header: String,
    /// The full question text
    pub question: String,
    /// Available options to choose from
    pub options: Vec<QuestionOption>,
    /// Whether multiple options can be selected
    #[serde(default, rename = "multiSelect")]
    pub multi_select: bool,
}

/// An option within a UserQuestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    /// The display label for this option
    pub label: String,
    /// Description explaining what this option means
    #[serde(default)]
    pub description: String,
}

/// Parsed data from AskUserQuestion tool call
#[derive(Debug, Clone)]
pub struct AskUserQuestionData {
    pub tool_id: String,
    pub questions: Vec<UserQuestion>,
}

/// Parsed data from ExitPlanMode tool call
#[derive(Debug, Clone)]
pub struct ExitPlanModeData {
    pub tool_id: String,
    pub plan_file_path: Option<String>,
}

/// Warning levels for context window usage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextWarningLevel {
    /// Under 80% - normal operation
    #[default]
    Normal,
    /// 80-89% - approaching limit
    Medium,
    /// 90-94% - high usage, compaction likely soon
    High,
    /// 95%+ - critical, compaction imminent
    Critical,
}

/// Context window state for tracking usage against limits
#[derive(Debug, Clone, Default)]
pub struct ContextWindowState {
    /// Current context usage (total tokens in context)
    pub current_tokens: i64,
    /// Maximum context window size for this model
    pub max_tokens: i64,
    /// Whether context has been compacted in this session
    pub has_compacted: bool,
    /// Number of compactions in this session
    pub compaction_count: u32,
    /// Last compaction event details (if any)
    pub last_compaction: Option<ContextCompactionEvent>,
}

impl ContextWindowState {
    /// Create new state with a given max context
    pub fn new(max_tokens: i64) -> Self {
        Self {
            current_tokens: 0,
            max_tokens,
            has_compacted: false,
            compaction_count: 0,
            last_compaction: None,
        }
    }

    /// Calculate usage percentage (0.0 to 1.0+)
    pub fn usage_percent(&self) -> f32 {
        if self.max_tokens <= 0 {
            return 0.0;
        }
        self.current_tokens as f32 / self.max_tokens as f32
    }

    /// Get warning level based on usage
    pub fn warning_level(&self) -> ContextWarningLevel {
        let pct = self.usage_percent();
        if pct >= 0.95 {
            ContextWarningLevel::Critical
        } else if pct >= 0.90 {
            ContextWarningLevel::High
        } else if pct >= 0.80 {
            ContextWarningLevel::Medium
        } else {
            ContextWarningLevel::Normal
        }
    }

    /// Update from TokenUsageEvent
    pub fn update_from_usage(&mut self, event: &TokenUsageEvent) {
        // Use total_tokens from usage as current context size
        self.current_tokens = event.usage.total_tokens;

        // Override max if provided by agent
        if let Some(window) = event.context_window {
            self.max_tokens = window;
        }

        // Update usage percent if provided and we can derive context window
        if let (Some(pct), 0) = (event.usage_percent, self.max_tokens) {
            // If we have percent but no max, try to derive max from current and percent
            if pct > 0.0 {
                self.max_tokens = (self.current_tokens as f32 / pct) as i64;
            }
        }
    }

    /// Record a compaction event
    pub fn record_compaction(&mut self, event: ContextCompactionEvent) {
        self.current_tokens = event.tokens_after;
        self.has_compacted = true;
        self.compaction_count += 1;
        self.last_compaction = Some(event);
    }

    /// Format tokens for display (e.g., "150k", "1.2M")
    pub fn format_tokens(tokens: i64) -> String {
        if tokens >= 1_000_000 {
            format!("{:.1}M", tokens as f64 / 1_000_000.0)
        } else if tokens >= 1_000 {
            format!("{:.0}k", tokens as f64 / 1_000.0)
        } else {
            format!("{}", tokens)
        }
    }
}
