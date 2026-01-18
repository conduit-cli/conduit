use crate::agent::{AgentType, ModelRegistry};
use crate::core::dto::{ListModelsDto, ModelGroupDto, ModelInfoDto};
use crate::core::services::config_service::ConfigService;
use crate::core::ConduitCore;

pub struct ModelService;

impl ModelService {
    pub fn list_models(core: &ConduitCore) -> ListModelsDto {
        let agent_types = [AgentType::Claude, AgentType::Codex, AgentType::Gemini];
        let (default_agent, default_model) = ConfigService::default_model(core);

        let groups = agent_types
            .iter()
            .map(|&agent_type| {
                let models = ModelRegistry::models_for(agent_type);
                let agent_type_str = agent_type.as_str().to_string();

                let model_entries = models
                    .into_iter()
                    .map(|model| {
                        let is_default =
                            model.agent_type == default_agent && model.id == default_model;
                        ModelInfoDto {
                            id: model.id,
                            display_name: model.display_name,
                            description: model.description,
                            is_default,
                            agent_type: agent_type_str.clone(),
                            context_window: model.context_window,
                        }
                    })
                    .collect();

                ModelGroupDto {
                    agent_type: agent_type_str,
                    section_title: ModelRegistry::agent_section_title(agent_type).to_string(),
                    icon: ModelRegistry::agent_icon(agent_type).to_string(),
                    models: model_entries,
                }
            })
            .collect();

        ListModelsDto { groups }
    }
}
