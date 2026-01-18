//! Onboarding handlers for the Conduit web API.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::data::Repository;
use crate::web::error::WebError;
use crate::web::handlers::repositories::RepositoryResponse;
use crate::web::state::WebAppState;

const PROJECTS_BASE_DIR_KEY: &str = "projects_base_dir";

#[derive(Debug, Serialize)]
pub struct BaseDirResponse {
    pub base_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetBaseDirRequest {
    pub base_dir: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectEntryResponse {
    pub name: String,
    pub path: String,
    pub modified_at: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectsResponse {
    pub projects: Vec<ProjectEntryResponse>,
}

#[derive(Debug, Deserialize)]
pub struct AddProjectRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct AddProjectResponse {
    pub repository: RepositoryResponse,
}

fn expand_path(raw: &str) -> PathBuf {
    if let Some(stripped) = raw.strip_prefix('~') {
        let stripped = stripped.trim_start_matches('/');
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

fn validate_dir(path: &Path) -> Result<(), WebError> {
    if !path.exists() {
        return Err(WebError::BadRequest("Directory does not exist".to_string()));
    }

    if !path.is_dir() {
        return Err(WebError::BadRequest("Path is not a directory".to_string()));
    }

    Ok(())
}

fn ensure_git_dir(path: &Path) -> Result<(), WebError> {
    let git_dir = path.join(".git");
    if !git_dir.exists() {
        return Err(WebError::BadRequest(
            "Not a git repository (no .git directory)".to_string(),
        ));
    }
    Ok(())
}

pub async fn get_base_dir(
    State(state): State<WebAppState>,
) -> Result<Json<BaseDirResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .app_state_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let base_dir = store.get(PROJECTS_BASE_DIR_KEY)?;
    Ok(Json(BaseDirResponse { base_dir }))
}

pub async fn set_base_dir(
    State(state): State<WebAppState>,
    Json(req): Json<SetBaseDirRequest>,
) -> Result<Json<BaseDirResponse>, WebError> {
    if req.base_dir.trim().is_empty() {
        return Err(WebError::BadRequest(
            "Base directory cannot be empty".to_string(),
        ));
    }

    let expanded = expand_path(req.base_dir.trim());
    validate_dir(&expanded)?;

    let core = state.core().await;
    let store = core
        .app_state_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    store.set(PROJECTS_BASE_DIR_KEY, req.base_dir.trim())?;

    Ok(Json(BaseDirResponse {
        base_dir: Some(req.base_dir.trim().to_string()),
    }))
}

pub async fn list_projects(
    State(state): State<WebAppState>,
) -> Result<Json<ProjectsResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .app_state_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let base_dir = store.get(PROJECTS_BASE_DIR_KEY)?;
    let base_dir = base_dir
        .ok_or_else(|| WebError::BadRequest("Projects directory is not set".to_string()))?;
    let base_path = expand_path(&base_dir);
    validate_dir(&base_path)?;

    let entries = std::fs::read_dir(&base_path)
        .map_err(|e| WebError::Internal(format!("Failed to read projects directory: {e}")))?;

    let mut projects = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if !path.join(".git").exists() {
            continue;
        }

        let modified_at = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let modified_at = chrono::DateTime::<chrono::Utc>::from(modified_at).to_rfc3339();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        projects.push(ProjectEntryResponse {
            name,
            path: path.to_string_lossy().to_string(),
            modified_at,
        });
    }

    projects.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

    Ok(Json(ProjectsResponse { projects }))
}

pub async fn add_project(
    State(state): State<WebAppState>,
    Json(req): Json<AddProjectRequest>,
) -> Result<Json<AddProjectResponse>, WebError> {
    if req.path.trim().is_empty() {
        return Err(WebError::BadRequest("Path cannot be empty".to_string()));
    }

    let expanded = expand_path(req.path.trim());
    validate_dir(&expanded)?;
    ensure_git_dir(&expanded)?;

    let core = state.core().await;
    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    if let Some(existing) = repo_store
        .get_by_path(&expanded)
        .map_err(|e| WebError::Internal(format!("Failed to check repositories: {e}")))?
    {
        return Ok(Json(AddProjectResponse {
            repository: RepositoryResponse::from(existing),
        }));
    }

    let name = expanded
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let repo = Repository::from_local_path(&name, expanded);
    repo_store
        .create(&repo)
        .map_err(|e| WebError::Internal(format!("Failed to create repository: {e}")))?;

    Ok(Json(AddProjectResponse {
        repository: RepositoryResponse::from(repo),
    }))
}
