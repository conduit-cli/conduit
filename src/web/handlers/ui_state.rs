//! UI state handlers for the Conduit web API.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::data::AppStateStore;
use crate::web::error::WebError;
use crate::web::state::WebAppState;

const WEB_UI_STATE_KEY: &str = "web_ui_state";

/// UI state persisted for the web interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebUiState {
    pub active_session_id: Option<Uuid>,
    pub tab_order: Vec<Uuid>,
    pub sidebar_open: bool,
    pub last_workspace_id: Option<Uuid>,
}

impl Default for WebUiState {
    fn default() -> Self {
        Self {
            active_session_id: None,
            tab_order: Vec::new(),
            sidebar_open: true,
            last_workspace_id: None,
        }
    }
}

/// Request payload for updating UI state.
#[derive(Debug, Deserialize)]
pub struct UpdateWebUiStateRequest {
    pub active_session_id: Option<Option<Uuid>>,
    pub tab_order: Option<Vec<Uuid>>,
    pub sidebar_open: Option<bool>,
    pub last_workspace_id: Option<Option<Uuid>>,
}

/// Response payload for UI state.
#[derive(Debug, Serialize)]
pub struct WebUiStateResponse {
    pub active_session_id: Option<Uuid>,
    pub tab_order: Vec<Uuid>,
    pub sidebar_open: bool,
    pub last_workspace_id: Option<Uuid>,
}

impl From<WebUiState> for WebUiStateResponse {
    fn from(state: WebUiState) -> Self {
        Self {
            active_session_id: state.active_session_id,
            tab_order: state.tab_order,
            sidebar_open: state.sidebar_open,
            last_workspace_id: state.last_workspace_id,
        }
    }
}

fn state_store(core: &crate::core::ConduitCore) -> Result<&AppStateStore, WebError> {
    core.app_state_store()
        .ok_or_else(|| WebError::Internal("Database not available".to_string()))
}

fn load_ui_state(store: &AppStateStore) -> Result<WebUiState, WebError> {
    let raw = store.get(WEB_UI_STATE_KEY)?;
    match raw {
        Some(value) => match serde_json::from_str::<WebUiState>(&value) {
            Ok(state) => Ok(state),
            Err(err) => {
                tracing::warn!(error = %err, "Failed to parse web UI state; resetting to default");
                Ok(WebUiState::default())
            }
        },
        None => Ok(WebUiState::default()),
    }
}

fn save_ui_state(store: &AppStateStore, state: &WebUiState) -> Result<(), WebError> {
    let serialized = serde_json::to_string(state)
        .map_err(|err| WebError::Internal(format!("Failed to serialize UI state: {}", err)))?;
    store.set(WEB_UI_STATE_KEY, &serialized)?;
    Ok(())
}

fn dedupe_tab_order(order: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = std::collections::HashSet::new();
    order.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// Get the persisted UI state.
pub async fn get_ui_state(
    State(state): State<WebAppState>,
) -> Result<Json<WebUiStateResponse>, WebError> {
    let core = state.core().await;
    let store = state_store(&core)?;
    let ui_state = load_ui_state(store)?;
    Ok(Json(WebUiStateResponse::from(ui_state)))
}

/// Update the persisted UI state.
pub async fn update_ui_state(
    State(state): State<WebAppState>,
    Json(payload): Json<UpdateWebUiStateRequest>,
) -> Result<Json<WebUiStateResponse>, WebError> {
    let core = state.core().await;
    let store = state_store(&core)?;
    let mut ui_state = load_ui_state(store)?;

    if let Some(active_session_id) = payload.active_session_id {
        ui_state.active_session_id = active_session_id;
    }

    if let Some(tab_order) = payload.tab_order {
        ui_state.tab_order = dedupe_tab_order(tab_order);
    }

    if let Some(sidebar_open) = payload.sidebar_open {
        ui_state.sidebar_open = sidebar_open;
    }

    if let Some(last_workspace_id) = payload.last_workspace_id {
        ui_state.last_workspace_id = last_workspace_id;
    }

    save_ui_state(store, &ui_state)?;

    Ok(Json(WebUiStateResponse::from(ui_state)))
}
