//! WebSocket message types for the Conduit web API.
//!
//! This module defines the JSON protocol used for real-time communication
//! between the web client and the Conduit server.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::events::AgentEvent;
use crate::agent::runner::AgentType;

/// Messages sent from client to server over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Subscribe to events for a specific session
    Subscribe { session_id: Uuid },

    /// Unsubscribe from a session's events
    Unsubscribe { session_id: Uuid },

    /// Start a new agent session
    StartSession {
        /// Session ID to use (from /api/sessions)
        session_id: Uuid,
        /// Initial prompt to send to the agent
        prompt: String,
        /// Working directory path
        working_dir: String,
        /// Optional model override
        #[serde(default)]
        model: Option<String>,
    },

    /// Send input to a running agent (follow-up message)
    SendInput {
        session_id: Uuid,
        /// The input text to send
        input: String,
    },

    /// Respond to a control request (permission prompt)
    RespondToControl {
        session_id: Uuid,
        /// The control request ID to respond to
        request_id: String,
        /// Whether to allow the operation
        allow: bool,
    },

    /// Stop a running agent session
    StopSession { session_id: Uuid },

    /// Ping to keep connection alive
    Ping,
}

/// Messages sent from server to client over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Acknowledgment of subscription
    Subscribed { session_id: Uuid },

    /// Acknowledgment of unsubscription
    Unsubscribed { session_id: Uuid },

    /// Session started successfully
    SessionStarted {
        session_id: Uuid,
        agent_type: String,
        /// The agent's internal session ID (from claude/codex)
        agent_session_id: Option<String>,
    },

    /// Agent event forwarded from a session
    AgentEvent { session_id: Uuid, event: AgentEvent },

    /// Session ended (completed or stopped)
    SessionEnded {
        session_id: Uuid,
        /// Reason for ending ("completed", "stopped", "error")
        reason: String,
        /// Error message if reason is "error"
        error: Option<String>,
    },

    /// Error response
    Error {
        /// Error message
        message: String,
        /// Related session ID if applicable
        session_id: Option<Uuid>,
    },

    /// Pong response to ping
    Pong,
}

impl ServerMessage {
    /// Create an error message.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            session_id: None,
        }
    }

    /// Create an error message for a specific session.
    pub fn session_error(session_id: Uuid, message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            session_id: Some(session_id),
        }
    }

    /// Create an agent event message.
    pub fn agent_event(session_id: Uuid, event: AgentEvent) -> Self {
        Self::AgentEvent { session_id, event }
    }

    /// Create a session started message.
    pub fn session_started(
        session_id: Uuid,
        agent_type: AgentType,
        agent_session_id: Option<String>,
    ) -> Self {
        Self::SessionStarted {
            session_id,
            agent_type: agent_type.as_str().to_string(),
            agent_session_id,
        }
    }
}
