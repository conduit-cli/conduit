//! Repository handlers for the Conduit web API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::data::Repository;
use crate::web::error::WebError;
use crate::web::state::WebAppState;

/// Response for a single repository.
#[derive(Debug, Serialize)]
pub struct RepositoryResponse {
    pub id: Uuid,
    pub name: String,
    pub base_path: Option<String>,
    pub repository_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Repository> for RepositoryResponse {
    fn from(repo: Repository) -> Self {
        Self {
            id: repo.id,
            name: repo.name,
            base_path: repo.base_path.map(|p| p.to_string_lossy().to_string()),
            repository_url: repo.repository_url,
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

/// List all repositories.
pub async fn list_repositories(
    State(state): State<WebAppState>,
) -> Result<Json<ListRepositoriesResponse>, WebError> {
    let core = state.core().await;
    let store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let repos = store
        .get_all()
        .map_err(|e| WebError::Internal(format!("Failed to list repositories: {}", e)))?;

    Ok(Json(ListRepositoriesResponse {
        repositories: repos.into_iter().map(RepositoryResponse::from).collect(),
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

    let repo = store
        .get_by_id(id)
        .map_err(|e| WebError::Internal(format!("Failed to get repository: {}", e)))?
        .ok_or_else(|| WebError::NotFound(format!("Repository {} not found", id)))?;

    Ok(Json(RepositoryResponse::from(repo)))
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

    store
        .create(&repo)
        .map_err(|e| WebError::Internal(format!("Failed to create repository: {}", e)))?;

    Ok((StatusCode::CREATED, Json(RepositoryResponse::from(repo))))
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
