//! Session handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::{
    load_claude_history_with_debug, load_codex_history_with_debug, AgentType, ModelRegistry,
};
use crate::data::SessionTab;
use crate::ui::components::MessageRole;
use crate::web::error::WebError;
use crate::web::state::WebAppState;

/// Response for a single session.
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub tab_index: i32,
    pub workspace_id: Option<Uuid>,
    pub agent_type: String,
    pub agent_mode: Option<String>,
    pub agent_session_id: Option<String>,
    pub model: Option<String>,
    pub model_display_name: Option<String>,
    pub pr_number: Option<i32>,
    pub created_at: String,
    pub title: Option<String>,
}

impl From<SessionTab> for SessionResponse {
    fn from(session: SessionTab) -> Self {
        // Look up model display name from registry
        let model_display_name = session.model.as_ref().and_then(|model_id| {
            ModelRegistry::find_model(session.agent_type, model_id).map(|info| info.display_name)
        });

        Self {
            id: session.id,
            tab_index: session.tab_index,
            workspace_id: session.workspace_id,
            agent_type: format!("{:?}", session.agent_type).to_lowercase(),
            agent_mode: session.agent_mode,
            agent_session_id: session.agent_session_id,
            model: session.model,
            model_display_name,
            pr_number: session.pr_number,
            created_at: session.created_at.to_rfc3339(),
            title: session.title,
        }
    }
}

/// Response for listing sessions.
#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionResponse>,
}

/// Request to create a new session.
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub workspace_id: Option<Uuid>,
    pub agent_type: String,
    pub model: Option<String>,
}

/// List all sessions.
pub async fn list_sessions(
    State(state): State<WebAppState>,
) -> Result<Json<ListSessionsResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let sessions = store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list sessions: {}", e)))?;

    Ok(Json(ListSessionsResponse {
        sessions: sessions.into_iter().map(SessionResponse::from).collect(),
    }))
}

/// Get a single session by ID.
pub async fn get_session(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let session = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get session: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Session {} not found", id)))?;

    Ok(Json(SessionResponse::from(session)))
}

/// Create a new session.
pub async fn create_session(
    State(state): State<WebAppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), WebError> {
    // Parse agent type
    let agent_type = match req.agent_type.to_lowercase().as_str() {
        "claude" => AgentType::Claude,
        "codex" => AgentType::Codex,
        "gemini" => AgentType::Gemini,
        _ => {
            return Err(WebError::BadRequest(format!(
                "Invalid agent type: {}. Must be one of: claude, codex, gemini",
                req.agent_type
            )));
        }
    };

    let core = state.core().await;
    let store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Get the next tab index
    let sessions = store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list sessions: {}", e)))?;

    let next_index = sessions.iter().map(|s| s.tab_index).max().unwrap_or(-1) + 1;

    // Create session model
    let session = SessionTab::new(
        next_index,
        agent_type,
        req.workspace_id,
        None, // agent_session_id will be set when agent starts
        req.model,
        None, // pr_number
    );

    // Save to database
    store
        .create(&session)
        .map_err(|e| WebError::Internal(format!("Failed to create session: {}", e)))?;

    Ok((StatusCode::CREATED, Json(SessionResponse::from(session))))
}

/// Close (delete) a session.
pub async fn close_session(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    let core = state.core().await;
    let store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Check if session exists
    let _session = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get session: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Session {} not found", id)))?;

    // Delete session
    store
        .delete(id)
        .map_err(|e| WebError::Internal(format!("Failed to close session: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

/// A single event/message in session history.
#[derive(Debug, Serialize)]
pub struct SessionEventResponse {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<TurnSummaryResponse>,
}

/// Turn summary information.
#[derive(Debug, Serialize)]
pub struct TurnSummaryResponse {
    pub duration_secs: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Response for session events.
#[derive(Debug, Serialize)]
pub struct ListSessionEventsResponse {
    pub events: Vec<SessionEventResponse>,
}

/// Get events/history for a session.
pub async fn get_session_events(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ListSessionEventsResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Get the session
    let session = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get session: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Session {} not found", id)))?;

    // Get the agent session ID
    let agent_session_id = match &session.agent_session_id {
        Some(id) => id.clone(),
        None => {
            // No agent session ID means no history yet
            return Ok(Json(ListSessionEventsResponse { events: vec![] }));
        }
    };

    // Load history based on agent type
    let messages = match session.agent_type {
        AgentType::Claude => match load_claude_history_with_debug(&agent_session_id) {
            Ok((msgs, _, _)) => msgs,
            Err(e) => {
                tracing::warn!("Failed to load Claude history: {}", e);
                vec![]
            }
        },
        AgentType::Codex => match load_codex_history_with_debug(&agent_session_id) {
            Ok((msgs, _, _)) => msgs,
            Err(e) => {
                tracing::warn!("Failed to load Codex history: {}", e);
                vec![]
            }
        },
        AgentType::Gemini => {
            // Gemini history loading not supported yet
            vec![]
        }
    };

    // Convert ChatMessages to SessionEventResponse
    let events: Vec<SessionEventResponse> = messages
        .into_iter()
        .map(|msg| {
            let role = match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
                MessageRole::System => "system",
                MessageRole::Error => "error",
                MessageRole::Summary => "summary",
            }
            .to_string();

            let summary = msg.summary.map(|s| TurnSummaryResponse {
                duration_secs: s.duration_secs,
                input_tokens: s.input_tokens,
                output_tokens: s.output_tokens,
            });

            SessionEventResponse {
                role,
                content: msg.content,
                tool_name: msg.tool_name,
                tool_args: msg.tool_args,
                exit_code: msg.exit_code,
                summary,
            }
        })
        .collect();

    Ok(Json(ListSessionEventsResponse { events }))
}
