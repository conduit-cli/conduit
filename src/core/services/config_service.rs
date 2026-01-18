use crate::agent::{AgentType, ModelRegistry};
use crate::config::save_default_model;
use crate::core::services::error::ServiceError;
use crate::core::ConduitCore;

pub struct ConfigService;

impl ConfigService {
    pub fn default_model(core: &ConduitCore) -> (AgentType, String) {
        let agent = core.config().default_agent;
        let model = core.config().default_model_for(agent);
        (agent, model)
    }

    pub fn set_default_model(
        core: &mut ConduitCore,
        agent_type: AgentType,
        model_id: &str,
    ) -> Result<(), ServiceError> {
        if ModelRegistry::find_model(agent_type, model_id).is_none() {
            return Err(ServiceError::InvalidInput(format!(
                "Invalid model '{}' for agent type {:?}",
                model_id, agent_type
            )));
        }

        core.config_mut()
            .set_default_model(agent_type, model_id.to_string());

        save_default_model(agent_type, model_id).map_err(|err| {
            ServiceError::Internal(format!("Failed to save default model: {err}"))
        })?;

        Ok(())
    }
}
