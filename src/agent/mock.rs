//! Mock agent runner for deterministic testing
//!
//! Implements AgentRunner trait to emit pre-configured events
//! without spawning real CLI processes. Use this for integration
//! and E2E tests that need to verify agent interaction flows.
//!
//! # Example
//! ```no_run
//! use conduit::agent::mock::{MockAgentRunner, MockConfig};
//! use conduit::agent::{AgentType, AgentEvent, SessionInitEvent, AgentStartConfig};
//! use conduit::agent::session::SessionId;
//!
//! #[tokio::test]
//! async fn test_agent_flow() {
//!     let events = vec![
//!         AgentEvent::SessionInit(SessionInitEvent {
//!             session_id: SessionId::from_string("test-001".into()),
//!             model: Some("mock-model".into()),
//!         }),
//!     ];
//!
//!     let runner = MockAgentRunner::new(AgentType::Claude)
//!         .with_config(MockConfig::default().with_events(events));
//!
//!     // Use runner in tests...
//! }
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::agent::error::AgentError;
use crate::agent::events::AgentEvent;
use crate::agent::runner::{AgentHandle, AgentInput, AgentRunner, AgentStartConfig, AgentType};

/// Type of error to simulate on start failure
#[derive(Clone, Debug)]
pub enum MockStartError {
    BinaryNotFound(String),
    ProcessSpawnFailed,
    Config(String),
    Timeout(u64),
}

impl MockStartError {
    fn into_agent_error(self) -> AgentError {
        match self {
            MockStartError::BinaryNotFound(msg) => AgentError::BinaryNotFound(msg),
            MockStartError::ProcessSpawnFailed => AgentError::ProcessSpawnFailed,
            MockStartError::Config(msg) => AgentError::Config(msg),
            MockStartError::Timeout(ms) => AgentError::Timeout(ms),
        }
    }
}

/// Configuration for mock agent behavior
#[derive(Clone, Default)]
pub struct MockConfig {
    /// Events to emit when started
    pub events: Vec<AgentEvent>,
    /// Delay between events (simulates streaming)
    pub event_delay: Duration,
    /// Whether start() should fail
    pub fail_on_start: bool,
    /// Error to return if failing
    pub start_error: Option<MockStartError>,
}

impl MockConfig {
    /// Configure events to emit when the agent is started
    pub fn with_events(mut self, events: Vec<AgentEvent>) -> Self {
        self.events = events;
        self
    }

    /// Configure delay between emitting events (default: Duration::ZERO)
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.event_delay = delay;
        self
    }

    /// Configure the mock to fail on start
    pub fn failing(mut self) -> Self {
        self.fail_on_start = true;
        self.start_error = Some(MockStartError::BinaryNotFound("mock-failure".into()));
        self
    }

    /// Configure the mock to fail with a specific error
    pub fn failing_with(mut self, error: MockStartError) -> Self {
        self.fail_on_start = true;
        self.start_error = Some(error);
        self
    }
}

/// Mock agent runner for testing
///
/// This runner implements `AgentRunner` but doesn't spawn any real processes.
/// Instead, it emits pre-configured events and captures all interactions
/// for later verification in tests.
pub struct MockAgentRunner {
    agent_type: AgentType,
    config: MockConfig,
    /// Captured start configs for verification
    captured_configs: Arc<Mutex<Vec<AgentStartConfig>>>,
    /// Captured inputs sent via send_input
    captured_inputs: Arc<Mutex<Vec<AgentInput>>>,
    /// Whether stop was called
    stop_called: Arc<Mutex<bool>>,
    /// Whether kill was called
    kill_called: Arc<Mutex<bool>>,
}

impl MockAgentRunner {
    /// Create a new mock runner for the specified agent type
    pub fn new(agent_type: AgentType) -> Self {
        Self {
            agent_type,
            config: MockConfig::default(),
            captured_configs: Arc::new(Mutex::new(Vec::new())),
            captured_inputs: Arc::new(Mutex::new(Vec::new())),
            stop_called: Arc::new(Mutex::new(false)),
            kill_called: Arc::new(Mutex::new(false)),
        }
    }

    /// Configure the mock with a MockConfig
    pub fn with_config(mut self, config: MockConfig) -> Self {
        self.config = config;
        self
    }

    /// Configure events to emit (convenience method)
    pub fn with_events(mut self, events: Vec<AgentEvent>) -> Self {
        self.config.events = events;
        self
    }

    /// Get captured start configurations for assertions
    pub fn captured_configs(&self) -> Vec<AgentStartConfig> {
        self.captured_configs.lock().clone()
    }

    /// Get the last captured config (most recent start call)
    pub fn last_config(&self) -> Option<AgentStartConfig> {
        self.captured_configs.lock().last().cloned()
    }

    /// Get captured inputs for assertions
    pub fn captured_inputs(&self) -> Vec<AgentInput> {
        self.captured_inputs.lock().clone()
    }

    /// Check if stop was called
    pub fn was_stopped(&self) -> bool {
        *self.stop_called.lock()
    }

    /// Check if kill was called
    pub fn was_killed(&self) -> bool {
        *self.kill_called.lock()
    }

    /// Reset all captured state
    pub fn reset(&self) {
        self.captured_configs.lock().clear();
        self.captured_inputs.lock().clear();
        *self.stop_called.lock() = false;
        *self.kill_called.lock() = false;
    }
}

#[async_trait]
impl AgentRunner for MockAgentRunner {
    fn agent_type(&self) -> AgentType {
        self.agent_type
    }

    async fn start(&self, config: AgentStartConfig) -> Result<AgentHandle, AgentError> {
        // Capture the config for later assertions
        self.captured_configs.lock().push(config);

        if self.config.fail_on_start {
            let mock_error = self
                .config
                .start_error
                .clone()
                .unwrap_or(MockStartError::ProcessSpawnFailed);
            return Err(mock_error.into_agent_error());
        }

        let (tx, rx) = mpsc::channel(32);
        let events = self.config.events.clone();
        let delay = self.config.event_delay;

        // Spawn task to emit pre-configured events
        tokio::spawn(async move {
            for event in events {
                // Small delay between events to simulate streaming
                if delay > Duration::ZERO {
                    tokio::time::sleep(delay).await;
                }

                if tx.send(event).await.is_err() {
                    break; // Receiver dropped
                }
            }
        });

        Ok(AgentHandle::new(rx, 99999, None)) // Fake PID
    }

    async fn send_input(&self, _handle: &AgentHandle, input: AgentInput) -> Result<(), AgentError> {
        self.captured_inputs.lock().push(input);
        Ok(())
    }

    async fn stop(&self, _handle: &AgentHandle) -> Result<(), AgentError> {
        *self.stop_called.lock() = true;
        Ok(())
    }

    async fn kill(&self, _handle: &AgentHandle) -> Result<(), AgentError> {
        *self.kill_called.lock() = true;
        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }

    fn binary_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/mock/agent"))
    }
}

/// Builder for creating mock agent event sequences
///
/// Provides a fluent API for building realistic event sequences
/// for testing different scenarios.
pub struct MockEventBuilder {
    events: Vec<AgentEvent>,
    session_id: String,
}

impl MockEventBuilder {
    /// Create a new builder with a session ID
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            events: Vec::new(),
            session_id: session_id.into(),
        }
    }

    /// Add a session init event
    pub fn session_init(mut self, model: Option<&str>) -> Self {
        use crate::agent::events::SessionInitEvent;
        use crate::agent::session::SessionId;

        self.events.push(AgentEvent::SessionInit(SessionInitEvent {
            session_id: SessionId::from_string(self.session_id.clone()),
            model: model.map(String::from),
        }));
        self
    }

    /// Add a turn started event
    pub fn turn_started(mut self) -> Self {
        self.events.push(AgentEvent::TurnStarted);
        self
    }

    /// Add an assistant message
    pub fn assistant_message(mut self, text: &str, is_final: bool) -> Self {
        use crate::agent::events::AssistantMessageEvent;

        self.events
            .push(AgentEvent::AssistantMessage(AssistantMessageEvent {
                text: text.to_string(),
                is_final,
            }));
        self
    }

    /// Add a tool started event
    pub fn tool_started(mut self, tool_name: &str, tool_id: &str, args: serde_json::Value) -> Self {
        use crate::agent::events::ToolStartedEvent;

        self.events.push(AgentEvent::ToolStarted(ToolStartedEvent {
            tool_name: tool_name.to_string(),
            tool_id: tool_id.to_string(),
            arguments: args,
        }));
        self
    }

    /// Add a tool completed event
    pub fn tool_completed(
        mut self,
        tool_id: &str,
        success: bool,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Self {
        use crate::agent::events::ToolCompletedEvent;

        self.events
            .push(AgentEvent::ToolCompleted(ToolCompletedEvent {
                tool_id: tool_id.to_string(),
                success,
                result: result.map(String::from),
                error: error.map(String::from),
            }));
        self
    }

    /// Add a turn completed event
    pub fn turn_completed(mut self, input_tokens: i64, output_tokens: i64) -> Self {
        use crate::agent::events::{TokenUsage, TurnCompletedEvent};

        self.events
            .push(AgentEvent::TurnCompleted(TurnCompletedEvent {
                usage: TokenUsage {
                    input_tokens,
                    output_tokens,
                    cached_tokens: 0,
                    total_tokens: input_tokens + output_tokens,
                },
            }));
        self
    }

    /// Add an error event
    pub fn error(mut self, message: &str, is_fatal: bool) -> Self {
        use crate::agent::events::ErrorEvent;

        self.events.push(AgentEvent::Error(ErrorEvent {
            message: message.to_string(),
            is_fatal,
        }));
        self
    }

    /// Build the event vector
    pub fn build(self) -> Vec<AgentEvent> {
        self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::{AssistantMessageEvent, SessionInitEvent};
    use crate::agent::session::SessionId;

    #[tokio::test]
    async fn test_mock_emits_configured_events() {
        let events = vec![AgentEvent::SessionInit(SessionInitEvent {
            session_id: SessionId::from_string("test-session"),
            model: Some("mock-model".to_string()),
        })];

        let runner = MockAgentRunner::new(AgentType::Claude)
            .with_config(MockConfig::default().with_events(events));

        let config = AgentStartConfig::new("test prompt", PathBuf::from("/tmp"));
        let mut handle = runner.start(config).await.unwrap();

        let event = handle.events.recv().await;
        assert!(event.is_some());

        match event.unwrap() {
            AgentEvent::SessionInit(init) => {
                assert_eq!(init.session_id.as_str(), "test-session");
                assert_eq!(init.model, Some("mock-model".to_string()));
            }
            _ => panic!("Expected SessionInit event"),
        }
    }

    #[tokio::test]
    async fn test_mock_emits_sequence() {
        let events = vec![
            AgentEvent::SessionInit(SessionInitEvent {
                session_id: SessionId::from_string("test"),
                model: None,
            }),
            AgentEvent::AssistantMessage(AssistantMessageEvent {
                text: "Hello!".to_string(),
                is_final: true,
            }),
        ];

        let runner = MockAgentRunner::new(AgentType::Claude).with_events(events);

        let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
        let mut handle = runner.start(config).await.unwrap();

        let mut received = Vec::new();
        while let Some(event) = handle.events.recv().await {
            received.push(event);
        }

        assert_eq!(received.len(), 2);
        assert!(matches!(received[0], AgentEvent::SessionInit(_)));
        assert!(matches!(received[1], AgentEvent::AssistantMessage(_)));
    }

    #[tokio::test]
    async fn test_mock_captures_config() {
        let runner = MockAgentRunner::new(AgentType::Claude);

        let config = AgentStartConfig::new("Test prompt", PathBuf::from("/workspace"))
            .with_model("opus")
            .with_tools(vec!["Bash".to_string(), "Read".to_string()]);

        runner.start(config).await.unwrap();

        let captured = runner.captured_configs();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].prompt, "Test prompt");
        assert_eq!(captured[0].model, Some("opus".to_string()));
        assert_eq!(captured[0].allowed_tools, vec!["Bash", "Read"]);
    }

    #[tokio::test]
    async fn test_mock_captures_inputs() {
        let runner = MockAgentRunner::new(AgentType::Claude);
        let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
        let handle = runner.start(config).await.unwrap();

        runner
            .send_input(&handle, AgentInput::ClaudeJsonl("first input".to_string()))
            .await
            .unwrap();
        runner
            .send_input(&handle, AgentInput::ClaudeJsonl("second input".to_string()))
            .await
            .unwrap();

        let inputs = runner.captured_inputs();
        assert_eq!(
            inputs,
            vec![
                AgentInput::ClaudeJsonl("first input".to_string()),
                AgentInput::ClaudeJsonl("second input".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn test_mock_failure() {
        let runner =
            MockAgentRunner::new(AgentType::Claude).with_config(MockConfig::default().failing());

        let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
        let result = runner.start(config).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_tracks_stop_kill() {
        let runner = MockAgentRunner::new(AgentType::Claude);
        let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
        let handle = runner.start(config).await.unwrap();

        assert!(!runner.was_stopped());
        assert!(!runner.was_killed());

        runner.stop(&handle).await.unwrap();
        assert!(runner.was_stopped());

        runner.kill(&handle).await.unwrap();
        assert!(runner.was_killed());
    }

    #[tokio::test]
    async fn test_mock_reset() {
        let runner = MockAgentRunner::new(AgentType::Claude);
        let config = AgentStartConfig::new("test", PathBuf::from("/tmp"));
        let handle = runner.start(config).await.unwrap();

        runner
            .send_input(&handle, AgentInput::ClaudeJsonl("input".to_string()))
            .await
            .unwrap();
        runner.stop(&handle).await.unwrap();

        assert!(!runner.captured_configs().is_empty());
        assert!(!runner.captured_inputs().is_empty());
        assert!(runner.was_stopped());

        runner.reset();

        assert!(runner.captured_configs().is_empty());
        assert!(runner.captured_inputs().is_empty());
        assert!(!runner.was_stopped());
    }

    #[test]
    fn test_event_builder() {
        let events = MockEventBuilder::new("test-session")
            .session_init(Some("claude-sonnet"))
            .turn_started()
            .assistant_message("Hello!", true)
            .turn_completed(100, 50)
            .build();

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], AgentEvent::SessionInit(_)));
        assert!(matches!(events[1], AgentEvent::TurnStarted));
        assert!(matches!(events[2], AgentEvent::AssistantMessage(_)));
        assert!(matches!(events[3], AgentEvent::TurnCompleted(_)));
    }

    #[test]
    fn test_event_builder_with_tools() {
        let events = MockEventBuilder::new("test")
            .session_init(None)
            .tool_started("Bash", "tool-001", serde_json::json!({"command": "ls"}))
            .tool_completed("tool-001", true, Some("file.txt"), None)
            .build();

        assert_eq!(events.len(), 3);
        assert!(matches!(events[1], AgentEvent::ToolStarted(_)));
        assert!(matches!(events[2], AgentEvent::ToolCompleted(_)));
    }
}
