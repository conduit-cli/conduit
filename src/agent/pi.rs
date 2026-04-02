use std::path::PathBuf;

use crate::agent::error::AgentError;
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};
use async_trait::async_trait;

pub struct PiRunner {
    binary_path: PathBuf,
}

impl Default for PiRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl PiRunner {
    pub fn new() -> Self {
        Self {
            binary_path: Self::find_binary().unwrap_or_else(|| PathBuf::from("pi")),
        }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { binary_path: path }
    }

    fn find_binary() -> Option<PathBuf> {
        which::which("pi").ok()
    }
}

#[async_trait]
impl AgentRunner for PiRunner {
    fn agent_type(&self) -> AgentType {
        AgentType::Pi
    }

    async fn start(&self, _config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        Err(AgentError::NotSupported(
            "Pi runner is not implemented yet".to_string(),
        ))
    }

    async fn send_input(
        &self,
        _handle: &AgentHandle,
        _input: AgentInput,
    ) -> Result<(), AgentError> {
        Err(AgentError::NotSupported(
            "Pi runner is not implemented yet".to_string(),
        ))
    }

    async fn stop(&self, _handle: &AgentHandle) -> Result<(), AgentError> {
        Ok(())
    }

    async fn kill(&self, _handle: &AgentHandle) -> Result<(), AgentError> {
        Ok(())
    }

    fn is_available(&self) -> bool {
        self.binary_path.exists() || which::which(&self.binary_path).is_ok()
    }

    fn binary_path(&self) -> Option<PathBuf> {
        Some(self.binary_path.clone())
    }
}
