use crate::agent::{AgentType, ModelRegistry};
use crate::core::ConduitCore;

const CONTEXT_WINDOW_KEY_PREFIX: &str = "model_context_window";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWindowSource {
    ObservedRuntime,
    ModelCatalog,
    FallbackDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedContextWindow {
    pub tokens: i64,
    pub source: ContextWindowSource,
}

pub struct ContextWindowService;

impl ContextWindowService {
    pub fn resolve(
        core: &ConduitCore,
        agent_type: AgentType,
        model_id: &str,
    ) -> ResolvedContextWindow {
        if let Some(observed) = Self::get_observed(core, agent_type, model_id) {
            return ResolvedContextWindow {
                tokens: observed,
                source: ContextWindowSource::ObservedRuntime,
            };
        }

        if let Some(provider_window) = Self::provider_context_window(agent_type, model_id) {
            return ResolvedContextWindow {
                tokens: provider_window,
                source: ContextWindowSource::ModelCatalog,
            };
        }

        ResolvedContextWindow {
            tokens: ModelRegistry::default_context_window(agent_type),
            source: ContextWindowSource::FallbackDefault,
        }
    }

    pub fn record_observed(
        core: &ConduitCore,
        agent_type: AgentType,
        model_id: &str,
        context_window: i64,
    ) {
        if context_window <= 0 {
            return;
        }

        let Some(store) = core.app_state_store() else {
            return;
        };

        let key = Self::key(agent_type, model_id);
        if let Err(err) = store.set(&key, &context_window.to_string()) {
            tracing::warn!(
                error = %err,
                key = %key,
                "Failed to persist observed model context window"
            );
        }
    }

    fn get_observed(core: &ConduitCore, agent_type: AgentType, model_id: &str) -> Option<i64> {
        let store = core.app_state_store()?;
        let key = Self::key(agent_type, model_id);
        let raw = match store.get(&key) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    key = %key,
                    "Failed to load observed model context window"
                );
                None
            }
        }?;

        match raw.parse::<i64>() {
            Ok(tokens) if tokens > 0 => Some(tokens),
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    key = %key,
                    value = %raw,
                    "Invalid observed model context window value"
                );
                None
            }
        }
    }

    fn provider_context_window(agent_type: AgentType, model_id: &str) -> Option<i64> {
        // OpenCode can expose dynamic provider/model IDs; prefer inference first.
        if agent_type == AgentType::Opencode {
            if let Some(inferred) = Self::infer_opencode_context_window(model_id) {
                return Some(inferred);
            }
        }

        if let Some(model) = ModelRegistry::find_model(agent_type, model_id) {
            return Some(model.context_window);
        }

        None
    }

    fn infer_opencode_context_window(model_id: &str) -> Option<i64> {
        let (provider, underlying_model) = model_id.split_once('/')?;
        match provider {
            "openai" => Self::openai_context_window(underlying_model),
            "anthropic" => {
                if underlying_model.contains("1m") {
                    Some(ModelRegistry::CLAUDE_1M_CONTEXT_WINDOW)
                } else {
                    Some(ModelRegistry::CLAUDE_CONTEXT_WINDOW)
                }
            }
            "google" => Some(ModelRegistry::GEMINI_CONTEXT_WINDOW),
            _ => None,
        }
    }

    fn openai_context_window(model_id: &str) -> Option<i64> {
        match model_id {
            "gpt-5.4" => Some(ModelRegistry::CODEX_CONTEXT_WINDOW),
            "gpt-5.3-codex" => Some(ModelRegistry::CODEX_GPT53_CONTEXT_WINDOW),
            "gpt-5.3-codex-spark" => Some(ModelRegistry::CODEX_GPT53_SPARK_CONTEXT_WINDOW),
            "gpt-5.2-codex" | "gpt-5.1-codex-max" | "gpt-5.2" | "gpt-5.1-codex-mini" => {
                Some(ModelRegistry::CODEX_CONTEXT_WINDOW)
            }
            _ => None,
        }
    }

    fn key(agent_type: AgentType, model_id: &str) -> String {
        format!(
            "{}::{}::{}",
            CONTEXT_WINDOW_KEY_PREFIX,
            agent_type.as_str(),
            model_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::util::{self, ToolAvailability};
    use std::path::PathBuf;
    use std::sync::OnceLock;

    fn init_test_data_dir() -> PathBuf {
        static TEST_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
        TEST_DATA_DIR
            .get_or_init(|| {
                let dir = tempfile::Builder::new()
                    .prefix("context-window-service-test-")
                    .tempdir()
                    .expect("tempdir");
                let path = dir.path().to_path_buf();
                std::mem::forget(dir);
                util::init_data_dir(Some(path.clone()));
                path
            })
            .clone()
    }

    fn build_core() -> ConduitCore {
        init_test_data_dir();
        ConduitCore::new(Config::default(), ToolAvailability::default())
    }

    fn clear_observed(core: &ConduitCore, agent_type: AgentType, model_id: &str) {
        if let Some(store) = core.app_state_store() {
            let key = ContextWindowService::key(agent_type, model_id);
            let _ = store.delete(&key);
        }
    }

    #[test]
    fn test_resolve_codex_catalog_windows() {
        let core = build_core();
        clear_observed(&core, AgentType::Codex, "gpt-5.4");
        clear_observed(&core, AgentType::Codex, "gpt-5.3-codex");
        clear_observed(&core, AgentType::Codex, "gpt-5.3-codex-spark");
        let gpt54 = ContextWindowService::resolve(&core, AgentType::Codex, "gpt-5.4");
        assert_eq!(gpt54.tokens, ModelRegistry::CODEX_CONTEXT_WINDOW);
        assert_eq!(gpt54.source, ContextWindowSource::ModelCatalog);

        let codex = ContextWindowService::resolve(&core, AgentType::Codex, "gpt-5.3-codex");
        assert_eq!(codex.tokens, ModelRegistry::CODEX_GPT53_CONTEXT_WINDOW);
        assert_eq!(codex.source, ContextWindowSource::ModelCatalog);

        let spark = ContextWindowService::resolve(&core, AgentType::Codex, "gpt-5.3-codex-spark");
        assert_eq!(
            spark.tokens,
            ModelRegistry::CODEX_GPT53_SPARK_CONTEXT_WINDOW
        );
        assert_eq!(spark.source, ContextWindowSource::ModelCatalog);
    }

    #[test]
    fn test_record_observed_overrides_catalog() {
        let core = build_core();
        clear_observed(&core, AgentType::Codex, "gpt-5.3-codex-spark");
        ContextWindowService::record_observed(
            &core,
            AgentType::Codex,
            "gpt-5.3-codex-spark",
            64_000,
        );

        let resolved =
            ContextWindowService::resolve(&core, AgentType::Codex, "gpt-5.3-codex-spark");
        assert_eq!(resolved.tokens, 64_000);
        assert_eq!(resolved.source, ContextWindowSource::ObservedRuntime);
    }

    #[test]
    fn test_resolve_opencode_infers_underlying_provider_window() {
        let core = build_core();
        clear_observed(&core, AgentType::Opencode, "openai/gpt-5.4");
        let gpt54 = ContextWindowService::resolve(&core, AgentType::Opencode, "openai/gpt-5.4");
        assert_eq!(gpt54.tokens, ModelRegistry::CODEX_CONTEXT_WINDOW);
        assert_eq!(gpt54.source, ContextWindowSource::ModelCatalog);

        clear_observed(&core, AgentType::Opencode, "openai/gpt-5.3-codex-spark");
        let resolved =
            ContextWindowService::resolve(&core, AgentType::Opencode, "openai/gpt-5.3-codex-spark");
        assert_eq!(
            resolved.tokens,
            ModelRegistry::CODEX_GPT53_SPARK_CONTEXT_WINDOW
        );
        assert_eq!(resolved.source, ContextWindowSource::ModelCatalog);
    }
}
