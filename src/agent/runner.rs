use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;
use crate::agent::events::AgentEvent;
use crate::agent::session::SessionId;

/// Agent type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentType {
    Claude,
    Codex,
    Gemini,
}

/// Agent mode (Build vs Plan)
///
/// Build mode (default): Claude can read, write, and execute commands
/// Plan mode: Claude can only read and analyze, no modifications allowed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AgentMode {
    #[default]
    Build,
    Plan,
}

impl AgentMode {
    /// Convert to Claude's --permission-mode argument value
    pub fn as_permission_mode(&self) -> &'static str {
        match self {
            AgentMode::Build => "default",
            AgentMode::Plan => "plan",
        }
    }

    /// Display name for the UI
    pub fn display_name(&self) -> &'static str {
        match self {
            AgentMode::Build => "Build",
            AgentMode::Plan => "Plan",
        }
    }

    /// String representation for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Build => "build",
            AgentMode::Plan => "plan",
        }
    }

    /// Parse from string
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plan" => AgentMode::Plan,
            _ => AgentMode::Build,
        }
    }

    /// Toggle between Build and Plan
    pub fn toggle(&self) -> Self {
        match self {
            AgentMode::Build => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Build,
        }
    }
}

impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
            AgentType::Gemini => "gemini",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "codex" => AgentType::Codex,
            "gemini" => AgentType::Gemini,
            _ => AgentType::Claude,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AgentType::Claude => "Claude Code",
            AgentType::Codex => "Codex CLI",
            AgentType::Gemini => "Gemini CLI",
        }
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Configuration for starting an agent
#[derive(Debug, Clone)]
pub struct AgentStartConfig {
    pub prompt: String,
    pub working_dir: PathBuf,
    pub allowed_tools: Vec<String>,
    pub resume_session: Option<SessionId>,
    pub timeout_ms: Option<u64>,
    pub additional_args: Vec<String>,
    /// Model to use (e.g., "sonnet", "opus" for Claude; "o4-mini" for Codex)
    pub model: Option<String>,
    /// Optional image paths to attach to the initial prompt
    pub images: Vec<PathBuf>,
    /// Agent mode (Build vs Plan) - only used by Claude
    pub agent_mode: AgentMode,
    /// Optional input format override (e.g. "stream-json" for Claude)
    pub input_format: Option<String>,
    /// Optional stdin payload for structured input (e.g. JSONL)
    pub stdin_payload: Option<String>,
}

impl AgentStartConfig {
    pub fn new(prompt: impl Into<String>, working_dir: PathBuf) -> Self {
        Self {
            prompt: prompt.into(),
            working_dir,
            allowed_tools: vec![],
            resume_session: None,
            timeout_ms: None,
            additional_args: vec![],
            model: None,
            images: Vec::new(),
            agent_mode: AgentMode::default(),
            input_format: None,
            stdin_payload: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    pub fn with_resume(mut self, session_id: SessionId) -> Self {
        self.resume_session = Some(session_id);
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_images(mut self, images: Vec<PathBuf>) -> Self {
        self.images = images;
        self
    }

    pub fn with_agent_mode(mut self, mode: AgentMode) -> Self {
        self.agent_mode = mode;
        self
    }

    pub fn with_input_format(mut self, format: impl Into<String>) -> Self {
        self.input_format = Some(format.into());
        self
    }

    pub fn with_stdin_payload(mut self, payload: impl Into<String>) -> Self {
        self.stdin_payload = Some(payload.into());
        self
    }
}

/// Handle to a running agent process
pub struct AgentHandle {
    /// Receiver for agent events
    pub events: mpsc::Receiver<AgentEvent>,
    /// Current session ID (may be set after init event)
    pub session_id: Option<SessionId>,
    /// Process ID for monitoring
    pub pid: u32,
    /// Optional input channel for streaming stdin payloads
    pub input_tx: Option<mpsc::Sender<String>>,
}

impl AgentHandle {
    pub fn new(
        events: mpsc::Receiver<AgentEvent>,
        pid: u32,
        input_tx: Option<mpsc::Sender<String>>,
    ) -> Self {
        Self {
            events,
            session_id: None,
            pid,
            input_tx,
        }
    }

    pub fn set_session_id(&mut self, session_id: SessionId) {
        self.session_id = Some(session_id);
    }

    pub fn take_input_sender(&mut self) -> Option<mpsc::Sender<String>> {
        self.input_tx.take()
    }
}

/// Trait for agent runners that can spawn and manage agent processes
#[async_trait]
pub trait AgentRunner: Send + Sync {
    /// Agent type identifier (e.g., "claude", "codex")
    fn agent_type(&self) -> AgentType;

    /// Start the agent with the given configuration
    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError>;

    /// Send input to a running agent (for interactive prompts)
    async fn send_input(&self, handle: &AgentHandle, input: &str) -> Result<(), AgentError>;

    /// Request graceful shutdown
    async fn stop(&self, handle: &AgentHandle) -> Result<(), AgentError>;

    /// Force kill the agent process
    async fn kill(&self, handle: &AgentHandle) -> Result<(), AgentError>;

    /// Check if the agent binary is available
    fn is_available(&self) -> bool;

    /// Get the path to the agent binary
    fn binary_path(&self) -> Option<PathBuf>;
}
