//! Models handler for the Conduit web API.

use axum::Json;
use serde::Serialize;

use crate::agent::{AgentType, ModelRegistry};
use crate::web::error::WebError;

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
pub async fn list_models() -> Result<Json<ListModelsResponse>, WebError> {
    let agent_types = [AgentType::Claude, AgentType::Codex, AgentType::Gemini];

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
                    .map(|m| ModelInfoResponse {
                        id: m.id,
                        display_name: m.display_name,
                        description: m.description,
                        is_default: m.is_default,
                        agent_type: agent_type_str.clone(),
                        context_window: m.context_window,
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(Json(ListModelsResponse { groups }))
}
