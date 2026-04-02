//! Settings handlers for the Conduit web API.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentType, ModelRegistry};
use crate::core::services::{ConfigService, ServiceError};
use crate::git::WorkspaceMode;
use crate::ui::components::theme::current_theme_name;
use crate::web::error::WebError;
use crate::web::state::WebAppState;

// --- Unified settings summary ---

#[derive(Debug, Serialize)]
pub struct SettingItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub items: Vec<SettingItem>,
}

/// Return all settings with current values in a single call.
pub async fn get_settings(
    State(state): State<WebAppState>,
) -> Result<Json<SettingsResponse>, WebError> {
    let core = state.core().await;

    // Projects directory
    let projects_dir = core
        .app_state_store()
        .and_then(|store| store.get("projects_base_dir").ok().flatten())
        .unwrap_or_else(|| "Not set".to_string());

    // Default model
    let (agent, model_id) = ConfigService::default_model(&core);
    let default_model = ModelRegistry::find_model(agent, &model_id)
        .map(|model| format!("{}: {}", agent.display_name(), model.display_name))
        .unwrap_or_else(|| format!("{}: {}", agent.display_name(), model_id));

    // Enabled providers
    let enabled_providers: Vec<String> = core
        .config()
        .effective_enabled_providers(core.tools())
        .into_iter()
        .map(|p| p.display_name().to_string())
        .collect();
    let providers_value = if enabled_providers.is_empty() {
        "None".to_string()
    } else {
        enabled_providers.join(", ")
    };

    // Theme
    let theme_value = core
        .config()
        .theme_name
        .clone()
        .or_else(|| {
            core.config()
                .theme_path
                .as_ref()
                .map(|path| path.display().to_string())
        })
        .unwrap_or_else(current_theme_name);

    // Workspace defaults
    let ws = &core.config().workspaces;
    let workspace_value = format!(
        "{}, delete branch {}, remote prompt {}",
        ws.default_mode.as_str(),
        if ws.archive_delete_branch {
            "on"
        } else {
            "off"
        },
        if ws.archive_remote_prompt {
            "on"
        } else {
            "off"
        },
    );

    let items = vec![
        SettingItem {
            id: "projects_directory".to_string(),
            title: "Projects Directory".to_string(),
            description: "Where Conduit scans for local git projects".to_string(),
            value: projects_dir,
        },
        SettingItem {
            id: "default_model".to_string(),
            title: "Default Model".to_string(),
            description: "Agent + model used for new sessions".to_string(),
            value: default_model,
        },
        SettingItem {
            id: "enabled_providers".to_string(),
            title: "Enabled Providers".to_string(),
            description: "Providers shown in model selection".to_string(),
            value: providers_value,
        },
        SettingItem {
            id: "theme".to_string(),
            title: "Theme".to_string(),
            description: "Active color theme".to_string(),
            value: theme_value,
        },
        SettingItem {
            id: "workspace_defaults".to_string(),
            title: "Workspace Defaults".to_string(),
            description: "Defaults applied when a repo has no override".to_string(),
            value: workspace_value,
        },
    ];

    Ok(Json(SettingsResponse { items }))
}

// --- Providers ---

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub installed: bool,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    pub providers: Vec<ProviderInfo>,
}

/// List all providers with their installed/enabled state.
pub async fn get_providers(
    State(state): State<WebAppState>,
) -> Result<Json<ProvidersResponse>, WebError> {
    let core = state.core().await;
    let enabled = core.config().effective_enabled_providers(core.tools());

    let providers = AgentType::preferred_order()
        .iter()
        .map(|&agent| {
            let tool = match agent {
                AgentType::Claude => crate::util::Tool::Claude,
                AgentType::Codex => crate::util::Tool::Codex,
                AgentType::Gemini => crate::util::Tool::Gemini,
                AgentType::Opencode => crate::util::Tool::Opencode,
                AgentType::Pi => crate::util::Tool::Pi,
            };
            ProviderInfo {
                id: format!("{:?}", agent).to_lowercase(),
                display_name: agent.display_name().to_string(),
                installed: core.tools().is_available(tool),
                enabled: enabled.contains(&agent),
            }
        })
        .collect();

    Ok(Json(ProvidersResponse { providers }))
}

#[derive(Debug, Deserialize)]
pub struct SetProvidersRequest {
    pub enabled: Vec<String>,
}

/// Update the set of enabled providers.
pub async fn set_providers(
    State(state): State<WebAppState>,
    Json(req): Json<SetProvidersRequest>,
) -> Result<StatusCode, WebError> {
    let providers: Vec<AgentType> = req
        .enabled
        .iter()
        .filter_map(|s| match s.to_lowercase().as_str() {
            "codex" => Some(AgentType::Codex),
            "claude" => Some(AgentType::Claude),
            "gemini" => Some(AgentType::Gemini),
            "opencode" => Some(AgentType::Opencode),
            "pi" => Some(AgentType::Pi),
            _ => None,
        })
        .collect();

    let mut core = state.core_mut().await;
    ConfigService::set_enabled_providers(&mut core, providers).map_err(map_service_error)?;

    Ok(StatusCode::NO_CONTENT)
}

// --- Workspace defaults ---

#[derive(Debug, Serialize)]
pub struct WorkspaceDefaultsResponse {
    pub mode: String,
    pub archive_delete_branch: bool,
    pub archive_remote_prompt: bool,
}

/// Get the current workspace defaults.
pub async fn get_workspace_defaults(
    State(state): State<WebAppState>,
) -> Json<WorkspaceDefaultsResponse> {
    let core = state.core().await;
    let ws = &core.config().workspaces;
    Json(WorkspaceDefaultsResponse {
        mode: ws.default_mode.as_str().to_string(),
        archive_delete_branch: ws.archive_delete_branch,
        archive_remote_prompt: ws.archive_remote_prompt,
    })
}

#[derive(Debug, Deserialize)]
pub struct SetWorkspaceDefaultsRequest {
    pub mode: String,
    pub archive_delete_branch: bool,
    pub archive_remote_prompt: bool,
}

/// Update workspace defaults.
pub async fn set_workspace_defaults(
    State(state): State<WebAppState>,
    Json(req): Json<SetWorkspaceDefaultsRequest>,
) -> Result<StatusCode, WebError> {
    let mode = match req.mode.to_lowercase().as_str() {
        "worktree" => WorkspaceMode::Worktree,
        "checkout" => WorkspaceMode::Checkout,
        _ => {
            return Err(WebError::BadRequest(format!(
                "Invalid workspace mode: {}. Must be 'worktree' or 'checkout'",
                req.mode
            )));
        }
    };

    let mut core = state.core_mut().await;
    ConfigService::set_workspace_defaults(
        &mut core,
        mode,
        req.archive_delete_branch,
        req.archive_remote_prompt,
    )
    .map_err(map_service_error)?;

    Ok(StatusCode::NO_CONTENT)
}

fn map_service_error(error: ServiceError) -> WebError {
    match error {
        ServiceError::InvalidInput(message) => WebError::BadRequest(message),
        ServiceError::NotFound(message) => WebError::NotFound(message),
        ServiceError::Internal(message) => WebError::Internal(message),
    }
}
