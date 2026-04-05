use crate::agent::{AgentType, ModelRegistry};
use crate::config::{save_default_model, save_enabled_providers, save_workspaces_config};
use crate::core::services::error::ServiceError;
use crate::core::ConduitCore;
use crate::git::WorkspaceMode;

pub struct ConfigService;

impl ConfigService {
    pub fn default_model(core: &ConduitCore) -> (AgentType, String) {
        let default = core.config().default_agent;
        let agent = if core
            .config()
            .is_provider_enabled_effective(default, core.tools())
        {
            default
        } else {
            core.config()
                .effective_enabled_providers(core.tools())
                .into_iter()
                .next()
                .unwrap_or(default)
        };
        let model = core.config().default_model_for(agent);
        (agent, model)
    }

    pub fn set_default_model(
        core: &mut ConduitCore,
        agent_type: AgentType,
        model_id: &str,
    ) -> Result<(), ServiceError> {
        if !core
            .config()
            .is_provider_enabled_effective(agent_type, core.tools())
        {
            return Err(ServiceError::InvalidInput(format!(
                "{} is not enabled. Use /providers first.",
                agent_type.display_name()
            )));
        }

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

    pub fn set_enabled_providers(
        core: &mut ConduitCore,
        providers: Vec<AgentType>,
    ) -> Result<(), ServiceError> {
        if providers.is_empty() {
            return Err(ServiceError::InvalidInput(
                "At least one provider must be enabled".to_string(),
            ));
        }

        let effective = providers
            .iter()
            .copied()
            .filter(|provider| {
                core.tools().is_available(match provider {
                    AgentType::Claude => crate::util::Tool::Claude,
                    AgentType::Codex => crate::util::Tool::Codex,
                    AgentType::Gemini => crate::util::Tool::Gemini,
                    AgentType::Opencode => crate::util::Tool::Opencode,
                    AgentType::Pi => crate::util::Tool::Pi,
                })
            })
            .count();

        if effective == 0 {
            return Err(ServiceError::InvalidInput(
                "No enabled providers are currently installed".to_string(),
            ));
        }

        save_enabled_providers(&providers).map_err(|err| {
            ServiceError::Internal(format!("Failed to save enabled providers: {err}"))
        })?;
        core.config_mut().set_enabled_providers(providers);
        Ok(())
    }

    pub fn set_workspace_defaults(
        core: &mut ConduitCore,
        mode: WorkspaceMode,
        archive_delete_branch: bool,
        archive_remote_prompt: bool,
    ) -> Result<(), ServiceError> {
        save_workspaces_config(mode, archive_delete_branch, archive_remote_prompt).map_err(
            |err| ServiceError::Internal(format!("Failed to save workspace defaults: {err}")),
        )?;

        core.config_mut().set_workspace_defaults(
            mode,
            archive_delete_branch,
            archive_remote_prompt,
        );
        Ok(())
    }
}
