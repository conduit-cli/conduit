use chrono::Utc;
use uuid::Uuid;

use crate::agent::{AgentMode, AgentType, ModelRegistry};
use crate::core::services::error::ServiceError;
use crate::core::ConduitCore;
use crate::data::{QueuedImageAttachment, QueuedMessage, QueuedMessageMode, SessionTab};

const INPUT_HISTORY_MAX: usize = 1000;

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

#[derive(Debug, Clone)]
pub struct CreateImportedSessionParams {
    pub workspace_id: Option<Uuid>,
    pub agent_type: AgentType,
    pub agent_session_id: String,
    pub title: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateForkedSessionParams {
    pub workspace_id: Uuid,
    pub agent_type: AgentType,
    pub agent_mode: Option<AgentMode>,
    pub model: Option<String>,
    pub fork_seed_id: Uuid,
}

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

        let session = SessionTab::new(0, params.agent_type, params.workspace_id, None, model, None);

        let session = store
            .create_with_next_index(session)
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
        if agent_type_changed && session.agent_type != AgentType::Claude {
            session.agent_mode = None;
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
            .set_open(id, false)
            .map_err(|e| ServiceError::Internal(format!("Failed to close session: {}", e)))?;

        Ok(())
    }

    pub fn create_imported_session(
        core: &ConduitCore,
        params: CreateImportedSessionParams,
    ) -> Result<SessionTab, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;

        let model = params
            .model
            .or_else(|| Some(core.config().default_model_for(params.agent_type)));

        let mut session = SessionTab::new(
            0,
            params.agent_type,
            params.workspace_id,
            Some(params.agent_session_id),
            model,
            None,
        );
        session.title = params.title;
        session.title_generated = false;

        let session = store
            .create_with_next_index(session)
            .map_err(|e| ServiceError::Internal(format!("Failed to create session: {}", e)))?;

        Ok(session)
    }

    pub fn create_forked_session(
        core: &ConduitCore,
        params: CreateForkedSessionParams,
    ) -> Result<SessionTab, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;

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

        let mut session = SessionTab::new(
            0,
            params.agent_type,
            Some(params.workspace_id),
            None,
            model,
            None,
        );
        if let Some(mode) = params.agent_mode {
            if params.agent_type != AgentType::Claude {
                return Err(ServiceError::InvalidInput(
                    "Agent mode is only supported for Claude sessions".to_string(),
                ));
            }
            session.agent_mode = Some(mode.as_str().to_string());
        }
        session.fork_seed_id = Some(params.fork_seed_id);

        let session = store
            .create_with_next_index(session)
            .map_err(|e| ServiceError::Internal(format!("Failed to create session: {}", e)))?;

        Ok(session)
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
            if !existing.is_open {
                let mut reopened = existing.clone();
                reopened.is_open = true;
                reopened.tab_index = session_store.next_tab_index().map_err(|e| {
                    ServiceError::Internal(format!("Failed to allocate tab index: {}", e))
                })?;
                session_store.update(&reopened).map_err(|e| {
                    ServiceError::Internal(format!("Failed to reopen session: {}", e))
                })?;
                return Self::ensure_model(core, session_store, reopened);
            }
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

        let default_agent = core.config().default_agent;
        let session = SessionTab::new(
            0,
            default_agent,
            Some(workspace_id),
            None,
            Some(core.config().default_model_for(default_agent)),
            None,
        );

        let session = session_store
            .create_with_next_index(session)
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

    pub fn list_queue(core: &ConduitCore, id: Uuid) -> Result<Vec<QueuedMessage>, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        Ok(session.queued_messages)
    }

    pub fn add_queue_message(
        core: &ConduitCore,
        id: Uuid,
        mode: QueuedMessageMode,
        text: String,
        images: Vec<QueuedImageAttachment>,
    ) -> Result<QueuedMessage, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let mut session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        let message = QueuedMessage {
            id: Uuid::new_v4(),
            mode,
            text,
            images,
            created_at: Utc::now(),
        };

        session.queued_messages.push(message.clone());
        store
            .update(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to update session: {}", e)))?;

        Ok(message)
    }

    pub fn update_queue_message(
        core: &ConduitCore,
        id: Uuid,
        message_id: Uuid,
        text: Option<String>,
        mode: Option<QueuedMessageMode>,
        position: Option<usize>,
    ) -> Result<QueuedMessage, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let mut session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        let idx = session
            .queued_messages
            .iter()
            .position(|msg| msg.id == message_id)
            .ok_or_else(|| {
                ServiceError::NotFound(format!("Queued message {} not found", message_id))
            })?;

        if let Some(text) = text {
            session.queued_messages[idx].text = text;
        }
        if let Some(mode) = mode {
            session.queued_messages[idx].mode = mode;
        }

        if let Some(position) = position {
            let message = session.queued_messages.remove(idx);
            let insert_at = position.min(session.queued_messages.len());
            session.queued_messages.insert(insert_at, message);
        }

        let updated = session
            .queued_messages
            .iter()
            .find(|msg| msg.id == message_id)
            .cloned()
            .ok_or_else(|| {
                ServiceError::NotFound(format!("Queued message {} not found", message_id))
            })?;

        store
            .update(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to update session: {}", e)))?;

        Ok(updated)
    }

    pub fn remove_queue_message(
        core: &ConduitCore,
        id: Uuid,
        message_id: Uuid,
    ) -> Result<QueuedMessage, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let mut session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        let idx = session
            .queued_messages
            .iter()
            .position(|msg| msg.id == message_id)
            .ok_or_else(|| {
                ServiceError::NotFound(format!("Queued message {} not found", message_id))
            })?;

        let removed = session.queued_messages.remove(idx);
        store
            .update(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to update session: {}", e)))?;

        Ok(removed)
    }

    pub fn get_input_history(core: &ConduitCore, id: Uuid) -> Result<Vec<String>, ServiceError> {
        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        Ok(session.input_history)
    }

    pub fn append_input_history(
        core: &ConduitCore,
        id: Uuid,
        input: &str,
    ) -> Result<Vec<String>, ServiceError> {
        let trimmed = input.trim_end();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let store = core
            .session_tab_store()
            .ok_or_else(|| ServiceError::Internal("Database not available".to_string()))?;
        let mut session = store
            .get_by_id(id)
            .map_err(|e| ServiceError::Internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| ServiceError::NotFound(format!("Session {} not found", id)))?;

        session.input_history.push(trimmed.to_string());
        if session.input_history.len() > INPUT_HISTORY_MAX {
            let overflow = session.input_history.len() - INPUT_HISTORY_MAX;
            session.input_history.drain(0..overflow);
        }

        store
            .update(&session)
            .map_err(|e| ServiceError::Internal(format!("Failed to update session: {}", e)))?;

        Ok(session.input_history)
    }
}
