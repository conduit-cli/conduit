//! Workspace handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::core::services::{ServiceError, SessionService};
use crate::data::Workspace;
use crate::git::PrManager;
use crate::util::names::{generate_branch_name, generate_workspace_name, get_git_username};
use crate::web::error::WebError;
use crate::web::handlers::sessions::SessionResponse;
use crate::web::state::WebAppState;
use crate::web::status_types::{PrStatusResponse, WorkspaceStatusResponse};

/// Response for a single workspace.
#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub name: String,
    pub branch: String,
    pub path: String,
    pub created_at: String,
    pub last_accessed: String,
    pub is_default: bool,
    pub archived_at: Option<String>,
}

impl From<Workspace> for WorkspaceResponse {
    fn from(ws: Workspace) -> Self {
        Self {
            id: ws.id,
            repository_id: ws.repository_id,
            name: ws.name,
            branch: ws.branch,
            path: ws.path.to_string_lossy().to_string(),
            created_at: ws.created_at.to_rfc3339(),
            last_accessed: ws.last_accessed.to_rfc3339(),
            is_default: ws.is_default,
            archived_at: ws.archived_at.map(|d| d.to_rfc3339()),
        }
    }
}

/// Response for listing workspaces.
#[derive(Debug, Serialize)]
pub struct ListWorkspacesResponse {
    pub workspaces: Vec<WorkspaceResponse>,
}

/// PR preflight response for a workspace.
#[derive(Debug, Serialize)]
pub struct PrPreflightResponse {
    pub gh_installed: bool,
    pub gh_authenticated: bool,
    pub on_main_branch: bool,
    pub branch_name: String,
    pub target_branch: String,
    pub uncommitted_count: usize,
    pub has_upstream: bool,
    pub existing_pr: Option<PrStatusResponse>,
}

/// PR create response returns prompt to send to agent.
#[derive(Debug, Serialize)]
pub struct PrCreateResponse {
    pub preflight: PrPreflightResponse,
    pub prompt: String,
}

/// Request to create a new workspace.
#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub branch: String,
    pub path: String,
    #[serde(default)]
    pub is_default: bool,
}

/// List all workspaces.
pub async fn list_workspaces(
    State(state): State<WebAppState>,
) -> Result<Json<ListWorkspacesResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let workspaces = store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list workspaces: {}", e)))?;

    Ok(Json(ListWorkspacesResponse {
        workspaces: workspaces
            .into_iter()
            .map(WorkspaceResponse::from)
            .collect(),
    }))
}

/// List workspaces for a specific repository.
pub async fn list_repository_workspaces(
    State(state): State<WebAppState>,
    Path(repository_id): Path<Uuid>,
) -> Result<Json<ListWorkspacesResponse>, WebError> {
    let core = state.core().await;

    // First check if repository exists
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let _repo = repo_store
        .get_by_id(repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", repository_id)))?;

    // Get workspaces for the repository
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let workspaces = workspace_store
        .get_by_repository(repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to list workspaces: {}", e)))?;

    Ok(Json(ListWorkspacesResponse {
        workspaces: workspaces
            .into_iter()
            .map(WorkspaceResponse::from)
            .collect(),
    }))
}

/// Get a single workspace by ID.
pub async fn get_workspace(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkspaceResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let workspace = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", id)))?;

    Ok(Json(WorkspaceResponse::from(workspace)))
}

/// Create a new workspace for a repository.
pub async fn create_workspace(
    State(state): State<WebAppState>,
    Path(repository_id): Path<Uuid>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceResponse>), WebError> {
    // Validate request
    if req.name.is_empty() {
        return Err(WebError::BadRequest(
            "Workspace name is required".to_string(),
        ));
    }

    if req.branch.is_empty() {
        return Err(WebError::BadRequest("Branch is required".to_string()));
    }

    if req.path.is_empty() {
        return Err(WebError::BadRequest("Path is required".to_string()));
    }

    let core = state.core().await;

    // Check if repository exists
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let _repo = repo_store
        .get_by_id(repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", repository_id)))?;

    // Create workspace model
    let workspace = if req.is_default {
        Workspace::new_default(
            repository_id,
            &req.name,
            &req.branch,
            PathBuf::from(&req.path),
        )
    } else {
        Workspace::new(
            repository_id,
            &req.name,
            &req.branch,
            PathBuf::from(&req.path),
        )
    };

    // Save to database
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    workspace_store
        .create(&workspace)
        .map_err(|e| WebError::Internal(format!("Failed to create workspace: {}", e)))?;

    let response = WorkspaceResponse::from(workspace.clone());
    state
        .status_manager()
        .register_workspace(workspace.id, workspace.path.clone());
    state.status_manager().refresh_workspace(workspace.id);

    Ok((StatusCode::CREATED, Json(response)))
}

/// Archive a workspace (soft delete).
pub async fn archive_workspace(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Check if workspace exists
    let _workspace = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", id)))?;

    // Archive the workspace
    store
        .archive(id, None)
        .map_err(|e| WebError::Internal(format!("Failed to archive workspace: {}", e)))?;

    state.status_manager().remove_workspace(id);

    Ok(StatusCode::NO_CONTENT)
}

/// Delete a workspace.
pub async fn delete_workspace(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Check if workspace exists
    let _workspace = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", id)))?;

    // Delete workspace
    store
        .delete(id)
        .map_err(|e| WebError::Internal(format!("Failed to delete workspace: {}", e)))?;

    state.status_manager().remove_workspace(id);

    Ok(StatusCode::NO_CONTENT)
}

/// Auto-create a workspace with generated name/branch.
///
/// This endpoint mirrors the TUI's workspace creation flow:
/// 1. Generates a unique workspace name (adjective-noun)
/// 2. Generates a branch name (username/workspace-name)
/// 3. Creates a git worktree
/// 4. Saves the workspace to the database
pub async fn auto_create_workspace(
    State(state): State<WebAppState>,
    Path(repository_id): Path<Uuid>,
) -> Result<(StatusCode, Json<WorkspaceResponse>), WebError> {
    // Get write access to core for worktree operations
    let core = state.core_mut().await;

    // Load repository
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let repo = repo_store
        .get_by_id(repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", repository_id)))?;

    // Get existing workspace names (including archived) to avoid conflicts
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let existing_names = workspace_store
        .get_all_names_by_repository(repository_id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace names: {}", e)))?;

    // Generate unique workspace name
    let workspace_name = generate_workspace_name(&existing_names);

    // Generate branch name (username/workspace-name)
    let username = get_git_username();
    let branch_name = generate_branch_name(&username, &workspace_name);

    // Get repository path
    let repo_path = repo
        .base_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| WebError::BadRequest("Repository has no base path".to_string()))?;

    // Create git worktree
    let worktree_manager = core.worktree_manager();
    let worktree_path = worktree_manager
        .create_worktree(&repo_path, &branch_name, &workspace_name)
        .map_err(|e| WebError::Internal(format!("Failed to create worktree: {}", e)))?;

    // Create workspace model
    let workspace = Workspace::new(repository_id, &workspace_name, &branch_name, worktree_path);

    // Save to database
    workspace_store.create(&workspace).map_err(|e| {
        // If database save fails, try to clean up the worktree
        let _ = core
            .worktree_manager()
            .remove_worktree(&repo_path, &workspace.path);
        WebError::Internal(format!("Failed to save workspace: {}", e))
    })?;

    let response = WorkspaceResponse::from(workspace.clone());
    state
        .status_manager()
        .register_workspace(workspace.id, workspace.path.clone());
    state.status_manager().refresh_workspace(workspace.id);

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get workspace git status and PR info.
pub async fn get_workspace_status(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkspaceStatusResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Get the workspace
    let workspace = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", id)))?;

    state
        .status_manager()
        .register_workspace(workspace.id, workspace.path.clone());

    Ok(Json(
        state
            .status_manager()
            .get_status(workspace.id)
            .unwrap_or_default(),
    ))
}

/// Run PR preflight checks for a workspace.
pub async fn get_workspace_pr_preflight(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PrPreflightResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let workspace = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", id)))?;

    let preflight = PrManager::preflight_check(&workspace.path);
    Ok(Json(build_pr_preflight_response(preflight)))
}

/// Create a PR prompt for a workspace after preflight checks.
pub async fn create_workspace_pr(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PrCreateResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let workspace = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", id)))?;

    let preflight = PrManager::preflight_check(&workspace.path);
    let prompt = PrManager::generate_pr_prompt(&preflight);

    Ok(Json(PrCreateResponse {
        preflight: build_pr_preflight_response(preflight),
        prompt,
    }))
}

/// Get or create a session for a workspace.
///
/// This endpoint returns the existing session for a workspace if one exists,
/// or creates a new session with the default agent (Claude) if none exists.
/// This mirrors the TUI behavior where opening a workspace automatically
/// creates/restores a session.
pub async fn get_or_create_session(
    State(state): State<WebAppState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<SessionResponse>, WebError> {
    let core = state.core().await;
    let session = SessionService::get_or_create_session_for_workspace(&core, workspace_id)
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

fn build_pr_preflight_response(preflight: crate::git::PrPreflightResult) -> PrPreflightResponse {
    PrPreflightResponse {
        gh_installed: preflight.gh_installed,
        gh_authenticated: preflight.gh_authenticated,
        on_main_branch: preflight.on_main_branch,
        branch_name: preflight.branch_name,
        target_branch: preflight.target_branch,
        uncommitted_count: preflight.uncommitted_count,
        has_upstream: preflight.has_upstream,
        existing_pr: preflight
            .existing_pr
            .as_ref()
            .and_then(PrStatusResponse::from_pr_status),
    }
}
