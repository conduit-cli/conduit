//! Model configuration and registry

use std::sync::{OnceLock, RwLock};

use crate::agent::opencode::load_opencode_models;
use crate::agent::AgentType;

/// Information about a model
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Internal model ID (passed to CLI)
    pub id: String,
    /// Display name for UI
    pub display_name: String,
    /// Short alias for quick selection
    pub alias: String,
    /// Description of model capabilities
    pub description: String,
    /// Whether this is the default model for the agent type
    pub is_default: bool,
    /// Agent type this model belongs to
    pub agent_type: AgentType,
    /// Maximum context window in tokens
    pub context_window: i64,
}

impl ModelInfo {
    pub fn new(
        agent_type: AgentType,
        id: &str,
        display_name: &str,
        alias: &str,
        description: &str,
        context_window: i64,
    ) -> Self {
        Self {
            id: id.to_string(),
            display_name: display_name.to_string(),
            alias: alias.to_string(),
            description: description.to_string(),
            is_default: false,
            agent_type,
            context_window,
        }
    }

    pub fn as_default(mut self) -> Self {
        self.is_default = true;
        self
    }
}

/// Registry of available models for each agent type
#[derive(Debug, Default)]
pub struct ModelRegistry;

impl ModelRegistry {
    /// Default context window for Claude models (200K tokens)
    pub const CLAUDE_CONTEXT_WINDOW: i64 = 200_000;

    /// Default context window for Codex models (272K tokens)
    pub const CODEX_CONTEXT_WINDOW: i64 = 272_000;

    /// Default context window for Gemini models (approximate)
    pub const GEMINI_CONTEXT_WINDOW: i64 = 1_000_000;

    /// Default context window for OpenCode models (approximate)
    pub const OPENCODE_CONTEXT_WINDOW: i64 = 200_000;

    const OPENCODE_DEFAULT_MODEL_ID: &'static str = "default";

    fn opencode_store() -> &'static RwLock<Vec<ModelInfo>> {
        static OPENCODE_MODELS: OnceLock<RwLock<Vec<ModelInfo>>> = OnceLock::new();
        OPENCODE_MODELS.get_or_init(|| RwLock::new(Vec::new()))
    }

    fn opencode_default_model() -> ModelInfo {
        ModelInfo::new(
            AgentType::Opencode,
            Self::OPENCODE_DEFAULT_MODEL_ID,
            "OpenCode Default",
            Self::OPENCODE_DEFAULT_MODEL_ID,
            "Use OpenCode's default model selection",
            Self::OPENCODE_CONTEXT_WINDOW,
        )
        .as_default()
    }

    fn build_opencode_models(model_ids: Vec<String>) -> Vec<ModelInfo> {
        let mut models = vec![Self::opencode_default_model()];
        for id in model_ids {
            if id == Self::OPENCODE_DEFAULT_MODEL_ID {
                continue;
            }
            models.push(ModelInfo::new(
                AgentType::Opencode,
                &id,
                &id,
                &id,
                "OpenCode model",
                Self::OPENCODE_CONTEXT_WINDOW,
            ));
        }
        models
    }

    pub fn set_opencode_models(model_ids: Vec<String>) {
        let mut models = Self::build_opencode_models(model_ids);
        models.sort_by(|a, b| a.id.cmp(&b.id));
        models.dedup_by(|a, b| a.id == b.id);
        if let Some(pos) = models
            .iter()
            .position(|model| model.id == Self::OPENCODE_DEFAULT_MODEL_ID)
        {
            let default = models.remove(pos);
            models.insert(0, default);
        }
        let mut store = Self::opencode_store()
            .write()
            .unwrap_or_else(|err| err.into_inner());
        *store = models;
    }

    pub fn clear_opencode_models() {
        let mut store = Self::opencode_store()
            .write()
            .unwrap_or_else(|err| err.into_inner());
        store.clear();
    }

    pub fn refresh_opencode_models() {
        let models = load_opencode_models(None);
        if models.is_empty() {
            return;
        }
        Self::set_opencode_models(models);
    }

    pub fn opencode_models() -> Vec<ModelInfo> {
        Self::opencode_store()
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Get available models for Claude Code
    pub fn claude_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo::new(
                AgentType::Claude,
                "opus",
                "Opus 4.5",
                "opus",
                "Most powerful, best for complex reasoning",
                Self::CLAUDE_CONTEXT_WINDOW,
            )
            .as_default(),
            ModelInfo::new(
                AgentType::Claude,
                "sonnet",
                "Sonnet 4.5",
                "sonnet",
                "Fast and capable, best for most tasks",
                Self::CLAUDE_CONTEXT_WINDOW,
            ),
            ModelInfo::new(
                AgentType::Claude,
                "haiku",
                "Haiku 4.5",
                "haiku",
                "Fastest, great for simple tasks",
                Self::CLAUDE_CONTEXT_WINDOW,
            ),
        ]
    }

    /// Get available models for Codex CLI
    pub fn codex_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo::new(
                AgentType::Codex,
                "gpt-5.2-codex",
                "GPT-5.2-Codex",
                "gpt-5.2-codex",
                "Latest Codex model",
                Self::CODEX_CONTEXT_WINDOW,
            )
            .as_default(),
            ModelInfo::new(
                AgentType::Codex,
                "gpt-5.2",
                "GPT-5.2",
                "gpt-5.2",
                "Fast and efficient",
                Self::CODEX_CONTEXT_WINDOW,
            ),
            ModelInfo::new(
                AgentType::Codex,
                "gpt-5.1-codex-max",
                "GPT-5.1-Codex-Max",
                "gpt-5.1-codex-max",
                "Maximum capability",
                Self::CODEX_CONTEXT_WINDOW,
            ),
        ]
    }

    /// Get available models for Gemini CLI
    pub fn gemini_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo::new(
                AgentType::Gemini,
                "gemini-2.5-pro",
                "Gemini 2.5 Pro",
                "gemini-2.5-pro",
                "Highest quality Gemini model",
                Self::GEMINI_CONTEXT_WINDOW,
            )
            .as_default(),
            ModelInfo::new(
                AgentType::Gemini,
                "gemini-2.5-flash",
                "Gemini 2.5 Flash",
                "gemini-2.5-flash",
                "Fast and capable Gemini model",
                Self::GEMINI_CONTEXT_WINDOW,
            ),
            ModelInfo::new(
                AgentType::Gemini,
                "gemini-2.5-flash-lite",
                "Gemini 2.5 Flash Lite",
                "gemini-2.5-flash-lite",
                "Lowest-latency Gemini model",
                Self::GEMINI_CONTEXT_WINDOW,
            ),
            ModelInfo::new(
                AgentType::Gemini,
                "gemini-3-pro-preview",
                "Gemini 3 Pro Preview",
                "gemini-3-pro-preview",
                "Preview Gemini 3 model",
                Self::GEMINI_CONTEXT_WINDOW,
            ),
            ModelInfo::new(
                AgentType::Gemini,
                "gemini-3-flash-preview",
                "Gemini 3 Flash Preview",
                "gemini-3-flash-preview",
                "Preview Gemini 3 flash model",
                Self::GEMINI_CONTEXT_WINDOW,
            ),
        ]
    }

    /// Get all models grouped by agent type
    pub fn all_models() -> Vec<ModelInfo> {
        let mut models = Self::claude_models();
        models.extend(Self::codex_models());
        models.extend(Self::gemini_models());
        models.extend(Self::opencode_models());
        models
    }

    /// Get models for a specific agent type
    pub fn models_for(agent_type: AgentType) -> Vec<ModelInfo> {
        match agent_type {
            AgentType::Claude => Self::claude_models(),
            AgentType::Codex => Self::codex_models(),
            AgentType::Gemini => Self::gemini_models(),
            AgentType::Opencode => Self::opencode_models(),
        }
    }

    /// Get the default model for an agent type
    pub fn default_model(agent_type: AgentType) -> String {
        match agent_type {
            AgentType::Claude => "opus".to_string(),
            AgentType::Codex => "gpt-5.2-codex".to_string(),
            AgentType::Gemini => "gemini-2.5-pro".to_string(),
            AgentType::Opencode => Self::OPENCODE_DEFAULT_MODEL_ID.to_string(),
        }
    }

    /// Find a model by ID or alias
    pub fn find_model(agent_type: AgentType, id_or_alias: &str) -> Option<ModelInfo> {
        if agent_type == AgentType::Opencode {
            let trimmed = id_or_alias.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(model) = Self::opencode_models()
                .into_iter()
                .find(|m| m.id == trimmed || m.alias == trimmed)
            {
                return Some(model);
            }
            return Some(ModelInfo::new(
                AgentType::Opencode,
                trimmed,
                trimmed,
                trimmed,
                "OpenCode model",
                Self::OPENCODE_CONTEXT_WINDOW,
            ));
        }

        Self::models_for(agent_type)
            .into_iter()
            .find(|m| m.id == id_or_alias || m.alias == id_or_alias)
    }

    /// Get the icon for an agent type
    pub fn agent_icon(agent_type: AgentType) -> &'static str {
        match agent_type {
            AgentType::Claude => "✻",
            AgentType::Codex => "◎",
            AgentType::Gemini => "◆",
            AgentType::Opencode => "◍",
        }
    }

    /// Get the section title for an agent type
    pub fn agent_section_title(agent_type: AgentType) -> &'static str {
        match agent_type {
            AgentType::Claude => "Claude Code",
            AgentType::Codex => "Codex",
            AgentType::Gemini => "Gemini",
            AgentType::Opencode => "OpenCode",
        }
    }

    /// Get context window limit for a specific model
    pub fn context_window(agent_type: AgentType, model_id: &str) -> i64 {
        Self::find_model(agent_type, model_id)
            .map(|m| m.context_window)
            .unwrap_or_else(|| Self::default_context_window(agent_type))
    }

    /// Default context window when model not found
    pub fn default_context_window(agent_type: AgentType) -> i64 {
        match agent_type {
            AgentType::Claude => Self::CLAUDE_CONTEXT_WINDOW,
            AgentType::Codex => Self::CODEX_CONTEXT_WINDOW,
            AgentType::Gemini => Self::GEMINI_CONTEXT_WINDOW,
            AgentType::Opencode => Self::OPENCODE_CONTEXT_WINDOW,
        }
    }
}
