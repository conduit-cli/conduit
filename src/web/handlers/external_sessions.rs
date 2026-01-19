//! External session discovery/import handlers.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};

use crate::agent::AgentType;
use crate::core::services::session_service::CreateImportedSessionParams;
use crate::core::services::{ServiceError, SessionService};
use crate::data::{Repository, Workspace};
use crate::git::WorktreeManager;
use crate::session::{discover_all_sessions, ExternalSession};
use crate::util::names::{generate_branch_name, get_git_username};
use crate::web::error::WebError;
use crate::web::handlers::repositories::RepositoryResponse;
use crate::web::handlers::sessions::SessionResponse;
use crate::web::handlers::workspaces::WorkspaceResponse;
use crate::web::state::WebAppState;

#[derive(Debug, Deserialize, Default)]
pub struct ExternalSessionsQuery {
    pub agent_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExternalSessionResponse {
    pub id: String,
    pub agent_type: String,
    pub display: String,
    pub project: Option<String>,
    pub project_name: Option<String>,
    pub timestamp: String,
    pub relative_time: String,
    pub message_count: usize,
    pub file_path: String,
}

impl ExternalSessionResponse {
    fn from_session(session: ExternalSession) -> Self {
        Self {
            id: session.id.clone(),
            agent_type: format!("{:?}", session.agent_type).to_lowercase(),
            display: session.truncated_display(140),
            project: session.project.clone(),
            project_name: session.project_name(),
            timestamp: session.timestamp.to_rfc3339(),
            relative_time: session.relative_time(),
            message_count: session.message_count,
            file_path: session.file_path.to_string_lossy().to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListExternalSessionsResponse {
    pub sessions: Vec<ExternalSessionResponse>,
}

#[derive(Debug, Serialize)]
pub struct ImportExternalSessionResponse {
    pub session: SessionResponse,
    pub workspace: Option<WorkspaceResponse>,
    pub repository: Option<RepositoryResponse>,
}

pub async fn list_external_sessions(
    Query(query): Query<ExternalSessionsQuery>,
) -> Result<Json<ListExternalSessionsResponse>, WebError> {
    let sessions = discover_all_sessions();
    let sessions = match query.agent_type {
        Some(agent_type) => {
            let agent = parse_agent_type(&agent_type)?;
            sessions
                .into_iter()
                .filter(|session| session.agent_type == agent)
                .collect()
        }
        None => sessions,
    };

    let response = ListExternalSessionsResponse {
        sessions: sessions
            .into_iter()
            .map(ExternalSessionResponse::from_session)
            .collect(),
    };

    Ok(Json(response))
}

pub async fn import_external_session(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<ImportExternalSessionResponse>), WebError> {
    let sessions = discover_all_sessions();
    let session = sessions
        .iter()
        .find(|session| session.id == id)
        .cloned()
        .ok_or_else(|| WebError::NotFound(format!("External session {} not found", id)))?;

    let core = state.core_mut().await;

    let (workspace, repository) = ensure_workspace_for_external_session(&core, &session)?;

    let session_tab = SessionService::create_imported_session(
        &core,
        CreateImportedSessionParams {
            workspace_id: workspace.as_ref().map(|ws| ws.id),
            agent_type: session.agent_type,
            agent_session_id: session.id.clone(),
            title: Some(session.truncated_display(80)),
            model: None,
        },
    )
    .map_err(map_service_error)?;

    if let Some(ref ws) = workspace {
        state
            .status_manager()
            .register_workspace(ws.id, ws.path.clone());
        state.status_manager().refresh_workspace(ws.id);
    }

    Ok((
        StatusCode::CREATED,
        Json(ImportExternalSessionResponse {
            session: SessionResponse::from(session_tab),
            workspace: workspace.map(WorkspaceResponse::from),
            repository: repository.map(RepositoryResponse::from),
        }),
    ))
}

fn ensure_workspace_for_external_session(
    core: &crate::core::ConduitCore,
    session: &ExternalSession,
) -> Result<(Option<Workspace>, Option<Repository>), WebError> {
    let project = match session.project.as_ref() {
        Some(project) => PathBuf::from(project),
        None => return Ok((None, None)),
    };

    if !project.exists() {
        tracing::warn!(
            path = %project.display(),
            "External session project path does not exist"
        );
        return Ok((None, None));
    }

    let repo_root = match resolve_repo_root(&project) {
        Some(path) => path,
        None => {
            tracing::warn!(
                path = %project.display(),
                "External session project is not a git repository"
            );
            return Ok((None, None));
        }
    };

    let repo_store = core
        .repo_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;
    let workspace_store = core
        .workspace_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))?;

    let repo = if let Some(existing) = repo_store
        .get_by_path(&repo_root)
        .map_err(|e| WebError::Internal(format!("Failed to query repository: {}", e)))?
    {
        existing
    } else {
        let name = repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Imported")
            .to_string();
        let repo = Repository::from_local_path(name, repo_root.clone());
        repo_store
            .create(&repo)
            .map_err(|e| WebError::Internal(format!("Failed to create repository: {}", e)))?;
        repo
    };

    let workspace = if let Some(existing) = workspace_store
        .get_by_path(&project)
        .map_err(|e| WebError::Internal(format!("Failed to query workspace: {}", e)))?
    {
        existing
    } else {
        let base_name = project
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("imported");
        let existing_names = workspace_store
            .get_all_names_by_repository(repo.id)
            .map_err(|e| WebError::Internal(format!("Failed to get workspace names: {}", e)))?;
        let name = unique_workspace_name(base_name, &existing_names);

        let worktree_manager = WorktreeManager::new();
        let branch = match worktree_manager.get_current_branch(&project) {
            Ok(branch) => branch,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    project = %project.display(),
                    "Failed to read current branch for imported project"
                );
                generate_branch_name(&get_git_username(), base_name)
            }
        };

        let workspace = Workspace::new(repo.id, name, branch, project.clone());
        workspace_store
            .create(&workspace)
            .map_err(|e| WebError::Internal(format!("Failed to create workspace: {}", e)))?;
        workspace
    };

    Ok((Some(workspace), Some(repo)))
}

fn resolve_repo_root(path: &StdPath) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return None;
    }
    Some(PathBuf::from(root))
}

fn unique_workspace_name(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|name| name == base) {
        return base.to_string();
    }

    let mut idx = 2;
    loop {
        let candidate = format!("{base}-{idx}");
        if !existing.iter().any(|name| name == &candidate) {
            return candidate;
        }
        idx += 1;
    }
}

fn parse_agent_type(agent_type: &str) -> Result<AgentType, WebError> {
    match agent_type.to_lowercase().as_str() {
        "claude" => Ok(AgentType::Claude),
        "codex" => Ok(AgentType::Codex),
        "gemini" => Ok(AgentType::Gemini),
        _ => Err(WebError::BadRequest(format!(
            "Invalid agent type: {}. Must be one of: claude, codex, gemini",
            agent_type
        ))),
    }
}

fn map_service_error(error: ServiceError) -> WebError {
    match error {
        ServiceError::InvalidInput(message) => WebError::BadRequest(message),
        ServiceError::NotFound(message) => WebError::NotFound(message),
        ServiceError::Internal(message) => WebError::Internal(message),
    }
}
