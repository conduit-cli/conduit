//! Models handler for the Conduit web API.

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

use crate::agent::AgentType;
use crate::core::dto::ListModelsDto;
use crate::core::services::{ConfigService, ModelService, ServiceError};
use crate::web::error::WebError;
use crate::web::state::WebAppState;

/// List all available models grouped by agent type.
pub async fn list_models(
    State(state): State<WebAppState>,
) -> Result<Json<ListModelsDto>, WebError> {
    let core = state.core().await;
    Ok(Json(ModelService::list_models(&core)))
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

    {
        let mut core = state.core_mut().await;
        ConfigService::set_default_model(&mut core, agent_type, &payload.model_id)
            .map_err(|err| map_service_error(err))?;
    }

    Ok(StatusCode::NO_CONTENT)
}

fn map_service_error(error: ServiceError) -> WebError {
    match error {
        ServiceError::InvalidInput(message) => WebError::BadRequest(message),
        ServiceError::NotFound(message) => WebError::NotFound(message),
        ServiceError::Internal(message) => WebError::Internal(message),
    }
}
