use axum::{extract::State, Json};
use uuid::Uuid;

use crate::core::services::{ServiceError, SessionService};
use crate::data::{SessionTab, Workspace};
use crate::web::error::WebError;
use crate::web::handlers::sessions::SessionResponse;
use crate::web::handlers::ui_state::{load_ui_state, state_store, WebUiStateResponse};
use crate::web::handlers::workspaces::WorkspaceResponse;
use crate::web::state::WebAppState;

#[derive(Debug, serde::Serialize)]
pub struct BootstrapResponse {
    pub ui_state: WebUiStateResponse,
    pub sessions: Vec<SessionResponse>,
    pub workspaces: Vec<WorkspaceResponse>,
    pub active_session: Option<SessionResponse>,
    pub active_workspace: Option<WorkspaceResponse>,
}

fn resolve_active_session(
    ui_state_session_id: Option<Uuid>,
    sessions: &[SessionTab],
) -> Option<SessionTab> {
    if let Some(id) = ui_state_session_id {
        if let Some(found) = sessions.iter().find(|session| session.id == id) {
            return Some(found.clone());
        }
    }
    sessions.first().cloned()
}

fn resolve_active_workspace(
    ui_state_workspace_id: Option<Uuid>,
    active_session: Option<&SessionTab>,
    workspaces: &[Workspace],
) -> Option<Workspace> {
    if let Some(id) = ui_state_workspace_id {
        if let Some(found) = workspaces.iter().find(|workspace| workspace.id == id) {
            return Some(found.clone());
        }
    }

    if let Some(session) = active_session {
        if let Some(workspace_id) = session.workspace_id {
            if let Some(found) = workspaces
                .iter()
                .find(|workspace| workspace.id == workspace_id)
            {
                return Some(found.clone());
            }
        }
    }

    workspaces.first().cloned()
}

pub async fn get_bootstrap(
    State(state): State<WebAppState>,
) -> Result<Json<BootstrapResponse>, WebError> {
    let core = state.core().await;
    let store = state_store(&core)?;
    let ui_state = load_ui_state(store)?;

    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let sessions = SessionService::list_sessions(&core).map_err(|err| map_service_error(err))?;
    let workspaces = workspace_store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list workspaces: {}", e)))?;

    let active_session = resolve_active_session(ui_state.active_session_id, &sessions);
    let active_workspace = resolve_active_workspace(
        ui_state.last_workspace_id,
        active_session.as_ref(),
        &workspaces,
    );

    Ok(Json(BootstrapResponse {
        ui_state: WebUiStateResponse::from(ui_state),
        sessions: sessions.into_iter().map(SessionResponse::from).collect(),
        workspaces: workspaces
            .into_iter()
            .map(WorkspaceResponse::from)
            .collect(),
        active_session: active_session.map(SessionResponse::from),
        active_workspace: active_workspace.map(WorkspaceResponse::from),
    }))
}

fn map_service_error(error: ServiceError) -> WebError {
    match error {
        ServiceError::InvalidInput(message) => WebError::BadRequest(message),
        ServiceError::NotFound(message) => WebError::NotFound(message),
        ServiceError::Internal(message) => WebError::Internal(message),
    }
}
