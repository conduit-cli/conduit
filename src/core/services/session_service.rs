use uuid::Uuid;

use crate::agent::{AgentMode, AgentType, ModelRegistry};
use crate::core::services::error::ServiceError;
use crate::core::ConduitCore;
use crate::data::SessionTab;

#[derive(Debug, Clone)]
pub struct CreateSessionParams {
    pub workspace_id: Option<Uuid>,
    pub agent_type: AgentType,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateSessionParams {
    pub model: Option<String>,
    pub agent_type: Option<AgentType>,
    pub agent_mode: Option<AgentMode>,
}

pub struct SessionService;

impl SessionService {
    pub fn list_sessions(core: &ConduitCore) -> Result<Vec<SessionTab>, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let sessions = store
            .get_all()
            .map_err(|e| ServiceError::Internal(format!("Failed to list sessions: {}", e)))?;

        sessions
            .into_iter()
            .map(|session| Self::ensure_model(core, store, session))
            .collect()
    }

    pub fn get_session(core: &ConduitCore, id: Uuid) -> Result<SessionTab, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        Self::ensure_model(core, store, session)
    }

    pub fn create_session(
        core: &ConduitCore,
        params: CreateSessionParams,
    ) -> Result<SessionTab, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;

        let sessions = store
            .get_all()
            .map_err(|e| ServiceError::Internal(format!("Failed to list sessions: {}", e)))?;

        let next_index = sessions.iter().map(|s| s.tab_index).max().unwrap_or(-1) + 1;

        let model = if let Some(model_id) = params.model {
            if ModelRegistry::find_model(params.agent_type, &model_id).is_none() {
                return Err(ServiceError::InvalidInput(format!(
                    "Invalid model '{}' for agent type {:?}",
                    model_id, params.agent_type
                )));
            }
            Some(model_id)
        } else {
            Some(core.config().default_model_for(params.agent_type))
        };

        let session = SessionTab::new(
            next_index,
            params.agent_type,
            params.workspace_id,
            None,
            model,
            None,
        );

        store
            .create(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to create session: {}", e)))?;

        Ok(session)
    }

    pub fn update_session(
        core: &ConduitCore,
        id: Uuid,
        params: UpdateSessionParams,
    ) -> Result<SessionTab, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let mut session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        if session.agent_session_id.is_some()
            && (params.model.is_some()
                || params.agent_type.is_some()
                || params.agent_mode.is_some())
        {
            return Err(ServiceError::InvalidInput(
                "Cannot change session settings while a run is active".to_string(),
            ));
        }

        let mut agent_type_changed = false;
        if let Some(agent_type) = params.agent_type {
            agent_type_changed = session.agent_type != agent_type;
            session.agent_type = agent_type;
        }

        if let Some(agent_mode) = params.agent_mode {
            if session.agent_type != AgentType::Claude {
                return Err(ServiceError::InvalidInput(
                    "Agent mode is only supported for Claude sessions".to_string(),
                ));
            }
            session.agent_mode = Some(agent_mode.as_str().to_string());
        }

        if let Some(model_id) = params.model {
            if ModelRegistry::find_model(session.agent_type, &model_id).is_none() {
                return Err(ServiceError::InvalidInput(format!(
                    "Invalid model '{}' for agent type {:?}",
                    model_id, session.agent_type
                )));
            }
            session.model = Some(model_id);
        } else if agent_type_changed {
            session.model = Some(core.config().default_model_for(session.agent_type));
        }

        store
            .update(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to update session: {}", e)))?;

        Ok(session)
    }

    pub fn close_session(core: &ConduitCore, id: Uuid) -> Result<(), ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;

        store
            .delete(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to close session: {}", e)))?;

        Ok(())
    }

    pub fn get_or_create_session_for_workspace(
        core: &ConduitCore,
        workspace_id: Uuid,
    ) -> Result<SessionTab, ServiceError> {
        let session_store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;

        if let Some(existing) = session_store
            .get_by_workspace_id(workspace_id)
            .map_err(|e| ServiceError::Internal(format!("Failed to query session: {}", e)))?
        {
            return Self::ensure_model(core, session_store, existing);
        }

        let workspace_store = core
            .workspace_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;

        workspace_store
            .get_by_id(workspace_id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get workspace: {}", e)))?
            .ok_or_else(|| {
                ServiceError::NotFound(format!("Workspace {} not found", workspace_id))
            })?;

        let sessions = session_store
            .get_all()
            .map_err(|e| ServiceError::Internal(format!("Failed to list sessions: {}", e)))?;
        let next_index = sessions.iter().map(|s| s.tab_index).max().unwrap_or(-1) + 1;

        let default_agent = core.config().default_agent;
        let session = SessionTab::new(
            next_index,
            default_agent,
            Some(workspace_id),
            None,
            Some(core.config().default_model_for(default_agent)),
            None,
        );

        session_store
            .create(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to create session: {}", e)))?;

        Ok(session)
    }

    fn ensure_model(
        core: &ConduitCore,
        store: &crate::data::SessionTabStore,
        mut session: SessionTab,
    ) -> Result<SessionTab, ServiceError> {
        if session.model.is_some() {
            return Ok(session);
        }

        session.model = Some(core.config().default_model_for(session.agent_type));
        store.update(&session).map_err(|e| {
            ServiceError::Internal(format!("Failed to update session model: {}", e))
        })?;
        Ok(session)
    }
}
