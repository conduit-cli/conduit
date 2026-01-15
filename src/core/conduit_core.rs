//! Core infrastructure shared between TUI and web interfaces.

use std::sync::Arc;

use crate::agent::{ClaudeCodeRunner, CodexCliRunner, GeminiCliRunner};
use crate::config::Config;
use crate::data::{
    AppStateStore, Database, ForkSeedStore, RepositoryStore, SessionTabStore, WorkspaceStore,
};
use crate::git::WorktreeManager;
use crate::util::{Tool, ToolAvailability};

/// Core infrastructure for Conduit, shared between TUI and web interfaces.
///
/// This struct owns all the foundational components:
/// - Database connection and DAO stores for persistent data
/// - Agent runners for Claude, Codex, and Gemini
/// - Configuration and tool availability
/// - Worktree manager for git workspace operations
pub struct ConduitCore {
    /// Application configuration
    config: Config,
    /// Tool availability (git, gh, claude, codex, gemini)
    tools: ToolAvailability,
    /// Database connection (owned to keep connection alive)
    _database: Option<Database>,
    /// Repository DAO
    repo_store: Option<RepositoryStore>,
    /// Workspace DAO
    workspace_store: Option<WorkspaceStore>,
    /// App state DAO (for persisting app settings)
    app_state_store: Option<AppStateStore>,
    /// Session tab DAO (for persisting open tabs)
    session_tab_store: Option<SessionTabStore>,
    /// Fork seed DAO (for persisting fork metadata)
    fork_seed_store: Option<ForkSeedStore>,
    /// Claude Code runner
    claude_runner: Arc<ClaudeCodeRunner>,
    /// Codex CLI runner
    codex_runner: Arc<CodexCliRunner>,
    /// Gemini CLI runner
    gemini_runner: Arc<GeminiCliRunner>,
    /// Worktree manager
    worktree_manager: WorktreeManager,
}

impl ConduitCore {
    /// Create a new ConduitCore with the given configuration and tool availability.
    pub fn new(config: Config, tools: ToolAvailability) -> Self {
        // Initialize database and DAOs
        let (
            database,
            repo_store,
            workspace_store,
            app_state_store,
            session_tab_store,
            fork_seed_store,
        ) = match Database::open_default() {
            Ok(db) => {
                let repo_store = RepositoryStore::new(db.connection());
                let workspace_store = WorkspaceStore::new(db.connection());
                let app_state_store = AppStateStore::new(db.connection());
                let session_tab_store = SessionTabStore::new(db.connection());
                let fork_seed_store = ForkSeedStore::new(db.connection());
                (
                    Some(db),
                    Some(repo_store),
                    Some(workspace_store),
                    Some(app_state_store),
                    Some(session_tab_store),
                    Some(fork_seed_store),
                )
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to open database");
                (None, None, None, None, None, None)
            }
        };

        // Migrate old worktrees folder to workspaces (one-time migration)
        crate::util::migrate_worktrees_to_workspaces();

        // Initialize worktree manager with managed directory (~/.conduit/workspaces)
        let worktree_manager = WorktreeManager::with_managed_dir(crate::util::workspaces_dir());

        // Create runners with configured paths if available
        let claude_runner = match tools.get_path(Tool::Claude) {
            Some(path) => Arc::new(ClaudeCodeRunner::with_path(path.clone())),
            None => Arc::new(ClaudeCodeRunner::new()),
        };
        let codex_runner = match tools.get_path(Tool::Codex) {
            Some(path) => Arc::new(CodexCliRunner::with_path(path.clone())),
            None => Arc::new(CodexCliRunner::new()),
        };
        let gemini_runner = match tools.get_path(Tool::Gemini) {
            Some(path) => Arc::new(GeminiCliRunner::with_path(path.clone())),
            None => Arc::new(GeminiCliRunner::new()),
        };

        Self {
            config,
            tools,
            _database: database,
            repo_store,
            workspace_store,
            app_state_store,
            session_tab_store,
            fork_seed_store,
            claude_runner,
            codex_runner,
            gemini_runner,
            worktree_manager,
        }
    }

    /// Get the application configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get the tool availability.
    pub fn tools(&self) -> &ToolAvailability {
        &self.tools
    }

    /// Get the repository store.
    pub fn repo_store(&self) -> Option<&RepositoryStore> {
        self.repo_store.as_ref()
    }

    /// Get a clone of the repository store.
    pub fn repo_store_clone(&self) -> Option<RepositoryStore> {
        self.repo_store.clone()
    }

    /// Get the workspace store.
    pub fn workspace_store(&self) -> Option<&WorkspaceStore> {
        self.workspace_store.as_ref()
    }

    /// Get a clone of the workspace store.
    pub fn workspace_store_clone(&self) -> Option<WorkspaceStore> {
        self.workspace_store.clone()
    }

    /// Get the app state store.
    pub fn app_state_store(&self) -> Option<&AppStateStore> {
        self.app_state_store.as_ref()
    }

    /// Get a clone of the app state store.
    pub fn app_state_store_clone(&self) -> Option<AppStateStore> {
        self.app_state_store.clone()
    }

    /// Get the session tab store.
    pub fn session_tab_store(&self) -> Option<&SessionTabStore> {
        self.session_tab_store.as_ref()
    }

    /// Get a clone of the session tab store.
    pub fn session_tab_store_clone(&self) -> Option<SessionTabStore> {
        self.session_tab_store.clone()
    }

    /// Get the fork seed store.
    pub fn fork_seed_store(&self) -> Option<&ForkSeedStore> {
        self.fork_seed_store.as_ref()
    }

    /// Get a clone of the fork seed store.
    pub fn fork_seed_store_clone(&self) -> Option<ForkSeedStore> {
        self.fork_seed_store.clone()
    }

    /// Get the Claude runner.
    pub fn claude_runner(&self) -> &Arc<ClaudeCodeRunner> {
        &self.claude_runner
    }

    /// Get the Codex runner.
    pub fn codex_runner(&self) -> &Arc<CodexCliRunner> {
        &self.codex_runner
    }

    /// Get the Gemini runner.
    pub fn gemini_runner(&self) -> &Arc<GeminiCliRunner> {
        &self.gemini_runner
    }

    /// Get the worktree manager.
    pub fn worktree_manager(&self) -> &WorktreeManager {
        &self.worktree_manager
    }

    /// Get a mutable reference to the worktree manager.
    pub fn worktree_manager_mut(&mut self) -> &mut WorktreeManager {
        &mut self.worktree_manager
    }

    /// Get a mutable reference to the tool availability.
    pub fn tools_mut(&mut self) -> &mut ToolAvailability {
        &mut self.tools
    }

    /// Get a mutable reference to the config.
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// Refresh agent runners using the latest tool configuration.
    ///
    /// This should be called after updating tool paths (e.g., when the user
    /// provides a custom path for a missing tool).
    pub fn refresh_runners(&mut self) {
        self.claude_runner = match self.tools.get_path(Tool::Claude) {
            Some(path) => Arc::new(ClaudeCodeRunner::with_path(path.clone())),
            None => Arc::new(ClaudeCodeRunner::new()),
        };
        self.codex_runner = match self.tools.get_path(Tool::Codex) {
            Some(path) => Arc::new(CodexCliRunner::with_path(path.clone())),
            None => Arc::new(CodexCliRunner::new()),
        };
        self.gemini_runner = match self.tools.get_path(Tool::Gemini) {
            Some(path) => Arc::new(GeminiCliRunner::with_path(path.clone())),
            None => Arc::new(GeminiCliRunner::new()),
        };
    }
}
