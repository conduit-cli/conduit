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
}

impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "codex" => AgentType::Codex,
            _ => AgentType::Claude,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AgentType::Claude => "Claude Code",
            AgentType::Codex => "Codex CLI",
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
}

/// Handle to a running agent process
pub struct AgentHandle {
    /// Receiver for agent events
    pub events: mpsc::Receiver<AgentEvent>,
    /// Current session ID (may be set after init event)
    pub session_id: Option<SessionId>,
    /// Process ID for monitoring
    pub pid: u32,
}

impl AgentHandle {
    pub fn new(events: mpsc::Receiver<AgentEvent>, pid: u32) -> Self {
        Self {
            events,
            session_id: None,
            pid,
        }
    }

    pub fn set_session_id(&mut self, session_id: SessionId) {
        self.session_id = Some(session_id);
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
