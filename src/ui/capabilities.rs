use crate::agent::AgentType;

#[derive(Debug, Clone, Copy)]
pub struct AgentCapabilities {
    pub supports_plan_mode: bool,
    pub supports_interactive_input: bool,
    pub supports_steer: bool,
    pub supports_follow_up: bool,
    pub supports_native_slash_commands: bool,
    pub supports_direct_user_skill_invocation: bool,
    pub supports_native_skill_tool: bool,
    pub supports_command_template_expansion: bool,
}

impl AgentCapabilities {
    pub fn for_agent(agent_type: AgentType) -> Self {
        Self {
            supports_plan_mode: agent_type.supports_plan_mode(),
            supports_interactive_input: false,
            supports_steer: false,
            supports_follow_up: false,
            supports_native_slash_commands: matches!(
                agent_type,
                AgentType::Claude | AgentType::Gemini | AgentType::Opencode
            ),
            supports_direct_user_skill_invocation: matches!(
                agent_type,
                AgentType::Codex | AgentType::Claude
            ),
            supports_native_skill_tool: matches!(
                agent_type,
                AgentType::Gemini | AgentType::Opencode
            ),
            supports_command_template_expansion: true,
        }
    }
}
