//! Session handlers for the Conduit web API.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::{
    load_claude_history_with_debug, load_codex_history_with_debug, AgentMode, AgentType,
    ModelRegistry,
};
use crate::core::services::session_service::CreateForkedSessionParams;
use crate::core::services::{
    CreateSessionParams, ServiceError, SessionService, UpdateSessionParams,
};
use crate::data::{ForkSeed, SessionTab, Workspace};
use crate::ui::app_prompt;
use crate::ui::components::{ChatMessage, MessageRole};
use crate::util::names::{generate_branch_name, generate_workspace_name, get_git_username};
use crate::web::error::WebError;
use crate::web::handlers::workspaces::WorkspaceResponse;
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

/// Request to update an existing session.
#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub model: Option<String>,
    pub agent_type: Option<String>,
    pub agent_mode: Option<String>,
}

/// List all sessions.
pub async fn list_sessions(
    State(state): State<WebAppState>,
) -> Result<Json<ListSessionsResponse>, WebError> {
    let core = state.core().await;
    let sessions = SessionService::list_sessions(&core).map_err(|err| map_service_error(err))?;

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
    let session = SessionService::get_session(&core, id).map_err(|err| map_service_error(err))?;

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
    let session = SessionService::create_session(
        &core,
        CreateSessionParams {
            workspace_id: req.workspace_id,
            agent_type,
            model: req.model,
        },
    )
    .map_err(|err| map_service_error(err))?;

    Ok((StatusCode::CREATED, Json(SessionResponse::from(session))))
}

/// Close (delete) a session.
pub async fn close_session(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    let core = state.core().await;
    SessionService::close_session(&core, id).map_err(|err| map_service_error(err))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Update an existing session.
pub async fn update_session(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<SessionResponse>, WebError> {
    let core = state.core().await;
    let agent_type = req
        .agent_type
        .as_ref()
        .map(
            |agent_type_str| match agent_type_str.to_lowercase().as_str() {
                "claude" => Ok(AgentType::Claude),
                "codex" => Ok(AgentType::Codex),
                "gemini" => Ok(AgentType::Gemini),
                _ => Err(WebError::BadRequest(format!(
                    "Invalid agent type: {}. Must be one of: claude, codex, gemini",
                    agent_type_str
                ))),
            },
        )
        .transpose()?;

    let agent_mode = req
        .agent_mode
        .as_ref()
        .map(|mode| match mode.to_lowercase().as_str() {
            "build" => Ok(AgentMode::Build),
            "plan" => Ok(AgentMode::Plan),
            _ => Err(WebError::BadRequest(format!(
                "Invalid agent mode: {}. Must be 'build' or 'plan'",
                mode
            ))),
        })
        .transpose()?;

    let session = SessionService::update_session(
        &core,
        id,
        UpdateSessionParams {
            model: req.model.clone(),
            agent_type,
            agent_mode,
        },
    )
    .map_err(|err| map_service_error(err))?;

    Ok(Json(SessionResponse::from(session)))
}

fn map_service_error(error: ServiceError) -> WebError {
    match error {
        ServiceError::InvalidInput(message) => WebError::BadRequest(message),
        ServiceError::NotFound(message) => WebError::NotFound(message),
        ServiceError::Internal(message) => WebError::Internal(message),
    }
}

fn load_history_for_session(session: &SessionTab) -> Vec<ChatMessage> {
    let Some(agent_session_id) = session.agent_session_id.as_deref() else {
        return Vec::new();
    };

    match session.agent_type {
        AgentType::Claude => load_claude_history_with_debug(agent_session_id)
            .map(|(messages, _, _)| messages)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to load Claude history: {}", e);
                Vec::new()
            }),
        AgentType::Codex => load_codex_history_with_debug(agent_session_id)
            .map(|(messages, _, _)| messages)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to load Codex history: {}", e);
                Vec::new()
            }),
        AgentType::Gemini => Vec::new(),
    }
}

fn estimate_tokens(text: &str) -> i64 {
    let chars = text.chars().count().max(1);
    ((chars as f64) / 4.0).ceil() as i64
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
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_file: Option<String>,
    #[serde(default)]
    pub debug_entries: Vec<HistoryDebugEntryResponse>,
}

/// Response for input history.
#[derive(Debug, Serialize)]
pub struct InputHistoryResponse {
    pub history: Vec<String>,
}

/// Response for a forked session.
#[derive(Debug, Serialize)]
pub struct ForkSessionResponse {
    pub session: SessionResponse,
    pub workspace: WorkspaceResponse,
    pub warnings: Vec<String>,
    pub token_estimate: i64,
    pub context_window: i64,
    pub usage_percent: f64,
    pub seed_prompt: String,
}

/// Debug entry for history loading (raw events view).
#[derive(Debug, Serialize)]
pub struct HistoryDebugEntryResponse {
    pub line: usize,
    pub entry_type: String,
    pub status: String,
    pub reason: String,
    pub raw: serde_json::Value,
}

#[derive(Debug, Deserialize, Default)]
pub struct SessionEventsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    #[serde(default)]
    pub tail: bool,
}

/// Get events/history for a session.
pub async fn get_session_events(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<SessionEventsQuery>,
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
            return Ok(Json(ListSessionEventsResponse {
                events: vec![],
                total: 0,
                offset: 0,
                limit: 0,
                debug_file: None,
                debug_entries: vec![],
            }));
        }
    };

    // Load history based on agent type
    let mut debug_entries = Vec::new();
    let mut debug_file: Option<String> = None;
    let messages = match session.agent_type {
        AgentType::Claude => match load_claude_history_with_debug(&agent_session_id) {
            Ok((msgs, entries, file_path)) => {
                debug_entries = entries;
                debug_file = Some(file_path.to_string_lossy().to_string());
                msgs
            }
            Err(e) => {
                tracing::warn!("Failed to load Claude history: {}", e);
                vec![]
            }
        },
        AgentType::Codex => match load_codex_history_with_debug(&agent_session_id) {
            Ok((msgs, entries, file_path)) => {
                debug_entries = entries;
                debug_file = Some(file_path.to_string_lossy().to_string());
                msgs
            }
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

    let messages: Vec<ChatMessage> = messages
        .into_iter()
        .filter(|msg| {
            !(msg.role == MessageRole::User
                && msg.content.trim_start().starts_with("[CONDUIT_FORK_SEED]"))
        })
        .collect();

    let total = messages.len();
    let limit = query.limit.unwrap_or(total).min(total);

    let (offset, selected) = if query.tail {
        let start = total.saturating_sub(limit);
        (start, messages.into_iter().skip(start).collect::<Vec<_>>())
    } else {
        let start = query.offset.unwrap_or(0).min(total);
        let end = (start + limit).min(total);
        (
            start,
            messages
                .into_iter()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect::<Vec<_>>(),
        )
    };

    // Convert ChatMessages to SessionEventResponse
    let events: Vec<SessionEventResponse> = selected
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

    let debug_entries: Vec<HistoryDebugEntryResponse> = debug_entries
        .into_iter()
        .map(|entry| HistoryDebugEntryResponse {
            line: entry.line_number,
            entry_type: entry.entry_type,
            status: entry.status,
            reason: entry.reason,
            raw: entry.raw_json,
        })
        .collect();

    Ok(Json(ListSessionEventsResponse {
        events,
        total,
        offset,
        limit,
        debug_file,
        debug_entries,
    }))
}

/// Get input history for a session.
pub async fn get_session_history(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<InputHistoryResponse>, WebError> {
    let core = state.core().await;
    let history =
        SessionService::get_input_history(&core, id).map_err(|err| map_service_error(err))?;
    Ok(Json(InputHistoryResponse { history }))
}

/// Fork a session into a new workspace and return the seed prompt.
pub async fn fork_session(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ForkSessionResponse>, WebError> {
    let core = state.core().await;

    let session = SessionService::get_session(&core, id).map_err(map_service_error)?;
    let workspace_id = session.workspace_id.ok_or_else(|| {
        WebError::BadRequest("Session is not associated with a workspace".to_string())
    })?;

    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let fork_seed_store = core
        .fork_seed_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let workspace = workspace_store
        .get_by_id(workspace_id)
        .map_err(|e| WebError::Internal(format!("Failed to load workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", workspace_id)))?;

    let repo = repo_store
        .get_by_id(workspace.repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to load repository: {}", e)))?
        .ok_or_else(|| {
            WebError::NotFound(format!("Repository {} not found", workspace.repository_id))
        })?;

    let base_repo_path = repo
        .base_path
        .clone()
        .ok_or_else(|| WebError::BadRequest("Repository has no base path".to_string()))?;

    let worktree_manager = core.worktree_manager();
    let base_branch = worktree_manager
        .get_current_branch(&workspace.path)
        .unwrap_or_else(|_| workspace.branch.clone());

    let history = load_history_for_session(&session);
    let seed_prompt = app_prompt::build_fork_seed_prompt(&history);
    let seed_hash = app_prompt::compute_seed_prompt_hash(&seed_prompt);

    let model_id = session
        .model
        .clone()
        .unwrap_or_else(|| ModelRegistry::default_model(session.agent_type));
    let context_window = ModelRegistry::context_window(session.agent_type, &model_id);
    let token_estimate = estimate_tokens(&seed_prompt);
    let usage_percent = if context_window > 0 {
        (token_estimate as f64 / context_window as f64) * 100.0
    } else {
        0.0
    };

    let mut warnings = Vec::new();
    if let Ok(status) = worktree_manager.get_branch_status(&workspace.path) {
        if status.is_dirty {
            if let Some(desc) = status.dirty_description {
                warnings.push(desc);
            } else {
                warnings.push("Uncommitted changes detected".to_string());
            }
            warnings.push("Commit before forking to preserve changes.".to_string());
        }
    }

    if usage_percent >= 100.0 {
        warnings.push(format!(
            "Seed exceeds context window ({} / {} tokens, ~{:.0}%).",
            token_estimate, context_window, usage_percent
        ));
    } else if usage_percent >= 80.0 {
        warnings.push(format!(
            "Seed uses ~{:.0}% of context window ({} / {}).",
            usage_percent, token_estimate, context_window
        ));
    }

    let fork_seed = ForkSeed::new(
        session.agent_type,
        session.agent_session_id.clone(),
        Some(workspace_id),
        seed_hash,
        None,
        token_estimate,
        context_window,
    );
    fork_seed_store
        .create(&fork_seed)
        .map_err(|e| WebError::Internal(format!("Failed to save fork metadata: {}", e)))?;

    let existing_names = workspace_store
        .get_all_names_by_repository(workspace.repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace names: {}", e)))?;
    let workspace_name = generate_workspace_name(&existing_names);
    let branch_name = generate_branch_name(&get_git_username(), &workspace_name);

    let worktree_path = worktree_manager
        .create_worktree_from_branch(&base_repo_path, &base_branch, &branch_name, &workspace_name)
        .map_err(|e| WebError::Internal(format!("Failed to create worktree: {}", e)))?;

    let new_workspace = Workspace::new(
        workspace.repository_id,
        &workspace_name,
        &branch_name,
        worktree_path,
    );
    if let Err(e) = workspace_store.create(&new_workspace) {
        if let Err(cleanup_err) =
            worktree_manager.remove_worktree(&base_repo_path, &new_workspace.path)
        {
            tracing::error!(
                error = %cleanup_err,
                base_path = %base_repo_path.display(),
                workspace_path = %new_workspace.path.display(),
                "Failed to clean up worktree after DB error"
            );
        }
        if let Err(branch_err) = worktree_manager.delete_branch(&base_repo_path, &branch_name) {
            tracing::error!(
                error = %branch_err,
                base_path = %base_repo_path.display(),
                workspace_path = %new_workspace.path.display(),
                branch = %branch_name,
                "Failed to delete branch after DB error"
            );
        }
        return Err(WebError::Internal(format!(
            "Failed to save workspace to database: {}",
            e
        )));
    }

    let forked_session = SessionService::create_forked_session(
        &core,
        CreateForkedSessionParams {
            workspace_id: new_workspace.id,
            agent_type: session.agent_type,
            agent_mode: session
                .agent_mode
                .as_ref()
                .and_then(|mode| match mode.as_str() {
                    "build" => Some(AgentMode::Build),
                    "plan" => Some(AgentMode::Plan),
                    _ => None,
                }),
            model: session.model.clone(),
            fork_seed_id: fork_seed.id,
        },
    )
    .map_err(map_service_error)?;

    state
        .status_manager()
        .register_workspace(new_workspace.id, new_workspace.path.clone());
    state.status_manager().refresh_workspace(new_workspace.id);

    Ok(Json(ForkSessionResponse {
        session: SessionResponse::from(forked_session),
        workspace: WorkspaceResponse::from(new_workspace),
        warnings,
        token_estimate,
        context_window,
        usage_percent,
        seed_prompt,
    }))
}
