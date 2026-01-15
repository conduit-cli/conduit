//! Workspace handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::agent::AgentType;
use crate::data::{SessionTab, Workspace};
use crate::git::{CheckState, GitDiffStats, PrManager, PrState};
use crate::util::names::{generate_branch_name, generate_workspace_name, get_git_username};
use crate::web::error::WebError;
use crate::web::handlers::sessions::SessionResponse;
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

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceResponse::from(workspace)),
    ))
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

    // Get the session store
    let session_store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Try to find existing session for this workspace
    if let Some(existing) = session_store
        .get_by_workspace_id(workspace_id)
        .map_err(|e| WebError::Internal(format!("Failed to query session: {}", e)))?
    {
        return Ok(Json(SessionResponse::from(existing)));
    }

    // Verify the workspace exists
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let _workspace = workspace_store
        .get_by_id(workspace_id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspace: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Workspace {} not found", workspace_id)))?;

    // No existing session - create new one with default agent (Claude)
    let sessions = session_store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list sessions: {}", e)))?;

    let next_index = sessions.iter().map(|s| s.tab_index).max().unwrap_or(-1) + 1;

    let session = SessionTab::new(
        next_index,
        AgentType::Claude, // Default agent
        Some(workspace_id),
        None, // agent_session_id will be set when agent starts
        None, // model - use default
        None, // pr_number
    );

    session_store
        .create(&session)
        .map_err(|e| WebError::Internal(format!("Failed to create session: {}", e)))?;

    Ok(Json(SessionResponse::from(session)))
}
