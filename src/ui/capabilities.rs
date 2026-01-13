use crate::agent::AgentType;

#[derive(Debug, Clone, Copy)]
pub struct AgentCapabilities {
    pub supports_plan_mode: bool,
    pub supports_interactive_input: bool,
    pub supports_steer: bool,
    pub supports_follow_up: bool,
}

impl AgentCapabilities {
    pub fn for_agent(agent_type: AgentType) -> Self {
        match agent_type {
            AgentType::Claude => Self {
                supports_plan_mode: true,
                supports_interactive_input: false,
                supports_steer: false,
                supports_follow_up: false,
            },
            AgentType::Codex => Self {
                supports_plan_mode: false,
                supports_interactive_input: false,
                supports_steer: false,
                supports_follow_up: false,
            },
            AgentType::Gemini => Self {
                supports_plan_mode: false,
                supports_interactive_input: false,
                supports_steer: false,
                supports_follow_up: false,
            },
        }
    }
}
