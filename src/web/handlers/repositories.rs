//! Repository handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};
use uuid::Uuid;

use crate::core::resolve_repo_workspace_settings;
use crate::data::Repository;
use crate::git::WorkspaceMode;
use crate::web::error::WebError;
use crate::web::state::WebAppState;

/// Response for a single repository.
#[derive(Debug, Serialize)]
pub struct RepositoryResponse {
    pub id: Uuid,
    pub name: String,
    pub base_path: Option<String>,
    pub repository_url: Option<String>,
    pub workspace_mode: Option<WorkspaceMode>,
    pub workspace_mode_effective: WorkspaceMode,
    pub archive_delete_branch: Option<bool>,
    pub archive_delete_branch_effective: bool,
    pub archive_remote_prompt: Option<bool>,
    pub archive_remote_prompt_effective: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl RepositoryResponse {
    pub(crate) fn from_repo(repo: Repository, config: &crate::config::Config) -> Self {
        let settings = resolve_repo_workspace_settings(config, &repo);
        Self {
            id: repo.id,
            name: repo.name,
            base_path: repo.base_path.map(|p| p.to_string_lossy().to_string()),
            repository_url: repo.repository_url,
            workspace_mode: repo.workspace_mode,
            workspace_mode_effective: settings.mode,
            archive_delete_branch: repo.archive_delete_branch,
            archive_delete_branch_effective: settings.archive_delete_branch,
            archive_remote_prompt: repo.archive_remote_prompt,
            archive_remote_prompt_effective: settings.archive_remote_prompt,
            created_at: repo.created_at.to_rfc3339(),
            updated_at: repo.updated_at.to_rfc3339(),
        }
    }
}

/// Response for listing repositories.
#[derive(Debug, Serialize)]
pub struct ListRepositoriesResponse {
    pub repositories: Vec<RepositoryResponse>,
}

/// Request to create a new repository.
#[derive(Debug, Deserialize)]
pub struct CreateRepositoryRequest {
    pub name: String,
    pub base_path: Option<String>,
    pub repository_url: Option<String>,
}

/// Request to update repository workspace settings.
#[derive(Debug, Deserialize)]
pub struct UpdateRepositorySettingsRequest {
    pub workspace_mode: Option<WorkspaceMode>,
    pub archive_delete_branch: Option<bool>,
    pub archive_remote_prompt: Option<bool>,
}

/// List all repositories.
pub async fn list_repositories(
    State(state): State<WebAppState>,
) -> Result<Json<ListRepositoriesResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let config = core.config();

    let repos = store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list repositories: {}", e)))?;

    Ok(Json(ListRepositoriesResponse {
        repositories: repos
            .into_iter()
            .map(|repo| RepositoryResponse::from_repo(repo, config))
            .collect(),
    }))
}

/// Get a single repository by ID.
pub async fn get_repository(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepositoryResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let config = core.config();

    let repo = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    Ok(Json(RepositoryResponse::from_repo(repo, config)))
}

/// Create a new repository.
pub async fn create_repository(
    State(state): State<WebAppState>,
    Json(req): Json<CreateRepositoryRequest>,
) -> Result<(StatusCode, Json<RepositoryResponse>), WebError> {
    // Validate request
    if req.name.is_empty() {
        return Err(WebError::BadRequest(
            "Repository name is required".to_string(),
        ));
    }

    if req.base_path.is_none() && req.repository_url.is_none() {
        return Err(WebError::BadRequest(
            "Either base_path or repository_url is required".to_string(),
        ));
    }

    // Create repository model
    let repo = if let Some(path) = req.base_path {
        Repository::from_local_path(&req.name, PathBuf::from(path))
    } else if let Some(url) = req.repository_url {
        Repository::from_url(&req.name, url)
    } else {
        unreachable!()
    };

    // Save to database
    let core = state.core().await;
    let store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let config = core.config();

    store
        .create(&repo)
        .map_err(|e| WebError::Internal(format!("Failed to create repository: {}", e)))?;

    Ok((
        StatusCode::CREATED,
        Json(RepositoryResponse::from_repo(repo, config)),
    ))
}

/// Update repository workspace settings.
pub async fn update_repository_settings(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateRepositorySettingsRequest>,
) -> Result<Json<RepositoryResponse>, WebError> {
    let core = state.core().await;
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let repo = repo_store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    if let Some(mode) = req.workspace_mode {
        let active_count = workspace_store
            .count_active_by_repository(id)
            .map_err(|e| WebError::Internal(format!("Failed to check workspaces: {}", e)))?;

        if active_count > 0 && repo.workspace_mode != Some(mode) {
            return Err(WebError::Conflict(
                "workspace_mode_locked_active_workspaces".to_string(),
            ));
        }
    }

    let workspace_mode = req.workspace_mode.or(repo.workspace_mode);
    let archive_delete_branch = req.archive_delete_branch.or(repo.archive_delete_branch);
    let archive_remote_prompt = req.archive_remote_prompt.or(repo.archive_remote_prompt);

    repo_store
        .update_settings(
            id,
            workspace_mode,
            archive_delete_branch,
            archive_remote_prompt,
        )
        .map_err(|e| WebError::Internal(format!("Failed to update repository: {}", e)))?;

    let updated = repo_store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to load repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    Ok(Json(RepositoryResponse::from_repo(updated, core.config())))
}

/// Delete a repository.
pub async fn delete_repository(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    let core = state.core().await;
    let store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    // Check if repository exists
    let _repo = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    // Delete repository (cascades to workspaces)
    store
        .delete(id)
        .map_err(|e| WebError::Internal(format!("Failed to delete repository: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Response for remove preflight checks.
#[derive(Debug, Serialize)]
pub struct RepositoryRemovePreflightResponse {
    pub repository_name: String,
    pub workspace_count: usize,
    pub warnings: Vec<String>,
    pub severity: String, // "info" | "warning" | "danger"
}

/// Preflight checks before removing a repository.
///
/// Returns information about workspaces that will be affected,
/// including warnings about uncommitted changes or unmerged branches.
pub async fn get_repository_remove_preflight(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepositoryRemovePreflightResponse>, WebError> {
    let core = state.core().await;
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let repo = repo_store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    let workspaces = workspace_store
        .get_by_repository(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspaces: {}", e)))?;

    let worktree_manager = core.worktree_manager();
    let mut warnings = Vec::new();
    let mut has_dirty = false;
    let mut has_unmerged = false;

    // Add workspace count warning
    if !workspaces.is_empty() {
        warnings.push(format!(
            "{} workspace(s) will be archived",
            workspaces.len()
        ));
    }

    // Check git status for each workspace
    for ws in &workspaces {
        match worktree_manager.get_branch_status(&ws.path) {
            Ok(status) => {
                if status.is_dirty {
                    has_dirty = true;
                }
                if !status.is_merged {
                    has_unmerged = true;
                }
            }
            Err(e) => {
                tracing::warn!(
                    workspace_id = %ws.id,
                    error = %e,
                    "Failed to get git status for workspace during preflight"
                );
            }
        }
    }

    if has_dirty {
        warnings.push("Some workspaces have uncommitted changes".to_string());
    }

    if has_unmerged {
        warnings.push("Some branches are not merged to main".to_string());
    }

    // Determine severity
    let severity = if has_dirty && has_unmerged {
        "danger"
    } else if has_dirty || has_unmerged {
        "warning"
    } else {
        "info"
    };

    Ok(Json(RepositoryRemovePreflightResponse {
        repository_name: repo.name,
        workspace_count: workspaces.len(),
        warnings,
        severity: severity.to_string(),
    }))
}

/// Response for remove repository operation.
#[derive(Debug, Serialize)]
pub struct RepositoryRemoveResponse {
    pub success: bool,
    pub errors: Vec<String>,
}

/// Remove a repository and archive all its workspaces.
///
/// This mirrors the TUI's RemoveProject logic:
/// 1. For each workspace: get branch SHA, remove worktree, delete branch, archive in DB
/// 2. Delete the repository folder (with path safety checks)
/// 3. Delete the repository from DB
pub async fn remove_repository(
    State(state): State<WebAppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepositoryRemoveResponse>, WebError> {
    let core = state.core().await;
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let session_store = core
        .session_tab_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let repo = repo_store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    let workspaces = workspace_store
        .get_by_repository(id)
        .map_err(|e| WebError::Internal(format!("Failed to get workspaces: {}", e)))?;

    let worktree_manager = core.worktree_manager();
    let settings = resolve_repo_workspace_settings(core.config(), &repo);
    let mut errors = Vec::new();

    // Process each workspace
    for ws in &workspaces {
        let mut archived_commit_sha = None;

        if let Some(ref base_path) = repo.base_path {
            // Get branch SHA
            match worktree_manager.get_branch_sha(settings.mode, base_path, &ws.path, &ws.branch) {
                Ok(sha) => {
                    archived_commit_sha = Some(sha);
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed to read branch SHA for workspace '{}': {}",
                        ws.name, e
                    ));
                }
            }

            // Remove worktree
            if let Err(e) = worktree_manager.remove_workspace(settings.mode, base_path, &ws.path) {
                errors.push(format!("Failed to remove worktree '{}': {}", ws.name, e));
            }

            // Delete branch
            if let Err(e) =
                worktree_manager.delete_branch(settings.mode, base_path, &ws.path, &ws.branch)
            {
                errors.push(format!(
                    "Failed to delete branch '{}' for workspace '{}': {}",
                    ws.branch, ws.name, e
                ));
            }
        }

        // Archive workspace in DB
        if let Err(e) = workspace_store.archive(ws.id, archived_commit_sha) {
            errors.push(format!("Failed to archive workspace '{}': {}", ws.name, e));
        }

        // Close sessions for workspace
        if let Err(e) = session_store.set_open_by_workspace(ws.id, false) {
            tracing::warn!(error = %e, "Failed to close sessions for archived workspace");
        }

        // Remove from status manager
        state.status_manager().remove_workspace(ws.id);
    }

    // Delete repository folder (with path safety checks)
    let workspaces_dir = crate::util::workspaces_dir();
    let repo_name_path = std::path::Path::new(&repo.name);
    let mut components = repo_name_path.components();
    let is_safe_repo_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();

    if !is_safe_repo_name {
        errors.push(format!(
            "Skipping project folder removal due to unsafe repo name: {}",
            repo.name
        ));
    } else {
        let project_workspaces_path = workspaces_dir.join(&repo.name);
        match (
            std::fs::canonicalize(&workspaces_dir),
            std::fs::canonicalize(&project_workspaces_path),
        ) {
            (Ok(canonical_root), Ok(canonical_project)) => {
                if canonical_project.starts_with(&canonical_root) {
                    if let Err(e) = std::fs::remove_dir_all(&canonical_project) {
                        errors.push(format!("Failed to remove project folder: {}", e));
                    }
                } else {
                    errors.push(format!(
                        "Skipping project folder removal outside managed root: {}",
                        canonical_project.display()
                    ));
                }
            }
            (Err(e), _) => {
                errors.push(format!("Failed to canonicalize workspaces dir: {}", e));
            }
            (_, Err(e)) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    errors.push(format!("Failed to canonicalize project folder: {}", e));
                }
            }
        }
    }

    // Delete repository from DB
    if let Err(e) = repo_store.delete(id) {
        errors.push(format!("Failed to delete repository from database: {}", e));
    }

    if !errors.is_empty() {
        tracing::warn!(
            repository_id = %id,
            errors = ?errors,
            "Repository removed with warnings"
        );
    }

    Ok(Json(RepositoryRemoveResponse {
        success: errors.is_empty(),
        errors,
    }))
}
