//! Models handler for the Conduit web API.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentType, ModelRegistry};
use crate::config::save_default_model;
use crate::web::error::WebError;
use crate::web::state::WebAppState;

/// Information about a single model.
#[derive(Debug, Serialize)]
pub struct ModelInfoResponse {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
    pub agent_type: String,
    pub context_window: i64,
}

/// A group of models for a specific agent type.
#[derive(Debug, Serialize)]
pub struct ModelGroup {
    pub agent_type: String,
    pub section_title: String,
    pub icon: String,
    pub models: Vec<ModelInfoResponse>,
}

/// Response for listing all available models.
#[derive(Debug, Serialize)]
pub struct ListModelsResponse {
    pub groups: Vec<ModelGroup>,
}

/// List all available models grouped by agent type.
pub async fn list_models(
    State(state): State<WebAppState>,
) -> Result<Json<ListModelsResponse>, WebError> {
    let agent_types = [AgentType::Claude, AgentType::Codex, AgentType::Gemini];
    let core = state.core().await;
    let default_agent = core.config().default_agent;
    let default_model = core.config().default_model_for(default_agent);

    let groups: Vec<ModelGroup> = agent_types
        .iter()
        .map(|&agent_type| {
            let models = ModelRegistry::models_for(agent_type);
            let agent_type_str = format!("{:?}", agent_type).to_lowercase();

            ModelGroup {
                agent_type: agent_type_str.clone(),
                section_title: ModelRegistry::agent_section_title(agent_type).to_string(),
                icon: ModelRegistry::agent_icon(agent_type).to_string(),
                models: models
                    .into_iter()
                    .map(|m| {
                        let is_default = m.agent_type == default_agent && m.id == default_model;
                        ModelInfoResponse {
                            id: m.id,
                            display_name: m.display_name,
                            description: m.description,
                            is_default,
                            agent_type: agent_type_str.clone(),
                            context_window: m.context_window,
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(Json(ListModelsResponse { groups }))
}

#[derive(Debug, Deserialize)]
pub struct SetDefaultModelRequest {
    pub agent_type: String,
    pub model_id: String,
}

/// Update the default model selection for the web UI.
pub async fn set_default_model(
    State(state): State<WebAppState>,
    Json(payload): Json<SetDefaultModelRequest>,
) -> Result<StatusCode, WebError> {
    let agent_type = match payload.agent_type.to_lowercase().as_str() {
        "claude" => AgentType::Claude,
        "codex" => AgentType::Codex,
        "gemini" => AgentType::Gemini,
        _ => {
            return Err(WebError::BadRequest(format!(
                "Invalid agent type: {}. Must be one of: claude, codex, gemini",
                payload.agent_type
            )));
        }
    };

    if ModelRegistry::find_model(agent_type, &payload.model_id).is_none() {
        return Err(WebError::BadRequest(format!(
            "Invalid model '{}' for agent type {:?}",
            payload.model_id, agent_type
        )));
    }

    {
        let mut core = state.core_mut().await;
        core.config_mut()
            .set_default_model(agent_type, payload.model_id.clone());
    }

    save_default_model(agent_type, &payload.model_id)
        .map_err(|err| WebError::Internal(format!("Failed to save default model: {}", err)))?;

    Ok(StatusCode::NO_CONTENT)
}
