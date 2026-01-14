//! Workspace handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::data::Workspace;
use crate::git::{CheckState, GitDiffStats, PrManager, PrState};
use crate::web::error::WebError;
use crate::web::state::WebAppState;

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

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceResponse::from(workspace)),
    ))
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

    Ok(StatusCode::NO_CONTENT)
}

/// Response for git diff statistics.
#[derive(Debug, Serialize)]
pub struct GitDiffStatsResponse {
    pub additions: usize,
    pub deletions: usize,
    pub files_changed: usize,
}

impl From<GitDiffStats> for GitDiffStatsResponse {
    fn from(stats: GitDiffStats) -> Self {
        Self {
            additions: stats.additions,
            deletions: stats.deletions,
            files_changed: stats.files_changed,
        }
    }
}

/// Response for PR status.
#[derive(Debug, Serialize)]
pub struct PrStatusResponse {
    pub number: u32,
    pub state: String,
    pub checks_passing: bool,
    pub url: Option<String>,
}

/// Response for workspace git/PR status.
#[derive(Debug, Serialize)]
pub struct WorkspaceStatusResponse {
    pub git_stats: Option<GitDiffStatsResponse>,
    pub pr_status: Option<PrStatusResponse>,
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

    // Get git diff stats
    let git_stats = GitDiffStats::from_working_dir(&workspace.path);
    let git_stats_response = if git_stats.has_changes() {
        Some(GitDiffStatsResponse::from(git_stats))
    } else {
        None
    };

    // Get PR status (only if gh is available)
    let pr_status_response = if PrManager::is_gh_installed() && PrManager::is_gh_authenticated() {
        PrManager::get_existing_pr(&workspace.path).and_then(|pr| {
            if pr.exists {
                Some(PrStatusResponse {
                    number: pr.number?,
                    state: match pr.state {
                        PrState::Open => "open".to_string(),
                        PrState::Merged => "merged".to_string(),
                        PrState::Closed => "closed".to_string(),
                        PrState::Draft => "draft".to_string(),
                        PrState::Unknown => "unknown".to_string(),
                    },
                    checks_passing: matches!(pr.checks.state(), CheckState::Passing),
                    url: pr.url,
                })
            } else {
                None
            }
        })
    } else {
        None
    };

    Ok(Json(WorkspaceStatusResponse {
        git_stats: git_stats_response,
        pr_status: pr_status_response,
    }))
}
