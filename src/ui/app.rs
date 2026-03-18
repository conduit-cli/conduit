use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use chrono::Utc;
use crossterm::{
    event::{EnableMouseCapture, Event, EventStream, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{enable_raw_mode, EnterAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Widget,
    Frame, Terminal,
};
use tempfile::Builder;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use crate::agent::events::UserQuestion;
use crate::agent::{
    load_claude_history_with_debug, load_codex_history_with_debug,
    load_opencode_history_for_dir_with_debug, load_opencode_history_with_debug, AgentEvent,
    AgentInput, AgentMode, AgentRunner, AgentStartConfig, AgentType, ClaudeCodeRunner,
    CodexCliRunner, GeminiCliRunner, HistoryDebugEntry, MessageDisplay, ModelRegistry,
    OpencodeRunner, SessionId,
};
use crate::command_resolver::{
    CommandResolver, ConduitCommand, MenuEntryKind, ResolveResult, ResolvedPrompt,
};
use crate::config::{parse_action, parse_key_notation, Config, KeyContext, COMMAND_NAMES};
use crate::core::resolve_repo_workspace_settings;
use crate::core::services::ContextWindowService;
use crate::core::ConduitCore;
use crate::data::{
    AppStateStore, ForkSeed, ForkSeedStore, QueuedImageAttachment, QueuedMessage,
    QueuedMessageMode, Repository, RepositoryStore, SessionTab, SessionTabStore, WorkspaceStore,
};
use crate::git::{PrManager, PrStatus, WorkspaceMode, WorkspaceRepoManager};
use crate::ui::action::Action;
use crate::ui::app_prompt;
use crate::ui::app_queue;
use crate::ui::app_state::{
    AppState, BaseDirDialogContext, ModelPickerContext, NewProjectTarget, PendingForkRequest,
    PendingHandoffRequest,
};
use crate::ui::capabilities::AgentCapabilities;
use crate::ui::components::{
    dialog_content_area, AddRepoDialog, AgentSelector, BaseDirDialog, ChatMessage, CommandPalette,
    ConfirmationContext, ConfirmationDialog, ConfirmationType, DefaultModelSelection, ErrorDialog,
    EventDirection, GlobalFooter, HelpDialog, InlinePromptState, InlinePromptType, MessageRole,
    MissingToolDialog, ModelSelector, ProcessingState, ProjectEntry, ProjectPicker, PromptAnswer,
    ProviderSelector, RawEventsClick, ReasoningSelector, SessionHeader, SessionImportPicker,
    SettingsMenu, SettingsMenuEntry, SettingsMenuEntryId, Sidebar, SidebarData, SlashMenu, TabBar,
    TabBarHitTarget, ThemePicker, WorkspaceDefaultsDialog, WorkspaceDefaultsDraft,
    SIDEBAR_HEADER_ROWS,
};
use crate::ui::effect::Effect;
use crate::ui::events::{
    AppEvent, ArchiveWorkspaceDialogPreflightResult, ArchiveWorkspacePreflightResult,
    ForkSessionDialogPreflightResult, ForkWorkspaceCreated, InputMode, ProjectDiscoveryEntry,
    RemoveProjectDialogPreflightResult, RemoveProjectResult, TitleGeneratedResult, ViewMode,
    WorkspaceArchived, WorkspaceCreated,
};
use crate::ui::session::AgentSession;
use crate::ui::terminal_guard::TerminalGuard;
use crate::util::ToolAvailability;

mod app_actions_confirm;
mod app_actions_confirmation;
mod app_actions_dialog;
mod app_actions_global;
mod app_actions_input_edit;
mod app_actions_list;
mod app_actions_overlay;
mod app_actions_pr;
mod app_actions_queue;
mod app_actions_raw_events;
mod app_actions_scroll;
mod app_actions_sidebar;
mod app_actions_submit;
mod app_actions_tabs;
mod app_input;
mod app_scroll;
mod app_selection;

#[cfg(target_os = "macos")]
const PROC_PIDTBSDINFO: libc::c_int = 3;

#[cfg(target_os = "macos")]
const MAXCOMLEN: usize = 16;

#[cfg(target_os = "macos")]
#[repr(C)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: libc::uid_t,
    pbi_gid: libc::gid_t,
    pbi_ruid: libc::uid_t,
    pbi_rgid: libc::gid_t,
    pbi_svuid: libc::uid_t,
    pbi_svgid: libc::gid_t,
    rfu_1: u32,
    pbi_comm: [u8; MAXCOMLEN],
    pbi_name: [u8; 2 * MAXCOMLEN],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn proc_pidinfo(
        pid: libc::c_int,
        flavor: libc::c_int,
        arg: u64,
        buffer: *mut libc::c_void,
        buffersize: libc::c_int,
    ) -> libc::c_int;
}

/// Timeout for double-press detection (ms)
const DOUBLE_PRESS_TIMEOUT_MS: u64 = 500;
/// Timeout for shell command execution.
const SHELL_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Wrapper for AskUserQuestion tool arguments
#[derive(serde::Deserialize)]
struct AskUserQuestionWrapper {
    questions: Vec<UserQuestion>,
}

/// Wrapper for ExitPlanMode tool arguments
#[derive(serde::Deserialize)]
struct ExitPlanModeWrapper {
    plan: String,
}
// 20s allows slow CLI agents to shut down on congested machines without UI hangs.
const AGENT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);
// 500ms grace keeps UI responsive while giving SIGTERM a brief chance to exit.
const AGENT_TERMINATION_GRACE: Duration = Duration::from_millis(500);
// 50ms polling keeps wait loops short without a busy spin.
const AGENT_TERMINATION_POLL_INTERVAL: Duration = Duration::from_millis(50);
// Limit shell output to keep memory bounded.
const SHELL_COMMAND_OUTPUT_LIMIT: usize = 1024 * 1024;
// Bound process reaping after a timeout.
const SHELL_COMMAND_REAP_TIMEOUT: Duration = Duration::from_secs(2);
const PLAN_MODE_INLINE_REMINDER_ENV: &str = "CONDUIT_PLAN_MODE_INLINE_REMINDER";

/// Main application state
pub struct App {
    /// Core infrastructure (database, runners, config)
    core: ConduitCore,
    /// In-memory UI state
    state: AppState,
    /// Event channel sender
    event_tx: mpsc::UnboundedSender<AppEvent>,
    /// Event channel receiver
    event_rx: mpsc::UnboundedReceiver<AppEvent>,
    /// Background git/PR status tracker
    git_tracker: Option<crate::ui::git_tracker::GitTrackerHandle>,
}

// Convenience accessors for backward compatibility during refactoring
impl App {
    /// Get the application configuration.
    #[inline]
    fn config(&self) -> &Config {
        self.core.config()
    }

    /// Get the tool availability.
    #[inline]
    fn tools(&self) -> &ToolAvailability {
        self.core.tools()
    }

    /// Get the repository DAO.
    #[inline]
    fn repo_dao(&self) -> Option<&RepositoryStore> {
        self.core.repo_store()
    }

    fn repo_dao_clone(&self) -> Option<RepositoryStore> {
        self.core.repo_store_clone()
    }

    /// Get the workspace DAO.
    #[inline]
    fn workspace_dao(&self) -> Option<&WorkspaceStore> {
        self.core.workspace_store()
    }

    /// Get a clone of the workspace DAO.
    #[inline]
    fn workspace_dao_clone(&self) -> Option<WorkspaceStore> {
        self.core.workspace_store_clone()
    }

    /// Get the app state DAO.
    #[inline]
    fn app_state_dao(&self) -> Option<&AppStateStore> {
        self.core.app_state_store()
    }

    /// Get a clone of the app state DAO.
    #[inline]
    fn app_state_dao_clone(&self) -> Option<AppStateStore> {
        self.core.app_state_store_clone()
    }

    /// Get the session tab DAO.
    #[inline]
    fn session_tab_dao(&self) -> Option<&SessionTabStore> {
        self.core.session_tab_store()
    }

    /// Get a clone of the session tab DAO.
    #[inline]
    fn session_tab_dao_clone(&self) -> Option<SessionTabStore> {
        self.core.session_tab_store_clone()
    }

    /// Get the fork seed DAO.
    #[inline]
    fn fork_seed_dao(&self) -> Option<&ForkSeedStore> {
        self.core.fork_seed_store()
    }

    /// Get a clone of the fork seed DAO.
    #[inline]
    #[allow(dead_code)] // Will be used by web interface
    fn fork_seed_dao_clone(&self) -> Option<ForkSeedStore> {
        self.core.fork_seed_store_clone()
    }

    /// Get the Claude runner.
    #[inline]
    fn claude_runner(&self) -> &Arc<ClaudeCodeRunner> {
        self.core.claude_runner()
    }

    /// Get the Codex runner.
    #[inline]
    fn codex_runner(&self) -> &Arc<CodexCliRunner> {
        self.core.codex_runner()
    }

    /// Get the Gemini runner.
    #[inline]
    fn gemini_runner(&self) -> &Arc<GeminiCliRunner> {
        self.core.gemini_runner()
    }

    /// Get the OpenCode runner.
    #[inline]
    fn opencode_runner(&self) -> &Arc<OpencodeRunner> {
        self.core.opencode_runner()
    }

    /// Get the worktree manager.
    #[inline]
    fn worktree_manager(&self) -> &WorkspaceRepoManager {
        self.core.worktree_manager()
    }

    /// Get a mutable reference to the worktree manager.
    #[inline]
    #[allow(dead_code)] // Will be used by web interface
    fn worktree_manager_mut(&mut self) -> &mut WorkspaceRepoManager {
        self.core.worktree_manager_mut()
    }

    /// Get a mutable reference to the tools.
    #[inline]
    fn tools_mut(&mut self) -> &mut ToolAvailability {
        self.core.tools_mut()
    }

    /// Get a mutable reference to the config.
    #[inline]
    fn config_mut(&mut self) -> &mut Config {
        self.core.config_mut()
    }

    /// Refresh agent runners (delegates to core) and update UI state.
    fn refresh_runners(&mut self) {
        self.core.refresh_runners();
        let tools = self.tools().clone();
        self.state
            .agent_selector_state
            .update_available_agents(&tools);
        if self.state.provider_selector_state.is_visible() {
            self.state.provider_selector_state =
                crate::ui::components::ProviderSelectorState::configure_for(self.config(), &tools);
            self.state.provider_selector_state.show();
        }
    }
}

fn send_app_event(
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    event: AppEvent,
    context: &'static str,
) -> bool {
    match event_tx.send(event) {
        Ok(()) => true,
        Err(err) => {
            let event_kind = std::mem::discriminant(&err.0);
            tracing::debug!(
                context,
                event_kind = ?event_kind,
                receiver_dropped = true,
                "Failed to send AppEvent"
            );
            false
        }
    }
}

impl App {
    // When true, selection drag auto-scrolls as soon as the cursor hits the first/last row.
    // When false, auto-scroll only starts after the cursor leaves the chat area.
    const AUTO_SCROLL_ON_EDGE_INCLUSIVE: bool = true;
    pub fn new(config: Config, tools: ToolAvailability) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Create core infrastructure (database, runners, worktree manager)
        let core = ConduitCore::new(config.clone(), tools);

        // Initialize git tracker
        let (git_update_tx, mut git_update_rx) = mpsc::unbounded_channel();
        let git_tracker = Some(crate::ui::git_tracker::spawn_git_tracker(git_update_tx));

        // Forward git tracker updates to main event channel
        let event_tx_for_tracker = event_tx.clone();
        tokio::spawn(async move {
            while let Some(update) = git_update_rx.recv().await {
                if event_tx_for_tracker
                    .send(AppEvent::GitTracker(update))
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut app = Self {
            core,
            state: AppState::new(config.max_tabs),
            event_tx,
            event_rx,
            git_tracker,
        };

        // Update agent selector based on available tools
        let tools = app.tools().clone();
        app.state
            .agent_selector_state
            .update_available_agents(&tools);

        // Load sidebar data
        app.refresh_sidebar_data();

        // Restore session state
        app.restore_session_state();

        app
    }

    /// Restore session state from database
    fn restore_session_state(&mut self) {
        tracing::info!("Restoring session state");
        // Check repository count first
        let repo_count = self
            .repo_dao()
            .and_then(|dao| dao.get_all().ok())
            .map(|repos| repos.len())
            .unwrap_or(0);

        // If no repos, show first-time splash
        if repo_count == 0 {
            self.state.show_first_time_splash = true;
            tracing::info!("No repositories found; skipping session restore");
            return;
        }

        // Has repos, don't show first-time splash
        self.state.show_first_time_splash = false;

        // Try to restore saved tabs
        let Some(session_tab_dao) = self.session_tab_dao_clone() else {
            tracing::warn!("Session tab DAO unavailable; skipping session restore");
            return;
        };
        let Some(app_state_dao) = self.app_state_dao_clone() else {
            tracing::warn!("App state DAO unavailable; skipping session restore");
            return;
        };

        let saved_tabs = match session_tab_dao.get_all() {
            Ok(tabs) => tabs,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load saved tabs");
                return;
            }
        };

        if saved_tabs.is_empty() {
            // Has repos but no saved tabs - show main UI without tabs
            tracing::info!("No saved tabs found; skipping session restore");
            return;
        }

        tracing::info!(tab_count = saved_tabs.len(), "Restoring saved tabs");

        // Restore each tab
        for tab in saved_tabs {
            let required_tool = Self::required_tool(tab.agent_type);
            if !self.tools().is_available(required_tool) {
                self.show_missing_tool(
                    required_tool,
                    format!(
                        "{} is required to restore this session.",
                        required_tool.display_name()
                    ),
                );
                break;
            }

            let mut session = AgentSession::new(tab.agent_type);
            session.id = tab.id;
            session.workspace_id = tab.workspace_id;
            session.model = tab.model;
            session.model_invalid = tab.model_invalid;
            session.pr_number = tab.pr_number.map(|n| n as u32);
            session.fork_seed_id = tab.fork_seed_id;
            // Restore AI-generated session title
            session.title = tab.title.clone();
            // Restore agent mode (defaults to Build if not set)
            let parsed_mode = tab
                .agent_mode
                .as_deref()
                .map(AgentMode::parse)
                .unwrap_or_default();
            session.agent_mode = Self::clamp_agent_mode(tab.agent_type, parsed_mode);

            // Look up workspace to get working_dir, workspace_name, and project_name
            if let Some(workspace_id) = tab.workspace_id {
                if let Some(workspace_dao) = self.workspace_dao() {
                    if let Ok(Some(workspace)) = workspace_dao.get_by_id(workspace_id) {
                        session.working_dir = Some(workspace.path);
                        session.workspace_name = Some(workspace.name.clone());

                        // Look up repository for project name
                        if let Some(repo_dao) = self.repo_dao() {
                            if let Ok(Some(repo)) = repo_dao.get_by_id(workspace.repository_id) {
                                session.project_name = Some(repo.name);
                            }
                        }
                    }
                }
            }

            // Set resume session ID if available
            if let Some(ref session_id_str) = tab.agent_session_id {
                let session_id = SessionId::from_string(session_id_str.clone());
                session.resume_session_id = Some(session_id.clone());
                if tab.agent_type != AgentType::Codex {
                    session.agent_session_id = Some(session_id.clone());
                }

                // Load chat history from agent files
                match tab.agent_type {
                    AgentType::Claude => {
                        if let Ok((msgs, debug_entries, file_path)) =
                            load_claude_history_with_debug(session_id_str)
                        {
                            // Populate debug pane with history load info
                            Self::populate_debug_from_history(
                                &mut session.raw_events_view,
                                &debug_entries,
                                &file_path,
                            );
                            for msg in msgs {
                                session.chat_view.push(msg);
                            }
                        }
                    }
                    AgentType::Codex => {
                        if let Ok((msgs, debug_entries, file_path)) =
                            load_codex_history_with_debug(session_id_str)
                        {
                            // Populate debug pane with history load info
                            Self::populate_debug_from_history(
                                &mut session.raw_events_view,
                                &debug_entries,
                                &file_path,
                            );
                            for msg in msgs {
                                session.chat_view.push(msg);
                            }
                        }
                    }
                    AgentType::Gemini => {
                        session.chat_view.push(
                            MessageDisplay::System {
                                content: "Gemini CLI history import isn't supported yet, so previous messages won't be shown.".to_string(),
                            }
                            .to_chat_message(),
                        );
                    }
                    AgentType::Opencode => {
                        if let Ok((msgs, debug_entries, file_path)) =
                            load_opencode_history_with_debug(session_id_str)
                        {
                            Self::populate_debug_from_history(
                                &mut session.raw_events_view,
                                &debug_entries,
                                &file_path,
                            );
                            for msg in msgs {
                                session.chat_view.push(msg);
                            }
                        }
                    }
                }
            } else if tab.agent_type == AgentType::Opencode {
                if let Some(working_dir) = session.working_dir.as_ref() {
                    if let Ok((session_id_str, msgs, debug_entries, file_path)) =
                        load_opencode_history_for_dir_with_debug(working_dir)
                    {
                        let session_id = SessionId::from_string(session_id_str.clone());
                        session.resume_session_id = Some(session_id.clone());
                        session.agent_session_id = Some(session_id);

                        Self::populate_debug_from_history(
                            &mut session.raw_events_view,
                            &debug_entries,
                            &file_path,
                        );
                        for msg in msgs {
                            session.chat_view.push(msg);
                        }
                    }
                }
            }

            // Restore pending user message if it exists and isn't already in history
            if let Some(ref pending) = tab.pending_user_message {
                // Check if last user message in chat matches pending
                let already_in_history = session
                    .chat_view
                    .messages()
                    .iter()
                    .rev()
                    .find(|m| m.role == MessageRole::User)
                    .map(|m| m.content.as_str() == pending.as_str())
                    .unwrap_or(false);

                if !already_in_history {
                    let display = MessageDisplay::User {
                        content: pending.clone(),
                    };
                    session.chat_view.push(display.to_chat_message());
                    session.pending_user_message = Some(pending.clone());
                }
            }

            if !tab.queued_messages.is_empty() {
                session.queued_messages = tab.queued_messages.clone();
            }

            session.input_box.set_history(tab.input_history.clone());

            // Derive fork_welcome_shown: if restoring a forked session that has messages,
            // the welcome message was already shown in the previous session
            if session.fork_seed_id.is_some() && !session.chat_view.messages().is_empty() {
                session.fork_welcome_shown = true;
            }

            session.update_status();

            // Register workspace with git tracker if available
            let track_info = session.workspace_id.zip(session.working_dir.clone());
            let sidebar_pr_update = session
                .pr_number
                .and_then(|pr_num| Self::apply_pr_number_to_session(&mut session, pr_num));

            self.state.tab_manager.add_session(session);

            if let Some((workspace_id, status)) = sidebar_pr_update {
                self.state
                    .sidebar_data
                    .update_workspace_pr_status(workspace_id, Some(status));
            }

            // Track workspace after session is added
            if let Some((workspace_id, working_dir)) = track_info {
                if let Some(ref tracker) = self.git_tracker {
                    tracker.track_workspace(workspace_id, working_dir);
                }
            }
        }

        // Restore active tab
        if let Ok(Some(index_str)) = app_state_dao.get("active_tab_index") {
            if let Ok(index) = index_str.parse::<usize>() {
                let tab_count = self.state.tab_manager.len();
                if tab_count > 0 {
                    let max_index = tab_count.saturating_sub(1);
                    let clamped_index = index.min(max_index);
                    self.state.tab_manager.switch_to(clamped_index);
                }
            }
        }

        // Restore sidebar visibility
        if let Ok(Some(visible_str)) = app_state_dao.get("sidebar_visible") {
            self.state.sidebar_state.visible = visible_str == "true";
        }

        // Restore collapsed repos (repos default to expanded, so we collapse the saved ones)
        if let Ok(Some(collapsed_str)) = app_state_dao.get("tree_collapsed_repos") {
            if !collapsed_str.is_empty() {
                for id_str in collapsed_str.split(',') {
                    if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                        self.state.sidebar_data.collapse_repo(id);
                    }
                }
            }
        }

        // Restore tree selection index (after expanding repos so visible count is correct)
        if let Ok(Some(index_str)) = app_state_dao.get("tree_selected_index") {
            if let Ok(index) = index_str.parse::<usize>() {
                let visible_count = self.state.sidebar_data.visible_nodes().len();
                self.state.sidebar_state.tree_state.selected =
                    index.min(visible_count.saturating_sub(1));
            }
        }

        tracing::info!("Session state restoration complete");
    }

    /// Refresh sidebar data from database
    fn refresh_sidebar_data(&mut self) {
        // Capture current expansion state before rebuild
        let expanded_repos = self.state.sidebar_data.expanded_repo_ids();

        // Collect all repo/workspace data first to avoid borrow conflicts
        type RepoWorkspaceData = Vec<(Uuid, String, Vec<(Uuid, String, String)>)>;

        let repo_workspace_data: RepoWorkspaceData = {
            let Some(repo_dao) = self.repo_dao() else {
                self.state.sidebar_data = SidebarData::new();
                return;
            };
            let Some(workspace_dao) = self.workspace_dao() else {
                self.state.sidebar_data = SidebarData::new();
                return;
            };

            let mut data = Vec::new();
            if let Ok(repos) = repo_dao.get_all() {
                for repo in repos {
                    if let Ok(workspaces) = workspace_dao.get_by_repository(repo.id) {
                        let workspace_info: Vec<_> = workspaces
                            .into_iter()
                            .map(|ws| (ws.id, ws.name, ws.branch))
                            .collect();
                        data.push((repo.id, repo.name, workspace_info));
                    }
                }
            }
            data
        };

        // Now update state (no more borrows on self.core)
        self.state.sidebar_data = SidebarData::new();
        for (repo_id, repo_name, workspace_info) in repo_workspace_data {
            self.state
                .sidebar_data
                .add_repository(repo_id, &repo_name, workspace_info);
        }

        // Restore expansion state
        for repo_id in expanded_repos {
            self.state.sidebar_data.expand_repo(repo_id);
        }

        self.sync_sidebar_busy_state();
    }

    fn sync_sidebar_busy_state(&mut self) {
        let busy_repos: Vec<Uuid> = self.state.busy_repos.iter().copied().collect();
        let busy_repo_actions: Vec<Uuid> = self.state.busy_repo_actions.iter().copied().collect();
        let busy_workspaces: Vec<Uuid> = self.state.busy_workspaces.iter().copied().collect();

        for repo_id in busy_repos {
            self.state.sidebar_data.set_repo_busy(repo_id, true);
        }
        for repo_id in busy_repo_actions {
            self.state.sidebar_data.set_action_busy(repo_id, true);
        }
        for workspace_id in busy_workspaces {
            self.state
                .sidebar_data
                .set_workspace_busy(workspace_id, true);
        }
    }

    fn busy_footer_message(&self) -> Option<String> {
        if !self.state.busy_repos.is_empty() {
            return Some("Removing project...".to_string());
        }
        if !self.state.busy_repo_actions.is_empty() {
            return Some("Creating workspace...".to_string());
        }
        if !self.state.busy_workspaces.is_empty() {
            return Some("Working on workspace...".to_string());
        }
        None
    }

    fn sync_busy_footer_message(&mut self) {
        let desired = self.busy_footer_message();

        if desired.is_none() {
            if self.state.busy_footer_message_active {
                if self.state.footer_message.as_deref() == self.state.busy_footer_message.as_deref()
                {
                    self.state.set_footer_message(None);
                }
                self.state.busy_footer_message_active = false;
                self.state.busy_footer_message = None;
            }
            return;
        }

        self.state.busy_footer_message = desired.clone();

        if self.state.footer_message_expires_at.is_some() {
            self.state.busy_footer_message_active = true;
            return;
        }

        if self.state.footer_message.is_some() && !self.state.busy_footer_message_active {
            self.state.busy_footer_message_active = true;
            return;
        }

        self.state.set_footer_message(desired);
        self.state.busy_footer_message_active = true;
    }

    fn mark_workspace_busy(&mut self, workspace_id: Uuid) {
        if self.state.busy_workspaces.insert(workspace_id) {
            self.state
                .sidebar_data
                .set_workspace_busy(workspace_id, true);
            self.sync_busy_footer_message();
        }
    }

    fn clear_workspace_busy(&mut self, workspace_id: Uuid) {
        if self.state.busy_workspaces.remove(&workspace_id) {
            self.state
                .sidebar_data
                .set_workspace_busy(workspace_id, false);
            self.sync_busy_footer_message();
            if let Some(branch) = self.state.pending_branch_updates.remove(&workspace_id) {
                self.apply_branch_update(workspace_id, branch);
            }
        }
    }

    fn mark_repo_busy(&mut self, repo_id: Uuid) {
        if self.state.busy_repos.insert(repo_id) {
            self.state.sidebar_data.set_repo_busy(repo_id, true);
            self.sync_busy_footer_message();
        }
    }

    fn clear_repo_busy(&mut self, repo_id: Uuid) {
        if self.state.busy_repos.remove(&repo_id) {
            self.state.sidebar_data.set_repo_busy(repo_id, false);
            self.sync_busy_footer_message();
        }
    }

    fn mark_repo_action_busy(&mut self, repo_id: Uuid) {
        if self.state.busy_repo_actions.insert(repo_id) {
            self.state.sidebar_data.set_action_busy(repo_id, true);
            self.sync_busy_footer_message();
        }
    }

    fn clear_repo_action_busy(&mut self, repo_id: Uuid) {
        if self.state.busy_repo_actions.remove(&repo_id) {
            self.state.sidebar_data.set_action_busy(repo_id, false);
            self.sync_busy_footer_message();
        }
    }

    /// Save session state to database for restoration on next startup.
    fn snapshot_session_state(&self) -> SessionStateSnapshot {
        let tabs = self
            .state
            .tab_manager
            .sessions()
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let mut tab = SessionTab::new(
                    index as i32,
                    session.agent_type,
                    session.workspace_id,
                    session
                        .agent_session_id
                        .as_ref()
                        .map(|s| s.as_str().to_string()),
                    session.model.clone(),
                    session.pr_number.map(|n| n as i32),
                );
                tab.id = session.id;
                tab.model_invalid = session.model_invalid;
                // Preserve agent mode for session restoration
                tab.agent_mode = Some(session.agent_mode.as_str().to_string());
                // Preserve pending user message for interrupted sessions
                tab.pending_user_message = session.pending_user_message.clone();
                // Preserve queued messages for interrupted sessions
                tab.queued_messages = session.queued_messages.clone();
                // Preserve input history for arrow-up restoration
                tab.input_history = session.input_box.history_snapshot();
                tab.fork_seed_id = session.fork_seed_id;
                // Preserve AI-generated session title
                tab.title = session.title.clone();
                tab.title_generated = false;
                tab
            })
            .collect();

        SessionStateSnapshot {
            tabs,
            active_tab_index: self.state.tab_manager.active_index(),
            sidebar_visible: self.state.sidebar_state.visible,
            tree_selected_index: self.state.sidebar_state.tree_state.selected,
            collapsed_repo_ids: self.state.sidebar_data.collapsed_repo_ids(),
        }
    }

    fn persist_session_state(
        snapshot: SessionStateSnapshot,
        session_tab_dao: Option<SessionTabStore>,
        app_state_dao: Option<AppStateStore>,
    ) -> SessionPersistenceReport {
        let mut report = SessionPersistenceReport::default();

        let Some(session_tab_dao) = session_tab_dao else {
            tracing::warn!("Session tab DAO unavailable; skipping session persistence");
            report.push("Session tab DAO unavailable; skipping session persistence".to_string());
            return report;
        };
        let Some(app_state_dao) = app_state_dao else {
            tracing::warn!("App state DAO unavailable; skipping session persistence");
            report.push("App state DAO unavailable; skipping session persistence".to_string());
            return report;
        };

        tracing::info!(
            tab_count = snapshot.tabs.len(),
            active_tab_index = snapshot.active_tab_index,
            "Persisting session state"
        );

        for tab in &snapshot.tabs {
            if let Err(e) = session_tab_dao.upsert(tab) {
                tracing::warn!(error = %e, tab_index = tab.tab_index, "Failed to save session tab");
                report.push(format!(
                    "Failed to save session tab at index {}: {}",
                    tab.tab_index, e
                ));
            }
        }

        if let Err(e) =
            app_state_dao.set("active_tab_index", &snapshot.active_tab_index.to_string())
        {
            tracing::warn!(error = %e, "Failed to save active tab index");
            report.push(format!("Failed to save active tab index: {}", e));
        }

        if let Err(e) = app_state_dao.set(
            "sidebar_visible",
            if snapshot.sidebar_visible {
                "true"
            } else {
                "false"
            },
        ) {
            tracing::warn!(error = %e, "Failed to save sidebar visibility");
            report.push(format!("Failed to save sidebar visibility: {}", e));
        }

        if let Err(e) = app_state_dao.set(
            "tree_selected_index",
            &snapshot.tree_selected_index.to_string(),
        ) {
            tracing::warn!(error = %e, "Failed to save tree selection");
            report.push(format!("Failed to save tree selection: {}", e));
        }

        let collapsed_ids: Vec<String> = snapshot
            .collapsed_repo_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        if let Err(e) = app_state_dao.set("tree_collapsed_repos", &collapsed_ids.join(",")) {
            tracing::warn!(error = %e, "Failed to save collapsed repos");
            report.push(format!("Failed to save collapsed repos: {}", e));
        }

        tracing::info!("Session state persistence complete");
        report
    }

    fn apply_session_persistence_report(&mut self, report: SessionPersistenceReport) {
        if report.has_errors() {
            tracing::warn!(
                error_count = report.error_count(),
                first_error = %report.first_error_or_unknown(),
                "Session state persistence completed with warnings"
            );
            self.state.set_timed_footer_message(
                "Warning: some session state could not be saved. Check logs.".to_string(),
                Duration::from_secs(5),
            );
        }
    }

    /// Run the application main loop
    pub async fn run(&mut self) -> anyhow::Result<()> {
        self.spawn_shutdown_listeners();

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();

        // Kitty keyboard protocol disabled - causes terminal corruption on exit
        let keyboard_enhancement_enabled = false;
        // Create terminal guard AFTER enabling features - Drop will clean up on any exit path
        let mut guard = TerminalGuard::new(keyboard_enhancement_enabled);

        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Clear screen
        terminal.clear()?;

        // Main event loop
        let result = self.event_loop(&mut terminal, &mut guard).await;

        // Best-effort persistence on any exit path.
        self.persist_session_state_on_exit();

        // Explicit cleanup with error handling (prevents double-cleanup in Drop)
        terminal.show_cursor()?;
        guard.cleanup()?;

        result
    }

    fn spawn_shutdown_listeners(&self) {
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                send_app_event(&tx, AppEvent::Quit, "shutdown:ctrl_c");
            }
        });

        #[cfg(unix)]
        {
            let tx = self.event_tx.clone();
            tokio::spawn(async move {
                if let Ok(mut sigterm) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                {
                    sigterm.recv().await;
                    send_app_event(&tx, AppEvent::Quit, "shutdown:sigterm");
                }
            });

            let tx = self.event_tx.clone();
            tokio::spawn(async move {
                if let Ok(mut sighup) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                {
                    sighup.recv().await;
                    send_app_event(&tx, AppEvent::Quit, "shutdown:sighup");
                }
            });
        }
    }

    fn persist_session_state_on_exit(&self) {
        let snapshot = self.snapshot_session_state();
        let report = Self::persist_session_state(
            snapshot,
            self.session_tab_dao_clone(),
            self.app_state_dao_clone(),
        );
        if report.has_errors() {
            tracing::warn!(
                error_count = report.error_count(),
                first_error = %report.first_error_or_unknown(),
                "Session state persistence on exit completed with warnings"
            );
        }
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<()> {
        const FRAME_INTERVAL_ACTIVE: Duration = Duration::from_millis(16); // ~60 FPS for animations
        const FRAME_INTERVAL_IDLE: Duration = Duration::from_millis(250); // ~4 FPS when idle

        // Create async event stream for terminal input
        let mut event_stream = EventStream::new();

        // Scroll batching state (moved outside loop to accumulate across frames)
        let mut pending_scroll_up = 0usize;
        let mut pending_scroll_down = 0usize;
        let mut last_tick = Instant::now();

        loop {
            let frame_start = Instant::now();

            // Only draw if needed to save CPU when idle
            if self.state.need_redraw {
                let draw_start = Instant::now();
                terminal.draw(|f| self.draw(f))?;
                let draw_end = Instant::now();
                self.state.metrics.draw_time = draw_end.duration_since(draw_start);
                self.state.metrics.on_draw_end(draw_end);
                self.state.need_redraw = false;
            }

            // Use shorter interval when animations are active, longer when idle
            let target_frame =
                if self.state.needs_animation() || pending_scroll_up > 0 || pending_scroll_down > 0
                {
                    FRAME_INTERVAL_ACTIVE
                } else {
                    FRAME_INTERVAL_IDLE
                };

            // Handle periodic updates (fixed time step)
            // This ensures we always process ticks/animations even if input events flood the queue
            if last_tick.elapsed() >= target_frame {
                let event_start = Instant::now();

                // Flush any pending scroll events accumulated this frame
                if pending_scroll_up > 0 || pending_scroll_down > 0 {
                    self.state.need_redraw = true;
                }
                self.flush_scroll_deltas(&mut pending_scroll_up, &mut pending_scroll_down);

                // Trigger redraw when animations are active
                if self.state.needs_animation() {
                    self.state.need_redraw = true;
                }

                // Handle tick and trigger redraw if UI state was mutated
                if self.handle_tick() {
                    self.state.need_redraw = true;
                }

                self.state.metrics.event_time = event_start.elapsed();
                last_tick = Instant::now();
            }

            let wait = target_frame.saturating_sub(last_tick.elapsed());

            tokio::select! {
                // Prioritize terminal input for immediate response
                biased;

                // Terminal input events via async EventStream - responds immediately
                Some(result) = event_stream.next() => {
                    let event_start = Instant::now();
                    match result {
                        Ok(Event::Key(key)) => {
                            self.state.need_redraw = true;
                            self.flush_scroll_deltas(&mut pending_scroll_up, &mut pending_scroll_down);
                            self.dispatch_event(AppEvent::Input(Event::Key(key)), terminal, guard)
                                .await?;
                        }
                        Ok(Event::Mouse(mouse)) => {
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    if self.handle_tab_bar_wheel(
                                        mouse.column,
                                        mouse.row,
                                        true,
                                    ) {
                                        // Handled by tab bar, skip
                                    } else {
                                        if self.should_route_scroll_to_chat() {
                                            self.record_scroll(1);
                                        }
                                        pending_scroll_up = pending_scroll_up.saturating_add(1);
                                        // Don't set need_redraw here - batch scroll events
                                        // and redraw on clean tick for smoother scrolling
                                    }
                                }
                                MouseEventKind::ScrollDown => {
                                    if self.handle_tab_bar_wheel(
                                        mouse.column,
                                        mouse.row,
                                        false,
                                    ) {
                                        // Handled by tab bar, skip
                                    } else {
                                        if self.should_route_scroll_to_chat() {
                                            self.record_scroll(1);
                                        }
                                        pending_scroll_down = pending_scroll_down.saturating_add(1);
                                        // Don't set need_redraw here - batch scroll events
                                        // and redraw on clean tick for smoother scrolling
                                    }
                                }
                                _ => {
                                    self.state.need_redraw = true;
                                    self.flush_scroll_deltas(
                                        &mut pending_scroll_up,
                                        &mut pending_scroll_down,
                                    );
                                    self.dispatch_event(
                                        AppEvent::Input(Event::Mouse(mouse)),
                                        terminal,
                                        guard,
                                    )
                                    .await?;
                                }
                            }
                        }
                        Ok(event) => {
                            // Other input events (resize, focus, paste, etc.)
                            self.state.need_redraw = true;
                            self.flush_scroll_deltas(
                                &mut pending_scroll_up,
                                &mut pending_scroll_down,
                            );
                            self.dispatch_event(AppEvent::Input(event), terminal, guard)
                                .await?;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Error reading terminal event");
                        }
                    }
                    self.state.metrics.event_time = event_start.elapsed();
                }

                // Sleep until next tick time
                _ = tokio::time::sleep(wait) => {}

                // App events from channel
                Some(event) = self.event_rx.recv() => {
                    // All app events trigger a redraw
                    self.state.need_redraw = true;
                    let event_start = Instant::now();
                    self.dispatch_event(event, terminal, guard).await?;
                    self.state.metrics.event_time = event_start.elapsed();
                }
            }

            // Record total frame time (includes sleep for accurate FPS)
            let frame_end = Instant::now();
            self.state
                .metrics
                .record_frame(frame_end.duration_since(frame_start));
            self.state.metrics.on_frame_end(frame_end);

            if self.state.should_quit {
                break;
            }
        }

        Ok(())
    }

    async fn dispatch_event(
        &mut self,
        event: AppEvent,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<()> {
        let effects = match event {
            AppEvent::Input(input) => {
                // All input events trigger a redraw
                self.state.need_redraw = true;
                self.handle_input_event(input, terminal, guard).await?
            }
            AppEvent::Tick => {
                if self.handle_tick() {
                    self.state.need_redraw = true;
                }
                Vec::new()
            }
            _ => self.handle_app_event(event).await?,
        };

        self.run_effects(effects).await
    }

    /// Handle periodic tick updates. Returns true if visible UI state was mutated
    /// and a redraw is needed.
    fn handle_tick(&mut self) -> bool {
        let mut state_changed = false;
        self.state.tick_count += 1;

        // Tick footer Knight Rider spinner every 2 frames (~40ms at 50 FPS, matches opencode)
        if self.state.tick_count.is_multiple_of(2) {
            self.state.tick_footer_spinner();
        }

        // Tick logo shine animation every 3 frames (~50ms for smooth diagonal sweep)
        // Only tick when splash screen is visible (no sessions open)
        let splash_visible = self.state.tab_manager.is_empty();
        if self.state.tick_count.is_multiple_of(3) {
            if splash_visible {
                // Reset animation when transitioning back to splash screen
                if !self.state.was_splash_visible {
                    self.state.logo_shine.reset();
                }
                self.state.logo_shine.tick();
            }
            self.state.was_splash_visible = splash_visible;
        }

        // Clear stale double-press state and messages
        let now = Instant::now();
        let timeout = Duration::from_millis(DOUBLE_PRESS_TIMEOUT_MS);

        if let Some(last) = self.state.last_ctrl_c_press {
            if now.duration_since(last) > timeout {
                self.state.last_ctrl_c_press = None;
                // Clear associated message
                if matches!(
                    self.state.footer_message.as_deref(),
                    Some("Press Ctrl+C again to interrupt and quit")
                        | Some("Press Ctrl+C again to quit")
                ) {
                    self.state.footer_message = None;
                    state_changed = true;
                }
            }
        }

        if let Some(last) = self.state.last_esc_press {
            if now.duration_since(last) > timeout {
                self.state.last_esc_press = None;
                if matches!(
                    self.state.footer_message.as_deref(),
                    Some("Press Esc again to interrupt") | Some("Press Esc again to clear")
                ) {
                    self.state.footer_message = None;
                    state_changed = true;
                }
            }
        }

        // Clear expired timed footer messages
        let had_timed_message = self.state.footer_message_expires_at.is_some();
        self.state.clear_expired_footer_message();
        self.sync_busy_footer_message();
        if had_timed_message && self.state.footer_message_expires_at.is_none() {
            state_changed = true;
        }

        self.state.theme_picker_state.tick();
        let can_show_picker_error = self.state.theme_picker_state.is_visible()
            || (self.state.footer_message.is_none()
                && self.state.footer_message_expires_at.is_none());
        if can_show_picker_error {
            if let Some(error) = self.state.theme_picker_state.take_error() {
                self.state
                    .set_timed_footer_message(error, Duration::from_secs(5));
                state_changed = true;
            }
        }

        // Tick other animations every 6 frames (~100ms)
        if !self.state.tick_count.is_multiple_of(6) {
            return state_changed;
        }

        // Advance spinner frame for PR processing indicator
        self.state.spinner_frame = self.state.spinner_frame.wrapping_add(1);

        // Tick confirmation dialog spinner (for loading state)
        self.state.confirmation_dialog_state.tick();

        // Tick session import spinner (for loading state)
        self.state.session_import_state.tick();

        // Tick project picker spinner (for loading state)
        self.state.project_picker_state.tick();

        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.tick();
        }

        state_changed
    }

    /// Interrupt the current agent processing
    fn interrupt_agent(&mut self) {
        let mut pid = None;
        let mut pid_start_time = None;
        let mut was_processing = false;
        let mut session_id = None;

        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session_id = Some(session.id);
            pid = session.agent_pid.take();
            pid_start_time = session.agent_pid_start_time.take();
            session.agent_input_tx = None;
            // Clear any active inline prompt and pending permissions since the agent is gone
            session.inline_prompt = None;
            session.pending_tool_permissions.clear();
            session.pending_tool_permission_responses.clear();
            if session.is_processing {
                was_processing = true;
                session.stop_processing();
                session.chat_view.finalize_streaming();
            }
        }

        if let Some(pid) = pid {
            self.spawn_agent_termination(pid, pid_start_time, "interrupt_agent", session_id, true);
        }

        if was_processing {
            if let Some(session_id) = session_id {
                if let Some(session) = self.state.tab_manager.session_by_id_mut(session_id) {
                    Self::flush_pending_agent_output(session);
                    let display = MessageDisplay::System {
                        content: "Interrupted".to_string(),
                    };
                    session.chat_view.push(display.to_chat_message());
                }
            }
            self.state.stop_footer_spinner();
        }
    }

    fn spawn_agent_termination(
        &self,
        pid: u32,
        pid_start_time: Option<u64>,
        context: &'static str,
        session_id: Option<Uuid>,
        report_result: bool,
    ) {
        let event_tx = self.event_tx.clone();
        let context = context.to_string();
        tokio::task::spawn_blocking(move || {
            let success = App::terminate_agent_pid(pid, pid_start_time, &context);
            if report_result {
                send_app_event(
                    &event_tx,
                    AppEvent::AgentTerminationResult {
                        session_id,
                        pid,
                        context,
                        success,
                    },
                    "agent_termination_result",
                );
            } else if !success {
                tracing::warn!(
                    pid,
                    context = %context,
                    "Agent termination failed"
                );
            }
        });
    }

    fn terminate_agent_pid(pid: u32, pid_start_time: Option<u64>, context: &str) -> bool {
        #[cfg(unix)]
        {
            let term_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if term_result == -1 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::ESRCH) {
                    return true;
                }
                tracing::warn!(
                    error = %err,
                    pid,
                    context,
                    "Failed to send SIGTERM to agent"
                );
            } else if Self::wait_for_pid_exit(pid, AGENT_TERMINATION_GRACE, context, "SIGTERM") {
                return true;
            }

            if !Self::pid_identity_matches(pid, pid_start_time, context) {
                return false;
            }

            let kill_result = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            if kill_result == -1 {
                let kill_err = std::io::Error::last_os_error();
                if kill_err.raw_os_error() == Some(libc::ESRCH) {
                    return true;
                }
                tracing::warn!(
                    error = %kill_err,
                    pid,
                    context,
                    "Failed to send SIGKILL to agent"
                );
                return false;
            }

            if Self::wait_for_pid_exit(pid, AGENT_TERMINATION_GRACE, context, "SIGKILL") {
                return true;
            }

            tracing::warn!(
                pid,
                context,
                "Agent still running after SIGKILL grace period"
            );
            false
        }
        #[cfg(not(unix))]
        {
            tracing::warn!(
                pid,
                context,
                "Process termination not implemented on this platform"
            );
            false
        }
    }

    #[cfg(unix)]
    fn wait_for_pid_exit(pid: u32, timeout: Duration, context: &str, signal: &str) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let result = unsafe { libc::kill(pid as i32, 0) };
            if result == 0 {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(AGENT_TERMINATION_POLL_INTERVAL);
                continue;
            }
            let err = std::io::Error::last_os_error();
            if let Some(code) = err.raw_os_error() {
                if code == libc::ESRCH {
                    return true;
                }
                if code == libc::EPERM {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(AGENT_TERMINATION_POLL_INTERVAL);
                    continue;
                }
            }
            tracing::warn!(
                error = %err,
                pid,
                context,
                signal,
                "Failed to poll agent pid after signal"
            );
            return false;
        }
    }

    #[cfg(unix)]
    fn pid_identity_matches(pid: u32, pid_start_time: Option<u64>, context: &str) -> bool {
        let Some(expected_start_time) = pid_start_time else {
            tracing::warn!(
                pid,
                context,
                "Agent pid identity unavailable; skipping SIGKILL"
            );
            return false;
        };
        match Self::pid_start_time(pid) {
            Some(current_start_time) => {
                if current_start_time != expected_start_time {
                    tracing::warn!(
                        pid,
                        context,
                        expected_start_time,
                        current_start_time,
                        "Agent pid start time mismatch; skipping SIGKILL"
                    );
                    return false;
                }
                true
            }
            None => {
                tracing::warn!(
                    pid,
                    context,
                    "Unable to verify agent pid start time; skipping SIGKILL"
                );
                false
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn pid_start_time(pid: u32) -> Option<u64> {
        let stat = match std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
            Ok(contents) => contents,
            Err(err) => {
                tracing::debug!(
                    pid,
                    error = %err,
                    "Failed to read /proc/{}/stat for pid start time",
                    pid
                );
                return None;
            }
        };
        let end = stat.rfind(')')?;
        let after = &stat[end + 1..];
        let mut fields = after.split_whitespace();
        let start_time_str = fields.nth(19)?;
        start_time_str.parse().ok()
    }

    #[cfg(target_os = "macos")]
    fn pid_start_time(pid: u32) -> Option<u64> {
        let mut info = ProcBsdInfo {
            pbi_flags: 0,
            pbi_status: 0,
            pbi_xstatus: 0,
            pbi_pid: 0,
            pbi_ppid: 0,
            pbi_uid: 0,
            pbi_gid: 0,
            pbi_ruid: 0,
            pbi_rgid: 0,
            pbi_svuid: 0,
            pbi_svgid: 0,
            rfu_1: 0,
            pbi_comm: [0; MAXCOMLEN],
            pbi_name: [0; 2 * MAXCOMLEN],
            pbi_nfiles: 0,
            pbi_pgid: 0,
            pbi_pjobc: 0,
            e_tdev: 0,
            e_tpgid: 0,
            pbi_nice: 0,
            pbi_start_tvsec: 0,
            pbi_start_tvusec: 0,
        };
        let size = std::mem::size_of::<ProcBsdInfo>() as libc::c_int;
        let result = unsafe {
            proc_pidinfo(
                pid as libc::c_int,
                PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                size,
            )
        };
        if result <= 0 {
            let err = std::io::Error::last_os_error();
            tracing::debug!(
                pid,
                error = %err,
                "Failed to read pid start time via proc_pidinfo"
            );
            return None;
        }
        if result < size {
            tracing::debug!(
                pid,
                result,
                expected = size,
                "Short proc_pidinfo response for pid start time"
            );
            return None;
        }
        let secs = info.pbi_start_tvsec;
        let usecs = info.pbi_start_tvusec;
        Some(secs.saturating_mul(1_000_000).saturating_add(usecs))
    }

    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    fn pid_start_time(_pid: u32) -> Option<u64> {
        None
    }

    #[cfg(not(unix))]
    fn pid_start_time(_pid: u32) -> Option<u64> {
        None
    }

    fn stop_agent_for_tab(&mut self, tab_index: usize) {
        let mut pid = None;
        let mut pid_start_time = None;
        {
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                Self::flush_pending_agent_output(session);
                if session.is_processing {
                    session.stop_processing();
                }
                pid = session.agent_pid.take();
                pid_start_time = session.agent_pid_start_time.take();
            }
        }

        if let Some(pid) = pid {
            self.spawn_agent_termination(pid, pid_start_time, "stop_agent_for_tab", None, false);
        }
    }

    /// Handle Ctrl+C press with double-press detection
    fn handle_ctrl_c_press(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        let now = Instant::now();
        let is_double = self
            .state
            .last_ctrl_c_press
            .map(|t| now.duration_since(t) < Duration::from_millis(DOUBLE_PRESS_TIMEOUT_MS))
            .unwrap_or(false);

        let is_processing = self
            .state
            .tab_manager
            .active_session()
            .map(|s| s.is_processing)
            .unwrap_or(false);

        tracing::debug!(
            "handle_ctrl_c_press: is_double={}, is_processing={}",
            is_double,
            is_processing
        );

        if is_processing {
            if is_double {
                // Second press while processing: interrupt + quit
                tracing::debug!("Ctrl+C: second press while processing, interrupting and quitting");
                self.interrupt_agent();
                self.state.should_quit = true;
                effects.push(Effect::SaveSessionState);
            } else {
                // First press: show warning
                tracing::debug!("Ctrl+C: first press while processing, showing warning");
                self.state.footer_message = Some("Press Ctrl+C again to interrupt and quit".into());
                self.state.last_ctrl_c_press = Some(now);
            }
        } else if is_double {
            // Second press while idle: quit
            tracing::debug!("Ctrl+C: second press while idle, quitting");
            self.state.should_quit = true;
            effects.push(Effect::SaveSessionState);
        } else {
            // First press while idle: save to history + clear input + show warning
            tracing::debug!("Ctrl+C: first press while idle, saving to history, clearing input and showing warning");
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                // Save current input to history before clearing (if non-empty)
                let current_input = session.input_box.input().to_string();
                if !current_input.trim().is_empty() {
                    session.input_box.add_to_history(&current_input);
                }
                session.input_box.clear();
            }
            self.state.footer_message = Some("Press Ctrl+C again to quit".into());
            self.state.last_ctrl_c_press = Some(now);
        }
        tracing::debug!("footer_message after: {:?}", self.state.footer_message);
        effects
    }

    /// Handle Esc press with double-press detection (only when no dialog is active)
    fn handle_esc_press(&mut self) -> bool {
        let now = Instant::now();
        let is_double = self
            .state
            .last_esc_press
            .map(|t| now.duration_since(t) < Duration::from_millis(DOUBLE_PRESS_TIMEOUT_MS))
            .unwrap_or(false);

        let is_processing = self
            .state
            .tab_manager
            .active_session()
            .map(|s| s.is_processing)
            .unwrap_or(false);

        if is_processing {
            if is_double {
                // Second press while processing: interrupt only
                self.interrupt_agent();
                self.state.footer_message = None;
                self.state.last_esc_press = None;
            } else {
                // First press: show warning
                self.state.footer_message = Some("Press Esc again to interrupt".into());
                self.state.last_esc_press = Some(now);
            }
        } else if is_double {
            // Second press while idle: clear input
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                session.input_box.clear();
            }
            self.state.footer_message = None;
            self.state.last_esc_press = None;
        } else {
            // First press while idle: show warning
            self.state.footer_message = Some("Press Esc again to clear".into());
            self.state.last_esc_press = Some(now);
        }
        true
    }

    /// Check if any overlay is currently active
    fn has_active_dialog(&self) -> bool {
        self.state.has_active_overlay()
    }

    /// Execute a keybinding action
    async fn execute_action(
        &mut self,
        action: Action,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();
        match action {
            // ========== Global Actions ==========
            Action::ToggleSidebar
            | Action::EnterSidebarMode
            | Action::ExitSidebarMode
            | Action::ExpandOrSelect
            | Action::Collapse => {
                self.handle_sidebar_action(action, &mut effects);
            }
            Action::Quit
            | Action::NewProject
            | Action::NewWorkspaceUnderCursor
            | Action::ForkSession
            | Action::HandoffSession
            | Action::InterruptAgent
            | Action::ToggleViewMode
            | Action::ShowModelSelector
            | Action::ShowReasoningSelector
            | Action::ShowThemePicker
            | Action::ShowProvidersSelector
            | Action::OpenSessionImport
            | Action::ImportSession
            | Action::CycleImportFilter
            | Action::ToggleMetrics
            | Action::ToggleAgentMode
            | Action::DumpDebugState
            | Action::CopyWorkspacePath
            | Action::CopySelection => {
                self.handle_global_action(action, &mut effects);
            }
            Action::OpenPr => {
                if let Some(effect) = self.handle_pr_action() {
                    effects.push(effect);
                }
            }
            Action::Suspend => {
                if let Err(err) = self.suspend_app(terminal, guard) {
                    tracing::warn!(error = %err, "Suspend failed: {err}");
                    self.state.set_timed_footer_message(
                        format!("Suspend failed: {err}"),
                        Duration::from_secs(3),
                    );
                }
            }

            // ========== Tab Management ==========
            Action::CloseTab | Action::NextTab | Action::PrevTab | Action::SwitchToTab(_) => {
                self.handle_tab_action(action, &mut effects);
            }

            // ========== File Viewer ==========
            Action::OpenFile(path) => {
                self.handle_open_file(path, &mut effects);
            }

            // ========== Chat Scrolling ==========
            Action::ScrollUp(_)
            | Action::ScrollDown(_)
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollToTop
            | Action::ScrollToBottom
            | Action::ScrollPrevUserMessage
            | Action::ScrollNextUserMessage => {
                self.handle_scroll_action(action);
            }

            // ========== Input Box Editing ==========
            Action::InsertNewline
            | Action::Backspace
            | Action::Delete
            | Action::DeleteWordBack
            | Action::DeleteWordForward
            | Action::DeleteToStart
            | Action::DeleteToEnd
            | Action::MoveCursorLeft
            | Action::MoveCursorRight
            | Action::MoveCursorStart
            | Action::MoveCursorEnd
            | Action::MoveWordLeft
            | Action::MoveWordRight
            | Action::MoveCursorUp
            | Action::MoveCursorDown
            | Action::HistoryPrev
            | Action::HistoryNext => {
                self.handle_input_edit_action(action);
            }
            Action::Submit | Action::SubmitSteer => {
                self.handle_submit_related_action(action, &mut effects)?;
            }
            Action::OpenQueueEditor
            | Action::CloseQueueEditor
            | Action::QueueMoveUp
            | Action::QueueMoveDown
            | Action::QueueEdit
            | Action::QueueDelete => {
                self.handle_queue_action(action);
            }
            Action::EditPromptExternal => {
                if let Err(err) = self.edit_prompt_external(terminal, guard) {
                    tracing::warn!(error = %err, "External editor failed");
                    self.state.set_timed_footer_message(
                        format!("External editor failed: {err}"),
                        Duration::from_secs(3),
                    );
                }
            }

            // ========== List/Tree Navigation ==========
            Action::SelectNext
            | Action::SelectPrev
            | Action::SelectPageDown
            | Action::SelectPageUp => {
                self.handle_list_action(action);
            }
            Action::Confirm => {
                // Defensive normalization: when overlay visibility and input mode diverge,
                // prioritize the top-most visible modal for confirm handling.
                if self.state.model_selector_state.is_visible()
                    && self.state.input_mode != InputMode::SelectingModel
                {
                    self.state.input_mode = InputMode::SelectingModel;
                } else if self.state.provider_selector_state.is_visible()
                    && self.state.input_mode != InputMode::SelectingProviders
                {
                    self.state.input_mode = InputMode::SelectingProviders;
                }

                if self.state.input_mode == InputMode::SlashMenu {
                    if let Some(entry) = self.state.slash_menu_state.selected_entry() {
                        let kind = entry.kind.clone();
                        let label = entry.label.clone();
                        self.state.slash_menu_state.hide();
                        self.state.input_mode = InputMode::Normal;
                        match kind {
                            MenuEntryKind::ConduitCommand(command) => {
                                let active_tab_index = self.state.tab_manager.active_index();
                                effects.extend(
                                    self.execute_resolved_conduit_command(
                                        active_tab_index,
                                        command,
                                    )?,
                                );
                            }
                            MenuEntryKind::ProviderInvocation(_) => {
                                if let Some(session) = self.state.tab_manager.active_session_mut() {
                                    session.input_box.clear();
                                    session.input_box.insert_str(&label);
                                }
                            }
                        }
                    }
                } else if self.state.input_mode == InputMode::CommandPalette {
                    if let Some(entry) = self.state.command_palette_state.selected_entry() {
                        let action = entry.action.clone();
                        self.state.command_palette_state.hide();
                        self.state.input_mode = InputMode::Normal;
                        // Execute the selected action (avoid recursion if it's Confirm)
                        if !matches!(action, Action::Confirm | Action::OpenCommandPalette) {
                            effects.extend(
                                Box::pin(self.execute_action(action, terminal, guard)).await?,
                            );
                        }
                    }
                } else {
                    self.handle_confirm_action(&mut effects)?;
                }
            }
            Action::SetDefaultModel => {
                if self.state.input_mode == InputMode::SelectingModel {
                    if let Some(model) = self.state.model_selector_state.selected_model().cloned() {
                        if self.persist_default_model_selection(&model) {
                            if self.state.model_picker_context
                                == ModelPickerContext::OnboardingDefaultSelection
                            {
                                self.state.model_selector_state.hide();
                                self.state.model_picker_context =
                                    ModelPickerContext::SessionSelection;
                                self.continue_new_project_flow();
                            } else if self.state.model_picker_context
                                == ModelPickerContext::SettingsDefaultSelection
                            {
                                self.state.model_selector_state.hide();
                                self.state.model_picker_context =
                                    ModelPickerContext::SessionSelection;
                                self.reopen_settings_menu();
                            }
                        }
                    }
                }
            }
            Action::Cancel
            | Action::AddRepository
            | Action::OpenSettings
            | Action::ArchiveOrRemove => {
                self.handle_dialog_action(action);
            }

            // ========== Raw Events View ==========
            Action::RawEventsSelectNext
            | Action::RawEventsSelectPrev
            | Action::RawEventsToggleExpand
            | Action::RawEventsCollapse
            | Action::EventDetailToggle
            | Action::EventDetailScrollUp
            | Action::EventDetailScrollDown
            | Action::EventDetailPageUp
            | Action::EventDetailPageDown
            | Action::EventDetailScrollToTop
            | Action::EventDetailScrollToBottom
            | Action::EventDetailCopy => {
                self.handle_raw_events_action(action, &mut effects);
            }

            // ========== Confirmation Dialog ==========
            Action::ConfirmYes | Action::ConfirmNo | Action::ConfirmToggle => {
                self.handle_confirmation_action(action, &mut effects)?;
            }
            Action::ToggleDetails => {
                self.handle_overlay_action(action, &mut effects)?;
            }

            // ========== Agent Selection ==========
            Action::SelectAgent => {
                self.handle_overlay_action(action, &mut effects)?;
            }

            // ========== Command Mode ==========
            Action::ShowHelp => {
                self.handle_overlay_action(action, &mut effects)?;
            }
            Action::ExecuteCommand => {
                if self.state.input_mode == InputMode::Command {
                    if let Some(action) = self.execute_command() {
                        // Prevent recursion - ExecuteCommand can't call itself
                        if !matches!(action, Action::ExecuteCommand) {
                            effects.extend(
                                Box::pin(self.execute_action(action, terminal, guard)).await?,
                            );
                        }
                    }
                }
            }
            Action::CompleteCommand => {
                if self.state.input_mode == InputMode::Command {
                    self.complete_command();
                }
            }

            // ========== Command Palette ==========
            Action::OpenCommandPalette => {
                self.handle_overlay_action(action, &mut effects)?;
            }
        }

        Ok(effects)
    }

    async fn run_effects(&mut self, effects: Vec<Effect>) -> anyhow::Result<()> {
        for effect in effects {
            match effect {
                Effect::SaveSessionState => {
                    tracing::debug!("SaveSessionState effect triggered");
                    let snapshot = self.snapshot_session_state();
                    let session_tab_dao = self.session_tab_dao_clone();
                    let app_state_dao = self.app_state_dao_clone();
                    let save_result = tokio::task::spawn_blocking(move || {
                        Self::persist_session_state(snapshot, session_tab_dao, app_state_dao)
                    })
                    .await;
                    match save_result {
                        Ok(report) => self.apply_session_persistence_report(report),
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to save session state task");
                            self.state.set_timed_footer_message(
                                "Warning: failed to save session state. Check logs.".to_string(),
                                Duration::from_secs(5),
                            );
                        }
                    }
                }
                Effect::StartAgent {
                    session_id,
                    agent_type,
                    config,
                } => {
                    let runner: Arc<dyn AgentRunner> = match agent_type {
                        AgentType::Claude => self.claude_runner().clone(),
                        AgentType::Codex => self.codex_runner().clone(),
                        AgentType::Gemini => self.gemini_runner().clone(),
                        AgentType::Opencode => self.opencode_runner().clone(),
                    };

                    let event_tx = self.event_tx.clone();

                    tokio::spawn(async move {
                        match runner.start(config).await {
                            Ok(mut handle) => {
                                // Send PID (and input channel when available) to main app for interrupt support
                                let pid = handle.pid;
                                let input_tx = handle.take_input_sender();
                                send_app_event(
                                    &event_tx,
                                    AppEvent::AgentStarted {
                                        session_id,
                                        pid,
                                        input_tx,
                                    },
                                    "agent_started",
                                );

                                while let Some(event) = handle.events.recv().await {
                                    if !send_app_event(
                                        &event_tx,
                                        AppEvent::Agent { session_id, event },
                                        "agent_stream",
                                    ) {
                                        tracing::debug!(
                                            session_id = %session_id,
                                            "Failed to send AppEvent for agent stream"
                                        );
                                        let stop_result = tokio::time::timeout(
                                            AGENT_SHUTDOWN_TIMEOUT,
                                            runner.stop(&handle),
                                        )
                                        .await;
                                        let mut stop_ok = false;
                                        match stop_result {
                                            Ok(Ok(())) => {
                                                stop_ok = true;
                                            }
                                            Ok(Err(stop_err)) => {
                                                tracing::debug!(
                                                    session_id = %session_id,
                                                    error = %stop_err,
                                                    "Failed to stop agent after event channel closed"
                                                );
                                            }
                                            Err(_) => {
                                                tracing::debug!(
                                                    session_id = %session_id,
                                                    timeout_secs = AGENT_SHUTDOWN_TIMEOUT.as_secs(),
                                                    "Timed out stopping agent after event channel closed"
                                                );
                                            }
                                        }

                                        if !stop_ok {
                                            let kill_result = tokio::time::timeout(
                                                AGENT_SHUTDOWN_TIMEOUT,
                                                runner.kill(&handle),
                                            )
                                            .await;
                                            match kill_result {
                                                Ok(Ok(())) => {}
                                                Ok(Err(kill_err)) => {
                                                    tracing::debug!(
                                                        session_id = %session_id,
                                                        error = %kill_err,
                                                        "Failed to kill agent after event channel closed"
                                                    );
                                                }
                                                Err(_) => {
                                                    tracing::debug!(
                                                        session_id = %session_id,
                                                        timeout_secs = AGENT_SHUTDOWN_TIMEOUT.as_secs(),
                                                        "Timed out killing agent after event channel closed"
                                                    );
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }
                                send_app_event(
                                    &event_tx,
                                    AppEvent::AgentStreamEnded { session_id },
                                    "agent_stream_ended",
                                );
                            }
                            Err(e) => {
                                send_app_event(
                                    &event_tx,
                                    AppEvent::AgentStartFailed {
                                        session_id,
                                        error: format!("Agent error: {}", e),
                                    },
                                    "agent_start_error",
                                );
                                send_app_event(
                                    &event_tx,
                                    AppEvent::AgentStreamEnded { session_id },
                                    "agent_stream_ended",
                                );
                            }
                        }
                    });
                }
                Effect::PrPreflight {
                    tab_index,
                    working_dir,
                } => {
                    let event_tx = self.event_tx.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = PrManager::preflight_check(&working_dir);
                        send_app_event(
                            &event_tx,
                            AppEvent::PrPreflightCompleted {
                                tab_index,
                                working_dir,
                                result,
                            },
                            "pr_preflight_completed",
                        );
                    });
                }
                Effect::OpenPrInBrowser { working_dir } => {
                    let event_tx = self.event_tx.clone();
                    tokio::task::spawn_blocking(move || {
                        let result =
                            PrManager::open_pr_in_browser(&working_dir).map_err(|e| e.to_string());
                        send_app_event(
                            &event_tx,
                            AppEvent::OpenPrCompleted { result },
                            "open_pr_completed",
                        );
                    });
                }
                Effect::DumpDebugState => {
                    let result = self.dump_debug_state();
                    send_app_event(
                        &self.event_tx,
                        AppEvent::DebugDumped { result },
                        "debug_dumped",
                    );
                }
                Effect::RunShellCommand {
                    session_id,
                    message_index,
                    command,
                    working_dir,
                } => {
                    let event_tx = self.event_tx.clone();
                    let config_working_dir = self.config().working_dir.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let effective_working_dir =
                                working_dir.as_ref().or(Some(&config_working_dir));
                            let effective_working_dir = match effective_working_dir {
                                Some(dir) => dir,
                                None => {
                                    return Err("No working directory available for shell command"
                                        .to_string())
                                }
                            };
                            let (shell, flag) = if cfg!(windows) {
                                ("cmd", "/C")
                            } else {
                                ("sh", "-c")
                            };
                            let mut cmd = tokio::process::Command::new(shell);
                            cmd.arg(flag).arg(&command);
                            cmd.kill_on_drop(true);
                            cmd.stdin(Stdio::null());
                            cmd.stdout(Stdio::piped());
                            cmd.stderr(Stdio::piped());
                            cmd.current_dir(effective_working_dir);

                            let mut child = cmd
                                .spawn()
                                .map_err(|e| format!("Failed to run shell command: {e}"))?;
                            let stdout = child.stdout.take().ok_or_else(|| {
                                "Failed to run shell command: stdout unavailable".to_string()
                            })?;
                            let stderr = child.stderr.take().ok_or_else(|| {
                                "Failed to run shell command: stderr unavailable".to_string()
                            })?;

                            let stdout_task = tokio::spawn(async move {
                                App::read_bounded_output(stdout, SHELL_COMMAND_OUTPUT_LIMIT).await
                            });
                            let stderr_task = tokio::spawn(async move {
                                App::read_bounded_output(stderr, SHELL_COMMAND_OUTPUT_LIMIT).await
                            });

                            let status =
                                match tokio::time::timeout(SHELL_COMMAND_TIMEOUT, child.wait())
                                    .await
                                {
                                    Ok(status) => status
                                        .map_err(|e| format!("Failed to run shell command: {e}"))?,
                                    Err(_) => {
                                        if let Err(err) = child.kill().await {
                                            tracing::debug!(
                                                error = %err,
                                                "Failed to kill timed out shell command"
                                            );
                                        }
                                        match tokio::time::timeout(
                                            SHELL_COMMAND_REAP_TIMEOUT,
                                            child.wait(),
                                        )
                                        .await
                                        {
                                            Ok(Ok(_)) => {}
                                            Ok(Err(err)) => {
                                                tracing::debug!(
                                                    error = %err,
                                                    "Failed to reap timed out shell command"
                                                );
                                            }
                                            Err(_) => {
                                                tracing::debug!(
                                                    timeout_secs =
                                                        SHELL_COMMAND_REAP_TIMEOUT.as_secs(),
                                                    "Timed out waiting to reap shell command"
                                                );
                                            }
                                        }
                                        stdout_task.abort();
                                        stderr_task.abort();
                                        if let Err(err) = stdout_task.await {
                                            tracing::debug!(
                                                error = %err,
                                                "Failed to abort stdout reader task"
                                            );
                                        }
                                        if let Err(err) = stderr_task.await {
                                            tracing::debug!(
                                                error = %err,
                                                "Failed to abort stderr reader task"
                                            );
                                        }
                                        return Err(format!(
                                            "Shell command timed out after {}s",
                                            SHELL_COMMAND_TIMEOUT.as_secs()
                                        ));
                                    }
                                };

                            let (stdout_bytes, stdout_truncated, stdout_timed_out) =
                                App::join_reader_with_timeout(stdout_task, "stdout").await?;
                            let (stderr_bytes, stderr_truncated, _stderr_timed_out) =
                                if stdout_timed_out {
                                    stderr_task.abort();
                                    if let Err(err) = stderr_task.await {
                                        tracing::debug!(
                                            error = %err,
                                            "Failed to abort stderr reader task"
                                        );
                                    }
                                    (Vec::new(), true, true)
                                } else {
                                    App::join_reader_with_timeout(stderr_task, "stderr").await?
                                };
                            let stdout = String::from_utf8_lossy(&stdout_bytes);
                            let stderr = String::from_utf8_lossy(&stderr_bytes);
                            let mut combined = String::new();
                            if !stdout.is_empty() {
                                combined.push_str(&stdout);
                            }
                            if !stderr.is_empty() {
                                if !combined.is_empty() && !combined.ends_with('\n') {
                                    combined.push('\n');
                                }
                                combined.push_str(&stderr);
                            }
                            if stdout_truncated || stderr_truncated {
                                if !combined.is_empty() && !combined.ends_with('\n') {
                                    combined.push('\n');
                                }
                                combined.push_str("[output truncated]\n");
                            }
                            Ok(crate::ui::events::ShellCommandResult {
                                output: combined,
                                exit_code: status.code(),
                            })
                        }
                        .await;

                        send_app_event(
                            &event_tx,
                            AppEvent::ShellCommandCompleted {
                                session_id,
                                message_index,
                                result,
                            },
                            "shell_command_completed",
                        );
                    });
                }
                Effect::CreateWorkspace { repo_id } => {
                    let repo_dao = self.repo_dao_clone();
                    let workspace_dao = self.workspace_dao_clone();
                    let worktree_manager = self.worktree_manager().clone();
                    let config = self.config().clone();
                    let event_tx = self.event_tx.clone();

                    tokio::task::spawn_blocking(move || {
                        let result: Result<WorkspaceCreated, String> = (|| {
                            let repo_dao = repo_dao
                                .ok_or_else(|| "No repository DAO available".to_string())?;
                            let workspace_dao = workspace_dao
                                .ok_or_else(|| "No workspace DAO available".to_string())?;

                            let repo = repo_dao
                                .get_by_id(repo_id)
                                .map_err(|e| format!("Failed to load repository: {}", e))?
                                .ok_or_else(|| "Repository not found".to_string())?;

                            let base_path = repo
                                .base_path
                                .clone()
                                .ok_or_else(|| "Repository has no base path".to_string())?;
                            let settings = resolve_repo_workspace_settings(&config, &repo);

                            // Get ALL workspace names (including archived) to prevent resurrection
                            // of old workspace names when creating new ones
                            let existing_names: Vec<String> = workspace_dao
                                .get_all_names_by_repository(repo_id)
                                .unwrap_or_default();

                            let workspace_name =
                                crate::util::generate_workspace_name(&existing_names);
                            let username = crate::util::get_git_username();
                            let branch_name =
                                crate::util::generate_branch_name(&username, &workspace_name);

                            let worktree_path = worktree_manager
                                .create_workspace(
                                    settings.mode,
                                    &base_path,
                                    &branch_name,
                                    &workspace_name,
                                )
                                .map_err(|e| format!("Failed to create workspace: {}", e))?;

                            let workspace = crate::data::Workspace::new(
                                repo_id,
                                &workspace_name,
                                &branch_name,
                                worktree_path,
                            );
                            let workspace_id = workspace.id;

                            if let Err(e) = workspace_dao.create(&workspace) {
                                if let Err(cleanup_err) = worktree_manager.remove_workspace(
                                    settings.mode,
                                    &base_path,
                                    &workspace.path,
                                ) {
                                    tracing::error!(
                                        error = %cleanup_err,
                                        base_path = %base_path.display(),
                                        workspace_path = %workspace.path.display(),
                                        "Failed to clean up workspace after DB error"
                                    );
                                }
                                if let Err(branch_err) = worktree_manager.delete_branch(
                                    settings.mode,
                                    &base_path,
                                    &workspace.path,
                                    &branch_name,
                                ) {
                                    tracing::error!(
                                        error = %branch_err,
                                        base_path = %base_path.display(),
                                        workspace_path = %workspace.path.display(),
                                        branch = %branch_name,
                                        "Failed to delete branch after DB error"
                                    );
                                }
                                return Err(format!("Failed to save workspace to database: {}", e));
                            }

                            Ok(WorkspaceCreated {
                                repo_id,
                                workspace_id,
                            })
                        })();

                        send_app_event(
                            &event_tx,
                            AppEvent::WorkspaceCreated { repo_id, result },
                            "workspace_created",
                        );
                    });
                }
                Effect::ForkWorkspace {
                    parent_workspace_id,
                    base_branch,
                } => {
                    let repo_dao = self.repo_dao_clone();
                    let workspace_dao = self.workspace_dao_clone();
                    let worktree_manager = self.worktree_manager().clone();
                    let config = self.config().clone();
                    let event_tx = self.event_tx.clone();

                    tokio::task::spawn_blocking(move || {
                        let result: Result<ForkWorkspaceCreated, String> = (|| {
                            let workspace_dao = workspace_dao
                                .ok_or_else(|| "No workspace DAO available".to_string())?;
                            let repo_dao = repo_dao
                                .ok_or_else(|| "No repository DAO available".to_string())?;

                            let parent_workspace = workspace_dao
                                .get_by_id(parent_workspace_id)
                                .map_err(|e| format!("Failed to load workspace: {}", e))?
                                .ok_or_else(|| "Workspace not found".to_string())?;

                            let repo = repo_dao
                                .get_by_id(parent_workspace.repository_id)
                                .map_err(|e| format!("Failed to load repository: {}", e))?
                                .ok_or_else(|| "Repository not found".to_string())?;

                            let base_path = repo
                                .base_path
                                .clone()
                                .ok_or_else(|| "Repository has no base path".to_string())?;
                            let settings = resolve_repo_workspace_settings(&config, &repo);

                            // Use the base_branch that was computed when the dialog was shown
                            // to ensure consistency between what was displayed and what is used

                            // Get ALL workspace names (including archived) to prevent resurrection
                            // of old workspace names when creating new ones
                            let existing_names: Vec<String> = workspace_dao
                                .get_all_names_by_repository(parent_workspace.repository_id)
                                .unwrap_or_default();

                            let workspace_name =
                                crate::util::generate_workspace_name(&existing_names);
                            let username = crate::util::get_git_username();
                            let branch_name =
                                crate::util::generate_branch_name(&username, &workspace_name);

                            let worktree_path = worktree_manager
                                .create_workspace_from_branch(
                                    settings.mode,
                                    &base_path,
                                    &base_branch,
                                    &branch_name,
                                    &workspace_name,
                                )
                                .map_err(|e| format!("Failed to create workspace: {}", e))?;

                            let workspace = crate::data::Workspace::new(
                                parent_workspace.repository_id,
                                &workspace_name,
                                &branch_name,
                                worktree_path,
                            );
                            let workspace_id = workspace.id;

                            if let Err(e) = workspace_dao.create(&workspace) {
                                if let Err(cleanup_err) = worktree_manager.remove_workspace(
                                    settings.mode,
                                    &base_path,
                                    &workspace.path,
                                ) {
                                    tracing::error!(
                                        error = %cleanup_err,
                                        base_path = %base_path.display(),
                                        workspace_path = %workspace.path.display(),
                                        "Failed to clean up workspace after DB error"
                                    );
                                }
                                if let Err(branch_err) = worktree_manager.delete_branch(
                                    settings.mode,
                                    &base_path,
                                    &workspace.path,
                                    &branch_name,
                                ) {
                                    tracing::error!(
                                        error = %branch_err,
                                        base_path = %base_path.display(),
                                        workspace_path = %workspace.path.display(),
                                        branch = %branch_name,
                                        "Failed to delete branch after DB error"
                                    );
                                }
                                return Err(format!("Failed to save workspace to database: {}", e));
                            }

                            Ok(ForkWorkspaceCreated {
                                repo_id: parent_workspace.repository_id,
                                workspace_id,
                            })
                        })(
                        );

                        send_app_event(
                            &event_tx,
                            AppEvent::ForkWorkspaceCreated {
                                parent_workspace_id,
                                result,
                            },
                            "fork_workspace_created",
                        );
                    });
                }
                Effect::ArchiveWorkspacePreflight { workspace_id } => {
                    let repo_dao = self.repo_dao_clone();
                    let workspace_dao = self.workspace_dao_clone();
                    let worktree_manager = self.worktree_manager().clone();
                    let config = self.config().clone();
                    let event_tx = self.event_tx.clone();

                    tokio::task::spawn_blocking(move || {
                        let result: Result<ArchiveWorkspacePreflightResult, String> = (|| {
                            let workspace_dao = workspace_dao
                                .ok_or_else(|| "No workspace DAO available".to_string())?;
                            let workspace = workspace_dao
                                .get_by_id(workspace_id)
                                .map_err(|e| format!("Failed to load workspace: {}", e))?
                                .ok_or_else(|| "Workspace not found".to_string())?;

                            let repo = match repo_dao.as_ref() {
                                Some(dao) => match dao.get_by_id(workspace.repository_id) {
                                    Ok(repo) => repo,
                                    Err(err) => {
                                        tracing::warn!(
                                            error = %err,
                                            workspace_id = %workspace_id,
                                            "Failed to load repository for archive preflight"
                                        );
                                        None
                                    }
                                },
                                None => {
                                    tracing::warn!(
                                        workspace_id = %workspace_id,
                                        "Repository DAO unavailable for archive preflight"
                                    );
                                    None
                                }
                            };

                            let should_prompt_remote_delete = match repo {
                                Some(repo) => {
                                    let settings = resolve_repo_workspace_settings(&config, &repo);
                                    if settings.archive_delete_branch
                                        && settings.archive_remote_prompt
                                    {
                                        match repo.base_path {
                                            Some(base_path) => {
                                                match worktree_manager.remote_branch_exists(
                                                    &base_path,
                                                    &workspace.branch,
                                                ) {
                                                    Ok(exists) => exists,
                                                    Err(err) => {
                                                        tracing::warn!(
                                                            error = %err,
                                                            workspace_id = %workspace_id,
                                                            branch = %workspace.branch,
                                                            "Failed to check remote branch existence during archive preflight"
                                                        );
                                                        false
                                                    }
                                                }
                                            }
                                            None => false,
                                        }
                                    } else {
                                        false
                                    }
                                }
                                None => false,
                            };

                            Ok(ArchiveWorkspacePreflightResult {
                                should_prompt_remote_delete,
                            })
                        })(
                        );

                        send_app_event(
                            &event_tx,
                            AppEvent::ArchiveWorkspacePreflightCompleted {
                                workspace_id,
                                result,
                            },
                            "archive_workspace_preflight_completed",
                        );
                    });
                }
                Effect::ArchiveWorkspace {
                    workspace_id,
                    delete_remote,
                } => {
                    let repo_dao = self.repo_dao_clone();
                    let workspace_dao = self.workspace_dao_clone();
                    let worktree_manager = self.worktree_manager().clone();
                    let config = self.config().clone();
                    let event_tx = self.event_tx.clone();

                    tokio::task::spawn_blocking(move || {
                        let result: Result<WorkspaceArchived, String> = (|| {
                            let workspace_dao = workspace_dao
                                .ok_or_else(|| "No workspace DAO available".to_string())?;
                            let workspace = workspace_dao
                                .get_by_id(workspace_id)
                                .map_err(|e| format!("Failed to load workspace: {}", e))?
                                .ok_or_else(|| "Workspace not found".to_string())?;

                            let repo = match repo_dao.as_ref() {
                                Some(dao) => match dao.get_by_id(workspace.repository_id) {
                                    Ok(repo) => repo,
                                    Err(err) => {
                                        tracing::warn!(
                                            error = %err,
                                            workspace_id = %workspace_id,
                                            "Failed to load repository for archive"
                                        );
                                        None
                                    }
                                },
                                None => {
                                    tracing::warn!(
                                        workspace_id = %workspace_id,
                                        "Repository DAO unavailable for archive"
                                    );
                                    None
                                }
                            };
                            let repo_base_path =
                                repo.as_ref().and_then(|repo| repo.base_path.clone());
                            let settings = repo
                                .as_ref()
                                .map(|repo| resolve_repo_workspace_settings(&config, repo));

                            let mut warnings = Vec::new();
                            let mut archived_commit_sha = None;
                            if let (Some(base_path), Some(settings)) = (repo_base_path, settings) {
                                match worktree_manager.get_branch_sha(
                                    settings.mode,
                                    &base_path,
                                    &workspace.path,
                                    &workspace.branch,
                                ) {
                                    Ok(commit_sha) => {
                                        archived_commit_sha = Some(commit_sha);
                                    }
                                    Err(e) => {
                                        warnings.push(format!("Failed to read branch SHA: {}", e));
                                    }
                                }

                                if let Err(e) = worktree_manager.remove_workspace(
                                    settings.mode,
                                    &base_path,
                                    &workspace.path,
                                ) {
                                    warnings.push(format!("Failed to remove worktree: {}", e));
                                }

                                if settings.archive_delete_branch {
                                    if let Err(e) = worktree_manager.delete_branch(
                                        settings.mode,
                                        &base_path,
                                        &workspace.path,
                                        &workspace.branch,
                                    ) {
                                        warnings.push(format!(
                                            "Failed to delete branch '{}': {}",
                                            workspace.branch, e
                                        ));
                                    }
                                }

                                if delete_remote && settings.archive_delete_branch {
                                    if let Err(e) = worktree_manager
                                        .delete_remote_branch(&base_path, &workspace.branch)
                                    {
                                        warnings.push(format!(
                                            "Failed to delete remote branch '{}': {}",
                                            workspace.branch, e
                                        ));
                                    }
                                }
                            }

                            workspace_dao
                                .archive(workspace_id, archived_commit_sha)
                                .map_err(|e| {
                                    format!("Failed to archive workspace in database: {}", e)
                                })?;

                            Ok(WorkspaceArchived {
                                workspace_id,
                                warnings,
                            })
                        })(
                        );

                        send_app_event(
                            &event_tx,
                            AppEvent::WorkspaceArchived {
                                workspace_id,
                                result,
                            },
                            "workspace_archived",
                        );
                    });
                }
                Effect::RemoveProject { repo_id } => {
                    let repo_dao = self.repo_dao_clone();
                    let workspace_dao = self.workspace_dao_clone();
                    let worktree_manager = self.worktree_manager().clone();
                    let config = self.config().clone();
                    let event_tx = self.event_tx.clone();

                    tokio::task::spawn_blocking(move || {
                        let mut errors = Vec::new();
                        let mut workspace_ids = Vec::new();

                        let Some(repo_dao) = repo_dao else {
                            errors.push("No repository DAO available".to_string());
                            send_app_event(
                                &event_tx,
                                AppEvent::ProjectRemoved {
                                    result: RemoveProjectResult {
                                        repo_id,
                                        workspace_ids,
                                        errors,
                                    },
                                },
                                "project_removed",
                            );
                            return;
                        };
                        let Some(workspace_dao) = workspace_dao else {
                            errors.push("No workspace DAO available".to_string());
                            send_app_event(
                                &event_tx,
                                AppEvent::ProjectRemoved {
                                    result: RemoveProjectResult {
                                        repo_id,
                                        workspace_ids,
                                        errors,
                                    },
                                },
                                "project_removed",
                            );
                            return;
                        };

                        let (repo_base_path, repo_name, repo_settings) =
                            match repo_dao.get_by_id(repo_id) {
                                Ok(Some(repo)) => {
                                    let settings = resolve_repo_workspace_settings(&config, &repo);
                                    (repo.base_path, repo.name, Some(settings))
                                }
                                Ok(None) => {
                                    errors.push("Repository not found".to_string());
                                    send_app_event(
                                        &event_tx,
                                        AppEvent::ProjectRemoved {
                                            result: RemoveProjectResult {
                                                repo_id,
                                                workspace_ids,
                                                errors,
                                            },
                                        },
                                        "project_removed",
                                    );
                                    return;
                                }
                                Err(e) => {
                                    errors.push(format!("Failed to load repository: {}", e));
                                    send_app_event(
                                        &event_tx,
                                        AppEvent::ProjectRemoved {
                                            result: RemoveProjectResult {
                                                repo_id,
                                                workspace_ids,
                                                errors,
                                            },
                                        },
                                        "project_removed",
                                    );
                                    return;
                                }
                            };

                        let workspaces =
                            workspace_dao.get_by_repository(repo_id).unwrap_or_default();
                        for ws in workspaces {
                            workspace_ids.push(ws.id);
                            let mut archived_commit_sha = None;
                            if let (Some(base_path), Some(settings)) =
                                (repo_base_path.as_ref(), repo_settings)
                            {
                                match worktree_manager.get_branch_sha(
                                    settings.mode,
                                    base_path,
                                    &ws.path,
                                    &ws.branch,
                                ) {
                                    Ok(sha) => {
                                        archived_commit_sha = Some(sha);
                                    }
                                    Err(e) => {
                                        errors.push(format!(
                                            "Failed to read branch SHA for workspace '{}': {}",
                                            ws.name, e
                                        ));
                                    }
                                }

                                if let Err(e) = worktree_manager.remove_workspace(
                                    settings.mode,
                                    base_path,
                                    &ws.path,
                                ) {
                                    errors.push(format!(
                                        "Failed to remove worktree '{}': {}",
                                        ws.name, e
                                    ));
                                }

                                if let Err(e) = worktree_manager.delete_branch(
                                    settings.mode,
                                    base_path,
                                    &ws.path,
                                    &ws.branch,
                                ) {
                                    errors.push(format!(
                                        "Failed to delete branch '{}' for workspace '{}': {}",
                                        ws.branch, ws.name, e
                                    ));
                                }
                            }
                            if let Err(e) = workspace_dao.archive(ws.id, archived_commit_sha) {
                                errors.push(format!(
                                    "Failed to archive workspace '{}': {}",
                                    ws.name, e
                                ));
                            }
                        }

                        let workspaces_dir = crate::util::workspaces_dir();
                        if let Some(e) =
                            crate::util::remove_project_workspaces_dir(&workspaces_dir, &repo_name)
                        {
                            errors.push(e);
                        }

                        if let Err(e) = repo_dao.delete(repo_id) {
                            errors
                                .push(format!("Failed to delete repository from database: {}", e));
                        }

                        send_app_event(
                            &event_tx,
                            AppEvent::ProjectRemoved {
                                result: RemoveProjectResult {
                                    repo_id,
                                    workspace_ids,
                                    errors,
                                },
                            },
                            "project_removed",
                        );
                    });
                }
                Effect::CopyToClipboard(text) => {
                    use arboard::Clipboard;
                    match Clipboard::new() {
                        Ok(mut clipboard) => {
                            if let Err(e) = clipboard.set_text(text) {
                                tracing::debug!(error = %e, "Failed to copy text to clipboard");
                            }
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "Failed to initialize clipboard");
                        }
                    }
                }
                Effect::DiscoverSessions => {
                    use crate::session::{discover_sessions_incremental, SessionDiscoveryUpdate};
                    let event_tx = self.event_tx.clone();
                    tokio::task::spawn_blocking(move || {
                        discover_sessions_incremental(|update| {
                            let event = match update {
                                SessionDiscoveryUpdate::CachedLoaded(sessions) => {
                                    AppEvent::SessionsCacheLoaded { sessions }
                                }
                                SessionDiscoveryUpdate::SessionUpdated(session) => {
                                    AppEvent::SessionUpdated { session }
                                }
                                SessionDiscoveryUpdate::SessionRemoved(file_path) => {
                                    AppEvent::SessionRemoved { file_path }
                                }
                                SessionDiscoveryUpdate::Complete => {
                                    AppEvent::SessionDiscoveryComplete
                                }
                            };
                            send_app_event(&event_tx, event, "session_discovery_update");
                        });
                    });
                }
                Effect::ImportSession(session) => {
                    // Create a new tab with the session's agent type and working directory
                    let agent_type = session.agent_type;
                    let working_dir = session
                        .project
                        .clone()
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| self.config().working_dir.clone());

                    // Load the session history into a new tab
                    self.create_imported_session_tab(
                        agent_type,
                        session.file_path.clone(),
                        working_dir,
                    )
                    .await?;
                }
                Effect::GenerateTitleAndBranch {
                    session_id,
                    user_message,
                    working_dir,
                    workspace_id,
                    current_branch,
                } => {
                    let tools = self.tools().clone();
                    let event_tx = self.event_tx.clone();
                    let worktree_manager = self.worktree_manager().clone();
                    let workspace_dao = self.workspace_dao_clone();

                    tokio::spawn(async move {
                        // No outer timeout here - timeout is applied inside generate_title_and_branch
                        // for the AI call. This ensures:
                        // 1. The event_tx.send always runs (not cancelled by outer timeout)
                        // 2. spawn_blocking git/db work always completes or fails deterministically
                        // 3. AI call has its own 10-second timeout in title_generator.rs
                        let result = generate_title_and_branch_impl(
                            tools,
                            user_message,
                            working_dir,
                            workspace_id,
                            current_branch,
                            worktree_manager,
                            workspace_dao,
                        )
                        .await;

                        if !send_app_event(
                            &event_tx,
                            AppEvent::TitleGenerated { session_id, result },
                            "title_generated",
                        ) {
                            tracing::debug!(%session_id, "Failed to send TitleGenerated event");
                        }
                    });
                }
            }
        }

        Ok(())
    }

    /// Helper to check if a colon keypress should trigger command mode.
    fn should_trigger_command_mode(
        key_code: KeyCode,
        key_modifiers: KeyModifiers,
        input_mode: InputMode,
        input_is_empty: bool,
        shell_mode: bool,
        has_inline_prompt: bool,
    ) -> bool {
        key_code == KeyCode::Char(':')
            && key_modifiers.is_empty()
            && input_is_empty
            && !shell_mode
            && !has_inline_prompt
            && !matches!(
                input_mode,
                InputMode::Command
                    | InputMode::ShowingHelp
                    | InputMode::AddingRepository
                    | InputMode::SettingBaseDir
                    | InputMode::PickingProject
                    | InputMode::ShowingError
                    | InputMode::SelectingAgent
                    | InputMode::Confirming
                    | InputMode::ImportingSession
                    | InputMode::CommandPalette
                    | InputMode::SlashMenu
                    | InputMode::SelectingTheme
                    | InputMode::SelectingProviders
                    | InputMode::SelectingModel
                    | InputMode::SelectingReasoning
            )
    }

    /// Helper to check if a slash or skill keypress should trigger the resolver menu.
    fn should_trigger_slash_menu(
        key_code: KeyCode,
        key_modifiers: KeyModifiers,
        input_mode: InputMode,
        input_is_empty: bool,
        shell_mode: bool,
        has_inline_prompt: bool,
        has_active_session: bool,
    ) -> bool {
        matches!(key_code, KeyCode::Char('/') | KeyCode::Char('$'))
            && key_modifiers.is_empty()
            && input_is_empty
            && has_active_session
            && !shell_mode
            && !has_inline_prompt
            && input_mode == InputMode::Normal
    }

    fn open_resolver_menu(&mut self, trigger: char) {
        let default_working_dir = self.config().working_dir.clone();
        let working_dir = self
            .state
            .tab_manager
            .active_session()
            .and_then(|session| session.working_dir.clone())
            .unwrap_or(default_working_dir);
        let entries = CommandResolver::menu_entries(&working_dir);
        self.state.close_overlays();
        self.state
            .slash_menu_state
            .show_with_entries(trigger, entries);
        self.state.input_mode = InputMode::SlashMenu;
    }

    fn slash_command_action(command: ConduitCommand) -> Option<Action> {
        match command {
            ConduitCommand::Model => Some(Action::ShowModelSelector),
            ConduitCommand::Reasoning => Some(Action::ShowReasoningSelector),
            ConduitCommand::Providers => Some(Action::ShowProvidersSelector),
            ConduitCommand::Fork => Some(Action::ForkSession),
            ConduitCommand::Handoff => Some(Action::HandoffSession),
            ConduitCommand::NewSession => None,
        }
    }

    fn execute_resolved_conduit_command(
        &mut self,
        tab_index: usize,
        command: ConduitCommand,
    ) -> anyhow::Result<Vec<Effect>> {
        if tab_index != self.state.tab_manager.active_index() {
            self.state.tab_manager.switch_to(tab_index);
            self.sync_input_mode_for_active_tab();
            self.sync_sidebar_to_active_tab();
            self.sync_footer_spinner();
        }
        let mut effects = Vec::new();
        if let Some(action) = Self::slash_command_action(command) {
            self.handle_global_action(action, &mut effects);
            Ok(effects)
        } else if matches!(command, ConduitCommand::NewSession) {
            self.start_new_session_in_place();
            Ok(Vec::new())
        } else {
            Ok(Vec::new())
        }
    }

    async fn read_bounded_output<R>(mut reader: R, limit: usize) -> io::Result<(Vec<u8>, bool)>
    where
        R: AsyncRead + Unpin,
    {
        let mut buf = Vec::with_capacity(limit.min(8192));
        let mut truncated = false;
        let mut chunk = [0u8; 8192];

        loop {
            let read = reader.read(&mut chunk).await?;
            if read == 0 {
                break;
            }

            if buf.len() < limit {
                let remaining = limit - buf.len();
                let take = remaining.min(read);
                buf.extend_from_slice(&chunk[..take]);
                if take < read {
                    truncated = true;
                }
            } else {
                truncated = true;
            }
        }

        Ok((buf, truncated))
    }

    async fn join_reader_with_timeout(
        mut task: tokio::task::JoinHandle<io::Result<(Vec<u8>, bool)>>,
        label: &'static str,
    ) -> Result<(Vec<u8>, bool, bool), String> {
        tokio::select! {
            res = &mut task => {
                let (bytes, truncated) = res
                    .map_err(|e| format!("Failed to run shell command: {e}"))?
                    .map_err(|e| format!("Failed to run shell command: {e}"))?;
                Ok((bytes, truncated, false))
            }
            _ = tokio::time::sleep(SHELL_COMMAND_REAP_TIMEOUT) => {
                task.abort();
                if let Err(err) = task.await {
                    tracing::debug!(
                        error = %err,
                        reader = label,
                        "Failed to abort reader task"
                    );
                }
                Ok((Vec::new(), true, true))
            }
        }
    }

    fn confirm_theme_picker(&mut self) -> anyhow::Result<Vec<Effect>> {
        let previous_theme_name = self.config().theme_name.clone();
        let previous_theme_path = self.config().theme_path.clone();

        let confirmed = self.state.theme_picker_state.confirm();
        if let Some(error) = self.state.theme_picker_state.take_error() {
            self.state
                .set_timed_footer_message(error, Duration::from_secs(5));
            return Ok(Vec::new());
        }

        if let Some(theme) = confirmed {
            let (name, path) = match &theme.source {
                crate::ui::components::ThemeSource::CustomPath { path } => {
                    (None, Some(path.clone()))
                }
                _ => (Some(theme.name.clone()), None),
            };
            let display_name = theme.display_name.clone();
            if let Err(err) = crate::config::save_theme_config(name.as_deref(), path.as_deref()) {
                self.config_mut().theme_name = previous_theme_name;
                self.config_mut().theme_path = previous_theme_path;
                self.state.theme_picker_state.hide(true); // Restore original theme
                                                          // Clear any pending theme picker error state.
                self.state.theme_picker_state.take_error();
                self.state.set_timed_footer_message(
                    format!("Failed to save theme: {err}"),
                    Duration::from_secs(5),
                );
                if !self.return_to_settings_menu_if_needed() {
                    self.state.input_mode = InputMode::Normal;
                }
                return Ok(Vec::new());
            }
            self.config_mut().theme_name = name;
            self.config_mut().theme_path = path;
            self.state.set_timed_footer_message(
                format!("Theme: {}", display_name),
                Duration::from_secs(3),
            );
        }

        self.state.theme_picker_state.hide(false); // Not cancelled
        if !self.return_to_settings_menu_if_needed() {
            self.state.input_mode = InputMode::Normal;
        }
        Ok(Vec::new())
    }

    /// Execute a command from command mode
    /// Returns an action to execute if the command maps to one
    fn execute_command(&mut self) -> Option<Action> {
        let command = std::mem::take(&mut self.state.command_buffer);
        let command = command.trim();
        self.state.input_mode = InputMode::Normal;

        // Check for :open command first (preserve path case, case-insensitive command)
        let mut parts = command.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        if cmd.eq_ignore_ascii_case("open") || cmd.eq_ignore_ascii_case("o") {
            if rest.is_empty() {
                self.state.set_timed_footer_message(
                    "Usage: :open <path>".to_string(),
                    Duration::from_secs(3),
                );
                return None;
            }

            let mut path = rest;
            // Allow quoted paths (common for paths with spaces)
            path = path
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| path.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(path);

            if !path.is_empty() {
                // Expand tilde to home directory
                // Only expand ~ or ~/path (not ~username which would require system lookup)
                let needs_home = path == "~" || path.starts_with("~/") || path.starts_with("~\\");

                if needs_home && dirs::home_dir().is_none() {
                    self.state.set_timed_footer_message(
                        "Home directory not found; cannot expand ~".to_string(),
                        Duration::from_secs(3),
                    );
                    return None;
                }

                let home = dirs::home_dir()?;
                let mut expanded_path = match path {
                    "~" => home,
                    _ => {
                        if let Some(rest) =
                            path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
                        {
                            home.join(rest)
                        } else {
                            std::path::PathBuf::from(path)
                        }
                    }
                };

                // Resolve relative paths against the active workspace (fallback to config working dir)
                if expanded_path.is_relative() {
                    let base_dir = self
                        .state
                        .tab_manager
                        .active_session()
                        .and_then(|s| s.working_dir.clone())
                        .unwrap_or_else(|| self.config().working_dir.clone());
                    expanded_path = base_dir.join(expanded_path);
                }

                return Some(Action::OpenFile(expanded_path));
            }
        }

        let command_lower = command.to_lowercase();

        // First check for built-in command aliases
        match command_lower.as_str() {
            "help" | "h" | "?" => {
                self.state.close_overlays();
                let keybindings = self.config().keybindings.clone();
                self.state.help_dialog_state.show(&keybindings);
                self.state.input_mode = InputMode::ShowingHelp;
                return None;
            }
            "q" => {
                return Some(Action::Quit);
            }
            _ => {}
        }

        // Try to parse as an action name
        parse_action(&command_lower)
    }

    /// Autocomplete the command buffer
    fn complete_command(&mut self) {
        let prefix = self.state.command_buffer.trim().to_lowercase();
        if prefix.is_empty() {
            return;
        }

        // Find all matching commands
        let matches: Vec<&str> = COMMAND_NAMES
            .iter()
            .filter(|cmd| cmd.starts_with(&prefix))
            .copied()
            .collect();

        if matches.is_empty() {
            return;
        }

        if matches.len() == 1 {
            // Single match - complete fully
            self.state.command_buffer = matches[0].to_string();
        } else {
            // Multiple matches - complete to longest common prefix
            let common = Self::longest_common_prefix(&matches);
            if common.len() > prefix.len() {
                self.state.command_buffer = common;
            } else {
                // Already at common prefix - cycle to next match
                let current = &self.state.command_buffer;
                let Some(next) = matches
                    .iter()
                    .find(|&&cmd| cmd > current.as_str())
                    .or(matches.first())
                else {
                    return;
                };
                self.state.command_buffer = (*next).to_string();
            }
        }
    }

    /// Find longest common prefix among strings
    fn longest_common_prefix(strings: &[&str]) -> String {
        if strings.is_empty() {
            return String::new();
        }
        if strings.len() == 1 {
            return strings[0].to_string();
        }

        let first = strings[0];
        let mut prefix_len = first.len();

        for s in &strings[1..] {
            prefix_len = first
                .chars()
                .zip(s.chars())
                .take_while(|(a, b)| a == b)
                .count()
                .min(prefix_len);
        }

        first[..prefix_len].to_string()
    }

    /// Open a workspace (create or switch to tab)
    /// If `close_sidebar` is true, the sidebar will be hidden after opening.
    fn open_workspace_with_options(&mut self, workspace_id: uuid::Uuid, close_sidebar: bool) {
        // Check if there's already a tab with this workspace - switch to it
        if let Some(existing_index) = self.find_tab_for_workspace(workspace_id) {
            self.state.tab_manager.switch_to(existing_index);
            self.sync_footer_spinner();
            if close_sidebar {
                self.state.sidebar_state.hide();
                self.state.input_mode = InputMode::Normal;
            }
            return;
        }

        // Find the workspace
        let Some(workspace_dao) = self.workspace_dao() else {
            return;
        };

        let Ok(Some(workspace)) = workspace_dao.get_by_id(workspace_id) else {
            return;
        };

        // Verify workspace path exists
        if !workspace.path.exists() {
            tracing::error!(
                workspace_id = %workspace_id,
                path = %workspace.path.display(),
                "Workspace path does not exist"
            );
            // TODO: Could offer to recreate the worktree or delete the workspace
            return;
        }

        // Get the repository name for the tab title
        let project_name = self
            .repo_dao()
            .and_then(|dao| dao.get_by_id(workspace.repository_id).ok().flatten())
            .map(|repo| repo.name);

        // Check if there's a saved session for this workspace (to restore chat history)
        let saved_tab = self
            .session_tab_dao()
            .and_then(|dao| dao.get_by_workspace_id(workspace_id).ok().flatten());

        // Update last accessed
        if let Err(e) = workspace_dao.update_last_accessed(workspace_id) {
            tracing::debug!(
                error = %e,
                workspace_id = %workspace_id,
                "Failed to update workspace last accessed time"
            );
        }

        let has_saved_session = saved_tab.is_some();
        let no_agents_available = !self.tools().is_available(crate::util::Tool::Claude)
            && !self.tools().is_available(crate::util::Tool::Codex)
            && !self.tools().is_available(crate::util::Tool::Gemini)
            && !self.tools().is_available(crate::util::Tool::Opencode);
        let tab_agent_type = saved_tab
            .as_ref()
            .map(|saved| saved.agent_type)
            .unwrap_or_else(|| {
                self.preferred_provider_for_new_sessions()
                    .unwrap_or(self.config().default_agent)
            });

        let saved_agent_mode = saved_tab.as_ref().map(|saved| {
            let parsed_mode = saved
                .agent_mode
                .as_deref()
                .map(AgentMode::parse)
                .unwrap_or_default();
            Self::clamp_agent_mode(saved.agent_type, parsed_mode)
        });

        let required_tool = Self::required_tool(tab_agent_type);
        if !self.tools().is_available(required_tool) {
            self.show_missing_tool(
                required_tool,
                if has_saved_session {
                    format!(
                        "{} is required to open this workspace's saved session.",
                        required_tool.display_name()
                    )
                } else if no_agents_available {
                    "An agent tool (Claude Code, Codex CLI, Gemini CLI, or OpenCode) is required to open this workspace."
                        .to_string()
                } else {
                    format!(
                        "{} is required to open this workspace.",
                        required_tool.display_name()
                    )
                },
            );
            if close_sidebar {
                self.state.sidebar_state.hide();
            }
            return;
        }

        // Create a new tab with the workspace's working directory
        if self
            .state
            .tab_manager
            .new_tab_with_working_dir(tab_agent_type, workspace.path.clone())
            .is_none()
        {
            self.show_error(
                "Too many tabs",
                "Maximum number of tabs reached. Close a tab before opening another workspace.",
            );
            return;
        }

        // Get default model before the mutable borrow
        let default_model = self.config().default_model_for(tab_agent_type);

        let session_tab_dao = self.session_tab_dao_clone();

        // Store workspace info in session and restore chat history if available
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.workspace_id = Some(workspace_id);
            session.project_name = project_name;
            session.workspace_name = Some(workspace.name.clone());

            // Restore saved session data if available
            if let Some(saved) = saved_tab.as_ref() {
                session.id = saved.id;
                if let Some(session_tab_dao) = session_tab_dao.as_ref() {
                    if let Err(e) = session_tab_dao.set_open(saved.id, true) {
                        tracing::warn!(error = %e, "Failed to mark saved session as open");
                    }
                }
            }
            if let Some(saved) = saved_tab {
                session.set_agent_and_model(saved.agent_type, saved.model);
                session.title = saved.title.clone();
                if let Some(saved_mode) = saved_agent_mode {
                    session.agent_mode = saved_mode; // Pre-clamped above
                }
                session.fork_seed_id = saved.fork_seed_id;

                // Restore chat history from agent files
                if let Some(ref session_id_str) = saved.agent_session_id {
                    let session_id = SessionId::from_string(session_id_str.clone());
                    session.resume_session_id = Some(session_id.clone());
                    if saved.agent_type != AgentType::Codex {
                        session.agent_session_id = Some(session_id);
                    }

                    // Load chat history
                    match saved.agent_type {
                        AgentType::Claude => {
                            if let Ok((msgs, debug_entries, file_path)) =
                                load_claude_history_with_debug(session_id_str)
                            {
                                // Populate debug pane with history load info
                                Self::populate_debug_from_history(
                                    &mut session.raw_events_view,
                                    &debug_entries,
                                    &file_path,
                                );
                                for msg in msgs {
                                    session.chat_view.push(msg);
                                }
                            }
                        }
                        AgentType::Codex => {
                            if let Ok((msgs, debug_entries, file_path)) =
                                load_codex_history_with_debug(session_id_str)
                            {
                                // Populate debug pane with history load info
                                Self::populate_debug_from_history(
                                    &mut session.raw_events_view,
                                    &debug_entries,
                                    &file_path,
                                );
                                for msg in msgs {
                                    session.chat_view.push(msg);
                                }
                            }
                        }
                        AgentType::Gemini => {
                            session.chat_view.push(
                                MessageDisplay::System {
                                    content: "Gemini CLI history import isn't supported yet, so previous messages won't be shown.".to_string(),
                                }
                                .to_chat_message(),
                            );
                        }
                        AgentType::Opencode => {
                            if let Ok((msgs, debug_entries, file_path)) =
                                load_opencode_history_with_debug(session_id_str)
                            {
                                Self::populate_debug_from_history(
                                    &mut session.raw_events_view,
                                    &debug_entries,
                                    &file_path,
                                );
                                for msg in msgs {
                                    session.chat_view.push(msg);
                                }
                            }
                        }
                    }
                } else if saved.agent_type == AgentType::Opencode {
                    if let Some(working_dir) = session.working_dir.as_ref() {
                        if let Ok((session_id_str, msgs, debug_entries, file_path)) =
                            load_opencode_history_for_dir_with_debug(working_dir)
                        {
                            let session_id = SessionId::from_string(session_id_str.clone());
                            session.resume_session_id = Some(session_id.clone());
                            session.agent_session_id = Some(session_id);

                            Self::populate_debug_from_history(
                                &mut session.raw_events_view,
                                &debug_entries,
                                &file_path,
                            );
                            for msg in msgs {
                                session.chat_view.push(msg);
                            }
                        }
                    }
                }

                // Restore pending user message if it exists and isn't already in history
                if let Some(ref pending) = saved.pending_user_message {
                    let already_in_history = session
                        .chat_view
                        .messages()
                        .iter()
                        .rev()
                        .find(|m| m.role == MessageRole::User)
                        .map(|m| m.content.as_str() == pending.as_str())
                        .unwrap_or(false);

                    if !already_in_history {
                        let display = MessageDisplay::User {
                            content: pending.clone(),
                        };
                        session.chat_view.push(display.to_chat_message());
                        session.pending_user_message = Some(pending.clone());
                    }
                }

                if !saved.queued_messages.is_empty() {
                    session.queued_messages = saved.queued_messages.clone();
                }

                // Derive fork_welcome_shown: if restoring a forked session that has messages,
                // the welcome message was already shown in the previous session
                if session.fork_seed_id.is_some() && !session.chat_view.messages().is_empty() {
                    session.fork_welcome_shown = true;
                }
            } else {
                session.model = Some(default_model.clone());
                session.model_invalid = false;
                session.init_context_for_model();
            }

            session.update_status();
        }

        // Register workspace with git tracker for background status updates
        if let Some(ref tracker) = self.git_tracker {
            tracker.track_workspace(workspace_id, workspace.path.clone());
        }

        // Close the sidebar and switch to normal mode (if requested)
        if close_sidebar {
            self.state.sidebar_state.hide();
            self.state.input_mode = InputMode::Normal;
        }
    }

    /// Open a workspace (create or switch to tab), closing the sidebar
    fn open_workspace(&mut self, workspace_id: uuid::Uuid) {
        self.open_workspace_with_options(workspace_id, true);
    }

    /// Clamp unsupported agent modes to a safe default.
    fn clamp_agent_mode(agent_type: AgentType, mode: AgentMode) -> AgentMode {
        if AgentCapabilities::for_agent(agent_type).supports_plan_mode {
            mode
        } else {
            AgentMode::Build
        }
    }

    /// Map an agent type to its required tool.
    fn required_tool(agent_type: AgentType) -> crate::util::Tool {
        match agent_type {
            AgentType::Claude => crate::util::Tool::Claude,
            AgentType::Codex => crate::util::Tool::Codex,
            AgentType::Gemini => crate::util::Tool::Gemini,
            AgentType::Opencode => crate::util::Tool::Opencode,
        }
    }

    fn reasoning_supported(agent_type: AgentType) -> bool {
        matches!(agent_type, AgentType::Claude | AgentType::Codex)
    }

    fn session_started(session: &AgentSession) -> bool {
        session.agent_session_id.is_some()
            || session.resume_session_id.is_some()
            || session.agent_input_tx.is_some()
            || session.turn_count > 0
    }

    fn reject_cross_agent_switch(session: &mut AgentSession, target_agent: AgentType) -> bool {
        if session.agent_type == target_agent || !Self::session_started(session) {
            return false;
        }

        let display = MessageDisplay::Error {
            content: "Switching agent type after a session has started is not supported. Start a new session/tab to change agents."
                .to_string(),
        };
        session.chat_view.push(display.to_chat_message());
        true
    }

    fn preferred_provider_for_new_sessions(&self) -> Option<AgentType> {
        let enabled = self.config().effective_enabled_providers(self.tools());
        if enabled.is_empty() {
            return None;
        }

        let default_provider = self.config().default_agent;
        if enabled.contains(&default_provider) {
            return Some(default_provider);
        }

        AgentType::preferred_order()
            .into_iter()
            .find(|provider| enabled.contains(provider))
    }

    fn preferred_provider_for_handoff(&self, source_agent: AgentType) -> AgentType {
        let enabled = self.config().effective_enabled_providers(self.tools());

        if let Some(provider) = AgentType::preferred_order()
            .into_iter()
            .find(|provider| enabled.contains(provider) && *provider != source_agent)
        {
            return provider;
        }

        if let Some(provider) = self.preferred_provider_for_new_sessions() {
            return provider;
        }

        source_agent
    }

    fn model_selector_defaults(&self) -> DefaultModelSelection {
        let agent_type = self
            .preferred_provider_for_new_sessions()
            .unwrap_or(self.config().default_agent);
        DefaultModelSelection {
            agent_type: Some(agent_type),
            model_id: Some(self.config().default_model_for(agent_type)),
        }
    }

    fn resolve_new_project_target(&self) -> NewProjectTarget {
        let base_dir = self
            .app_state_dao()
            .and_then(|dao| dao.get("projects_base_dir").ok().flatten());

        if let Some(base_dir_str) = base_dir {
            let base_path = if base_dir_str.starts_with('~') {
                dirs::home_dir()
                    .map(|h| h.join(base_dir_str[1..].trim_start_matches('/')))
                    .unwrap_or_else(|| PathBuf::from(&base_dir_str))
            } else {
                PathBuf::from(&base_dir_str)
            };
            NewProjectTarget::ProjectPicker(base_path)
        } else {
            NewProjectTarget::BaseDirDialog
        }
    }

    fn open_new_project_target(&mut self, target: NewProjectTarget) {
        match target {
            NewProjectTarget::ProjectPicker(base_path) => {
                self.start_project_discovery(base_path);
            }
            NewProjectTarget::BaseDirDialog => {
                self.state.base_dir_dialog_context = BaseDirDialogContext::ProjectDiscovery;
                self.state.base_dir_dialog_state.show();
                self.state.input_mode = InputMode::SettingBaseDir;
            }
        }
    }

    fn start_project_discovery(&mut self, base_dir: PathBuf) {
        self.state.close_overlays();
        self.state
            .project_picker_state
            .show_loading(base_dir.clone());
        self.state.input_mode = InputMode::PickingProject;

        let event_base_dir = base_dir.clone();
        self.spawn_blocking_preflight(
            move || {
                let projects = crate::ui::components::ProjectPickerState::scan_projects(base_dir)?;
                Ok(projects
                    .into_iter()
                    .map(|project| ProjectDiscoveryEntry {
                        name: project.name,
                        path: project.path,
                    })
                    .collect())
            },
            move |result| AppEvent::ProjectsDiscovered {
                base_dir: event_base_dir,
                result,
            },
            "projects_discovered",
        );
    }

    fn show_onboarding_provider_selector(&mut self) {
        self.state.provider_selector_state =
            crate::ui::components::ProviderSelectorState::configure_for(
                self.config(),
                self.tools(),
            );
        self.state.provider_selector_state.show();
        self.state.input_mode = InputMode::SelectingProviders;
    }

    fn show_onboarding_model_selector(&mut self) -> bool {
        let allowed = self.config().effective_enabled_providers(self.tools());
        if allowed.is_empty() {
            self.state.set_timed_footer_message(
                "No enabled providers available. Use /providers.".to_string(),
                Duration::from_secs(4),
            );
            self.state.pending_new_project_target = None;
            self.state.input_mode = InputMode::Normal;
            return false;
        }

        self.state
            .model_selector_state
            .set_allowed_providers(Some(allowed));
        self.state.model_selector_state.show_with_title(
            None,
            DefaultModelSelection::default(),
            "Pick your default model".to_string(),
        );
        self.state.model_picker_context = ModelPickerContext::OnboardingDefaultSelection;
        self.state.input_mode = InputMode::SelectingModel;
        true
    }

    fn continue_new_project_flow(&mut self) {
        if self.state.pending_new_project_target.is_none() {
            self.state.input_mode = InputMode::Normal;
            return;
        }

        if self.config().enabled_providers.is_none() {
            self.show_onboarding_provider_selector();
            return;
        }

        if self.config().default_model.is_none() {
            let _ = self.show_onboarding_model_selector();
            return;
        }

        if let Some(target) = self.state.pending_new_project_target.take() {
            self.open_new_project_target(target);
        } else {
            self.state.input_mode = InputMode::Normal;
        }
    }

    fn persist_default_model_selection(&mut self, model: &crate::agent::ModelInfo) -> bool {
        let model_id = model.id.clone();
        let agent_type = model.agent_type;
        self.state
            .model_selector_state
            .set_default_model(agent_type, model_id.clone());

        if let Err(err) = crate::core::services::ConfigService::set_default_model(
            &mut self.core,
            agent_type,
            &model_id,
        ) {
            tracing::warn!(error = %err, "Failed to save default model");
            self.state.set_timed_footer_message(
                format!("Failed to save default model: {err}"),
                Duration::from_secs(5),
            );
            return false;
        }

        self.state.set_timed_footer_message(
            format!("Default model set to: {}", model.display_name),
            Duration::from_secs(5),
        );
        true
    }

    fn open_project_picker_or_base_dir(&mut self) {
        self.state.close_overlays();
        self.state.pending_new_project_target = Some(self.resolve_new_project_target());
        self.continue_new_project_flow();
    }

    fn projects_base_dir_value(&self) -> String {
        self.app_state_dao()
            .and_then(|dao| dao.get("projects_base_dir").ok().flatten())
            .unwrap_or_else(|| "Not set".to_string())
    }

    fn settings_menu_entries(&self) -> Vec<SettingsMenuEntry> {
        let default_agent = self
            .preferred_provider_for_new_sessions()
            .unwrap_or(self.config().default_agent);
        let default_model_id = self.config().default_model_for(default_agent);
        let default_model = ModelRegistry::find_model(default_agent, &default_model_id)
            .map(|model| format!("{}: {}", default_agent.display_name(), model.display_name))
            .unwrap_or_else(|| format!("{}: {}", default_agent.display_name(), default_model_id));

        let enabled_providers = self
            .config()
            .effective_enabled_providers(self.tools())
            .into_iter()
            .map(|provider| provider.display_name().to_string())
            .collect::<Vec<_>>();

        let theme_value = self
            .config()
            .theme_name
            .clone()
            .or_else(|| {
                self.config()
                    .theme_path
                    .as_ref()
                    .map(|path| path.display().to_string())
            })
            .unwrap_or_else(crate::ui::components::current_theme_name);

        let workspace_defaults = format!(
            "{}, delete branch {}, remote prompt {}",
            self.config().workspaces.default_mode.as_str(),
            if self.config().workspaces.archive_delete_branch {
                "on"
            } else {
                "off"
            },
            if self.config().workspaces.archive_remote_prompt {
                "on"
            } else {
                "off"
            }
        );

        vec![
            SettingsMenuEntry {
                id: SettingsMenuEntryId::ProjectsDirectory,
                title: "Projects Directory".to_string(),
                description: "Where Conduit scans for local git projects".to_string(),
                value: self.projects_base_dir_value(),
            },
            SettingsMenuEntry {
                id: SettingsMenuEntryId::DefaultModel,
                title: "Default Model".to_string(),
                description: "Agent + model used for new sessions".to_string(),
                value: default_model,
            },
            SettingsMenuEntry {
                id: SettingsMenuEntryId::EnabledProviders,
                title: "Enabled Providers".to_string(),
                description: "Providers shown in model selection".to_string(),
                value: if enabled_providers.is_empty() {
                    "None".to_string()
                } else {
                    enabled_providers.join(", ")
                },
            },
            SettingsMenuEntry {
                id: SettingsMenuEntryId::Theme,
                title: "Theme".to_string(),
                description: "Active color theme".to_string(),
                value: theme_value,
            },
            SettingsMenuEntry {
                id: SettingsMenuEntryId::WorkspaceDefaults,
                title: "Workspace Defaults".to_string(),
                description: "Defaults applied when a repo has no override".to_string(),
                value: workspace_defaults,
            },
        ]
    }

    fn open_settings_menu(&mut self) {
        self.state.close_overlays();
        self.state
            .settings_menu_state
            .show(self.settings_menu_entries());
        self.state.input_mode = InputMode::SettingsMenu;
    }

    fn reopen_settings_menu(&mut self) {
        self.state
            .settings_menu_state
            .show(self.settings_menu_entries());
        self.state.input_mode = InputMode::SettingsMenu;
    }

    fn open_settings_child(&mut self) {
        self.state.settings_menu_state.hide();
        self.state.settings_menu_return = true;
    }

    fn return_to_settings_menu_if_needed(&mut self) -> bool {
        if self.state.settings_menu_return {
            self.state.settings_menu_return = false;
            self.reopen_settings_menu();
            return true;
        }
        false
    }

    fn open_selected_setting(&mut self) {
        let Some(entry) = self.state.settings_menu_state.selected_entry().cloned() else {
            return;
        };

        match entry.id {
            SettingsMenuEntryId::ProjectsDirectory => {
                self.open_settings_child();
                self.state.base_dir_dialog_context = BaseDirDialogContext::Settings;
                if let Some(dao) = self.app_state_dao() {
                    if let Ok(Some(current_dir)) = dao.get("projects_base_dir") {
                        self.state
                            .base_dir_dialog_state
                            .show_with_path(&current_dir);
                    } else {
                        self.state.base_dir_dialog_state.show();
                    }
                } else {
                    self.state.base_dir_dialog_state.show();
                }
                self.state.input_mode = InputMode::SettingBaseDir;
            }
            SettingsMenuEntryId::DefaultModel => {
                let allowed = self.config().effective_enabled_providers(self.tools());
                if allowed.is_empty() {
                    self.state.set_timed_footer_message(
                        "No enabled providers available. Use providers first.".to_string(),
                        Duration::from_secs(4),
                    );
                    return;
                }
                self.open_settings_child();
                self.state
                    .model_selector_state
                    .set_allowed_providers(Some(allowed));
                self.state.model_selector_state.show_with_title(
                    None,
                    self.model_selector_defaults(),
                    "Pick your default model".to_string(),
                );
                self.state.model_picker_context = ModelPickerContext::SettingsDefaultSelection;
                self.state.input_mode = InputMode::SelectingModel;
            }
            SettingsMenuEntryId::EnabledProviders => {
                self.open_settings_child();
                self.state.provider_selector_state =
                    crate::ui::components::ProviderSelectorState::configure_for(
                        self.config(),
                        self.tools(),
                    );
                self.state.provider_selector_state.show();
                self.state.input_mode = InputMode::SelectingProviders;
            }
            SettingsMenuEntryId::Theme => {
                self.open_settings_child();
                let theme_path = self.config().theme_path.clone();
                self.state.theme_picker_state.show(theme_path.as_deref());
                self.state.input_mode = InputMode::SelectingTheme;
            }
            SettingsMenuEntryId::WorkspaceDefaults => {
                self.open_settings_child();
                self.state
                    .workspace_defaults_dialog_state
                    .show(WorkspaceDefaultsDraft {
                        mode: self.config().workspaces.default_mode,
                        archive_delete_branch: self.config().workspaces.archive_delete_branch,
                        archive_remote_prompt: self.config().workspaces.archive_remote_prompt,
                    });
                self.state.input_mode = InputMode::WorkspaceDefaults;
            }
        }
    }

    /// Show missing tool dialog and enter MissingTool mode.
    fn show_missing_tool(&mut self, tool: crate::util::Tool, message: impl Into<String>) {
        self.state.close_overlays();
        self.state
            .missing_tool_dialog_state
            .show_with_context(tool, message);
        self.state.input_mode = InputMode::MissingTool;
    }

    /// Find the tab index for a workspace if it's already open
    fn find_tab_for_workspace(&self, workspace_id: uuid::Uuid) -> Option<usize> {
        self.state.tab_manager.tabs().iter().position(|tab| {
            tab.as_agent()
                .is_some_and(|session| session.workspace_id == Some(workspace_id))
        })
    }

    /// Extract PR number from text containing a GitHub PR URL
    /// Looks for patterns like "github.com/owner/repo/pull/123"
    fn extract_pr_number_from_text(text: &str) -> Option<u32> {
        // Look for GitHub PR URLs in the text
        for word in text.split_whitespace() {
            // Check if this word contains a GitHub PR URL
            if let Some(pull_idx) = word.find("/pull/") {
                // Extract the part after "/pull/"
                let after_pull = &word[pull_idx + 6..];
                // Parse the number (stop at any non-digit character)
                let num_str: String = after_pull
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !num_str.is_empty() {
                    if let Ok(num) = num_str.parse::<u32>() {
                        return Some(num);
                    }
                }
            }
        }
        None
    }

    /// Build a minimal PR status from a known PR number (used when full status is unavailable).
    fn synthesize_pr_status(number: u32) -> PrStatus {
        PrStatus {
            exists: true,
            number: Some(number),
            ..Default::default()
        }
    }

    /// Apply PR status to a session and return the workspace_id for sidebar updates.
    fn apply_pr_status_to_session(
        session: &mut AgentSession,
        mut status: PrStatus,
    ) -> Option<(Uuid, PrStatus)> {
        let effective_number = status.number.or(session.pr_number);
        status.number = effective_number;
        session.pr_number = effective_number;
        session.status_bar.set_pr_status(Some(status.clone()));
        session.workspace_id.map(|id| (id, status))
    }

    fn apply_pr_number_to_session(
        session: &mut AgentSession,
        pr_num: u32,
    ) -> Option<(Uuid, PrStatus)> {
        let status = Self::synthesize_pr_status(pr_num);
        Self::apply_pr_status_to_session(session, status)
    }

    /// Estimate token usage for a prompt (rough heuristic)
    fn estimate_tokens(text: &str) -> i64 {
        let chars = text.chars().count().max(1);
        ((chars as f64) / 4.0).ceil() as i64
    }

    /// Populate the debug pane with history loading debug entries
    fn populate_debug_from_history(
        raw_events_view: &mut crate::ui::components::RawEventsView,
        debug_entries: &[HistoryDebugEntry],
        file_path: &std::path::Path,
    ) {
        use crate::ui::components::EventDirection;

        // First, add a header entry showing the file being loaded
        let header_json = serde_json::json!({
            "action": "history_load",
            "file": file_path.to_string_lossy(),
            "total_entries": debug_entries.len(),
            "included": debug_entries.iter().filter(|e| e.status == "INCLUDE").count(),
            "skipped": debug_entries.iter().filter(|e| e.status == "SKIP").count(),
        });
        raw_events_view.push_event(EventDirection::Received, "history_load", header_json);

        // Add each debug entry
        for entry in debug_entries {
            // Create a summary JSON that includes status info
            let summary_json = serde_json::json!({
                "line": entry.line_number,
                "type": entry.entry_type,
                "status": entry.status,
                "reason": entry.reason,
                "raw": entry.raw_json,
            });

            let event_type = format!(
                "L{} {} {}",
                entry.line_number, entry.status, entry.entry_type
            );
            raw_events_view.push_event(EventDirection::Received, event_type, summary_json);
        }
    }

    /// Schedule the workspace creation process for a repository.
    fn start_workspace_creation(&mut self, repo_id: uuid::Uuid) -> Option<Effect> {
        let repo_dao = self.repo_dao()?;

        let Ok(Some(repo)) = repo_dao.get_by_id(repo_id) else {
            tracing::error!(repo_id = %repo_id, "Repository not found");
            return None;
        };

        if repo.workspace_mode.is_none() {
            let description = format!(
                "Choose how Conduit should create workspaces for \"{}\".\n\nYou can change this later when no active workspaces exist.",
                repo.name
            );
            self.state.close_overlays();
            self.state.confirmation_dialog_state.show(
                "Select Workspace Mode",
                description,
                Vec::new(),
                ConfirmationType::Info,
                "Use Worktrees",
                Some(ConfirmationContext::SelectWorkspaceMode { repo_id }),
            );
            self.state.confirmation_dialog_state.cancel_text = "Use Checkouts".to_string();
            if self.config().workspaces.default_mode == WorkspaceMode::Worktree {
                self.state.confirmation_dialog_state.select_confirm();
            } else {
                self.state.confirmation_dialog_state.select_cancel();
            }
            self.state.input_mode = InputMode::Confirming;
            return None;
        }

        self.mark_repo_action_busy(repo_id);
        Some(Effect::CreateWorkspace { repo_id })
    }

    /// Find the visible index of a workspace by its ID
    fn find_workspace_index(&self, workspace_id: uuid::Uuid) -> Option<usize> {
        use crate::ui::components::NodeType;
        self.state
            .sidebar_data
            .visible_nodes()
            .iter()
            .position(|node| node.id == workspace_id && node.node_type == NodeType::Workspace)
    }

    /// Sync sidebar selection to the active tab's workspace (if sidebar is visible)
    fn sync_sidebar_to_active_tab(&mut self) {
        if !self.state.sidebar_state.visible {
            return;
        }
        if let Some(session) = self.state.tab_manager.active_session() {
            if let Some(workspace_id) = session.workspace_id {
                if let Some(index) = self.state.sidebar_data.focus_workspace(workspace_id) {
                    self.state.sidebar_state.tree_state.selected = index;
                }
            }
        }
    }

    /// Keep the non-modal input mode aligned with the currently active tab type.
    fn sync_input_mode_for_active_tab(&mut self) {
        match self.state.input_mode {
            InputMode::Normal | InputMode::Scrolling | InputMode::FileViewer => {
                if self.state.tab_manager.active_is_file() {
                    self.state.input_mode = InputMode::FileViewer;
                } else if self.state.input_mode == InputMode::FileViewer {
                    self.state.input_mode = InputMode::Normal;
                }
            }
            _ => {}
        }
    }

    /// Sync footer spinner state to the active tab's processing state
    fn sync_footer_spinner(&mut self) {
        let active_session = self.state.tab_manager.active_session();
        let is_active_processing = active_session.is_some_and(|s| s.is_processing);
        let has_inline_prompt = active_session.is_some_and(|s| s.inline_prompt.is_some());

        // Don't show spinner when awaiting user response (inline prompt active)
        if is_active_processing && !has_inline_prompt {
            // Start spinner if active tab is processing and spinner not already running
            if self.state.footer_spinner.is_none() {
                self.state.start_footer_spinner(None);
            }
        } else if self.state.footer_spinner.is_some() {
            // Stop spinner if not processing, or awaiting response
            self.state.stop_footer_spinner();
        }
    }

    /// Dismiss the confirmation dialog and clean up fork state if applicable.
    /// Returns the input mode to transition to.
    fn dismiss_confirmation_dialog(&mut self) -> InputMode {
        // Cache context before hide() clears it
        let ctx = self.state.confirmation_dialog_state.context.clone();

        // Clear pending fork request if dismissing a fork confirmation
        if matches!(
            &ctx,
            Some(ConfirmationContext::ForkSession { .. })
                | Some(ConfirmationContext::ForkSessionPreflightInProgress { .. })
        ) {
            self.state.pending_fork_request = None;
        }

        self.state.confirmation_dialog_state.hide();

        // Return appropriate input mode based on context
        match ctx {
            // PR/Fork/Steer dialogs originated from chat view, return to Normal
            Some(ConfirmationContext::CreatePullRequest { .. })
            | Some(ConfirmationContext::OpenExistingPr { .. })
            | Some(ConfirmationContext::ForkSession { .. })
            | Some(ConfirmationContext::ForkSessionPreflightInProgress { .. })
            | Some(ConfirmationContext::SteerFallback { .. }) => InputMode::Normal,
            // Sidebar operations return to sidebar navigation
            Some(ConfirmationContext::ArchiveWorkspace(_))
            | Some(ConfirmationContext::ArchiveWorkspaceRemoteDelete { .. })
            | Some(ConfirmationContext::ArchiveWorkspacePreflightInProgress { .. })
            | Some(ConfirmationContext::ArchiveWorkspaceInProgress { .. })
            | Some(ConfirmationContext::RemoveProject(_))
            | Some(ConfirmationContext::RemoveProjectPreflightInProgress { .. })
            | Some(ConfirmationContext::SelectWorkspaceMode { .. }) => InputMode::SidebarNavigation,
            // No context: return to Normal if tabs exist, otherwise SidebarNavigation
            // (avoids unexpectedly flipping to sidebar when user has active tabs)
            None => {
                if !self.state.tab_manager.is_empty() {
                    InputMode::Normal
                } else {
                    InputMode::SidebarNavigation
                }
            }
        }
    }

    fn is_blocking_confirmation_loading_dialog(&self) -> bool {
        self.state.confirmation_dialog_state.visible
            && self.state.confirmation_dialog_state.loading
            && self
                .state
                .confirmation_dialog_state
                .context
                .as_ref()
                .is_some_and(ConfirmationContext::is_blocking_loading)
    }

    fn show_blocking_confirmation_loading(
        &mut self,
        title: impl Into<String>,
        loading_message: impl Into<String>,
        context: ConfirmationContext,
    ) {
        self.state.close_overlays();
        self.state
            .confirmation_dialog_state
            .show_loading_with_context(title, loading_message, Some(context));
        self.state.input_mode = InputMode::Confirming;
    }

    fn spawn_blocking_preflight<T, W, E>(&self, work: W, event_builder: E, context: &'static str)
    where
        T: Send + 'static,
        W: FnOnce() -> Result<T, String> + Send + 'static,
        E: FnOnce(Result<T, String>) -> AppEvent + Send + 'static,
    {
        let event_tx = self.event_tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = work();
            send_app_event(&event_tx, event_builder(result), context);
        });
    }

    fn show_archive_progress_dialog(&mut self, workspace_id: uuid::Uuid) {
        self.state.close_overlays();
        self.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Archive Workspace",
                "Archiving workspace...",
                Some(ConfirmationContext::ArchiveWorkspaceInProgress { workspace_id }),
            );
        self.state.input_mode = InputMode::Confirming;
    }

    fn hide_archive_progress_dialog(&mut self, workspace_id: uuid::Uuid) {
        let is_matching_archive_progress = self.state.confirmation_dialog_state.loading
            && matches!(
                self.state.confirmation_dialog_state.context,
                Some(ConfirmationContext::ArchiveWorkspaceInProgress {
                    workspace_id: id
                }) if id == workspace_id
            );
        if is_matching_archive_progress {
            self.state.confirmation_dialog_state.hide();
            self.state.input_mode = InputMode::SidebarNavigation;
        }
    }

    /// Initiate the archive workspace flow - check git status and show confirmation dialog
    fn initiate_archive_workspace(&mut self, workspace_id: uuid::Uuid) {
        if self.state.busy_workspaces.contains(&workspace_id) {
            self.state.set_timed_footer_message(
                "Archive already in progress for this workspace".to_string(),
                Duration::from_secs(3),
            );
            return;
        }

        self.show_blocking_confirmation_loading(
            "Archive Workspace",
            "Analyzing workspace...",
            ConfirmationContext::ArchiveWorkspacePreflightInProgress { workspace_id },
        );

        let workspace_dao = self.workspace_dao_clone();
        let repo_dao = self.repo_dao_clone();
        let worktree_manager = self.worktree_manager().clone();
        let config = self.config().clone();

        self.spawn_blocking_preflight(
            move || {
                let workspace_dao =
                    workspace_dao.ok_or_else(|| "Workspace database unavailable".to_string())?;
                let repo_dao =
                    repo_dao.ok_or_else(|| "Repository database unavailable".to_string())?;

                let workspace = workspace_dao
                    .get_by_id(workspace_id)
                    .map_err(|e| format!("Failed to load workspace: {}", e))?
                    .ok_or_else(|| "Workspace not found".to_string())?;
                let repo = repo_dao
                    .get_by_id(workspace.repository_id)
                    .map_err(|e| format!("Failed to load repository: {}", e))?
                    .ok_or_else(|| "Repository not found for workspace".to_string())?;
                let settings = resolve_repo_workspace_settings(&config, &repo);

                let branch_status = worktree_manager.get_branch_status(&workspace.path);
                let mut warnings = Vec::new();
                let mut has_dirty = false;
                let mut has_unmerged = false;

                if let Ok(status) = branch_status {
                    if status.is_dirty {
                        has_dirty = true;
                        if let Some(desc) = &status.dirty_description {
                            warnings.push(desc.clone());
                        } else {
                            warnings.push("Uncommitted changes".to_string());
                        }
                    }

                    if !status.is_merged {
                        has_unmerged = true;
                        if status.commits_ahead > 0 {
                            warnings.push(format!(
                                "Branch not merged ({} commits ahead)",
                                status.commits_ahead
                            ));
                        } else {
                            warnings.push("Branch not merged into main".to_string());
                        }
                    }

                    if status.commits_behind > 0 {
                        warnings.push(format!(
                            "Branch is {} commits behind main",
                            status.commits_behind
                        ));
                    }
                }

                let mut message = match settings.mode {
                    WorkspaceMode::Worktree => "This will remove the worktree.".to_string(),
                    WorkspaceMode::Checkout => "This will remove the checkout.".to_string(),
                };

                if settings.archive_delete_branch {
                    message.push_str(" The local branch will be deleted.");
                }
                if settings.archive_delete_branch && settings.archive_remote_prompt {
                    message.push_str(" You'll be asked about deleting the remote branch.");
                }

                Ok(ArchiveWorkspaceDialogPreflightResult {
                    workspace_name: workspace.name,
                    message,
                    warnings,
                    has_dirty,
                    has_unmerged,
                })
            },
            move |result| AppEvent::ArchiveWorkspaceDialogPreflightCompleted {
                workspace_id,
                result,
            },
            "archive_workspace_dialog_preflight_completed",
        );
    }

    /// Show an error dialog with a simple message
    fn show_error(&mut self, title: &str, message: &str) {
        self.state.close_overlays();
        self.state.error_dialog_state.show(title, message);
        self.state.input_mode = InputMode::ShowingError;
    }

    /// Show an error dialog with technical details
    fn show_error_with_details(&mut self, title: &str, message: &str, details: &str) {
        self.state.close_overlays();
        self.state
            .error_dialog_state
            .show_with_details(title, message, details);
        self.state.input_mode = InputMode::ShowingError;
    }

    /// Execute the archive workspace action after confirmation
    fn execute_archive_workspace_preflight(&mut self, workspace_id: uuid::Uuid) -> Effect {
        self.mark_workspace_busy(workspace_id);
        self.show_archive_progress_dialog(workspace_id);
        Effect::ArchiveWorkspacePreflight { workspace_id }
    }

    /// Execute the archive workspace action after confirmation
    fn execute_archive_workspace(
        &mut self,
        workspace_id: uuid::Uuid,
        delete_remote: bool,
    ) -> Effect {
        self.mark_workspace_busy(workspace_id);
        self.show_archive_progress_dialog(workspace_id);
        Effect::ArchiveWorkspace {
            workspace_id,
            delete_remote,
        }
    }

    fn prompt_archive_remote_delete(&mut self, workspace: &crate::data::Workspace) {
        self.state.close_overlays();
        self.state.confirmation_dialog_state.show(
            format!("Delete remote branch for \"{}\"?", workspace.name),
            format!(
                "Delete branch '{}' from the remote repository?",
                workspace.branch
            ),
            Vec::new(),
            ConfirmationType::Warning,
            "Delete Remote",
            Some(ConfirmationContext::ArchiveWorkspaceRemoteDelete {
                workspace_id: workspace.id,
            }),
        );
        self.state.confirmation_dialog_state.cancel_text = "Keep Remote".to_string();
        self.state.confirmation_dialog_state.select_cancel();
        self.state.input_mode = InputMode::Confirming;
    }

    fn apply_repo_workspace_mode(
        &mut self,
        repo_id: uuid::Uuid,
        mode: WorkspaceMode,
    ) -> Result<(), String> {
        let repo_dao = self
            .repo_dao()
            .ok_or_else(|| "Repository database unavailable".to_string())?;
        let workspace_dao = self
            .workspace_dao()
            .ok_or_else(|| "Workspace database unavailable".to_string())?;

        let repo = repo_dao
            .get_by_id(repo_id)
            .map_err(|e| format!("Failed to load repository: {}", e))?
            .ok_or_else(|| "Repository not found".to_string())?;

        if let Some(existing_mode) = repo.workspace_mode {
            if existing_mode == mode {
                return Ok(());
            }
        }

        let active_count = workspace_dao
            .count_active_by_repository(repo_id)
            .map_err(|e| format!("Failed to check workspaces: {}", e))?;
        if active_count > 0 {
            return Err("Cannot change workspace mode while active workspaces exist.".to_string());
        }

        repo_dao
            .update_settings(
                repo_id,
                Some(mode),
                repo.archive_delete_branch,
                repo.archive_remote_prompt,
            )
            .map_err(|e| format!("Failed to update repository settings: {}", e))?;

        Ok(())
    }

    /// Initiate project removal - shows confirmation dialog
    fn initiate_remove_project(&mut self, repo_id: uuid::Uuid) {
        self.show_blocking_confirmation_loading(
            "Remove Project",
            "Analyzing project workspaces...",
            ConfirmationContext::RemoveProjectPreflightInProgress { repo_id },
        );

        let repo_dao = self.repo_dao_clone();
        let workspace_dao = self.workspace_dao_clone();
        let worktree_manager = self.worktree_manager().clone();

        self.spawn_blocking_preflight(
            move || {
                let repo_dao =
                    repo_dao.ok_or_else(|| "Repository database unavailable".to_string())?;
                let workspace_dao =
                    workspace_dao.ok_or_else(|| "Workspace database unavailable".to_string())?;

                let repo = repo_dao
                    .get_by_id(repo_id)
                    .map_err(|e| format!("Failed to load repository: {}", e))?
                    .ok_or_else(|| "Repository not found".to_string())?;

                let workspaces = workspace_dao
                    .get_by_repository(repo_id)
                    .map_err(|e| format!("Failed to load workspaces: {}", e))?;

                let mut warnings = Vec::new();
                let mut has_dirty = false;
                let mut has_unmerged = false;

                for workspace in &workspaces {
                    if let Ok(status) = worktree_manager.get_branch_status(&workspace.path) {
                        if status.is_dirty {
                            has_dirty = true;
                        }
                        if !status.is_merged {
                            has_unmerged = true;
                        }
                    }
                }

                let workspace_count = workspaces.len();
                if workspace_count > 0 {
                    warnings.push(format!(
                        "{} workspace{} will be archived",
                        workspace_count,
                        if workspace_count == 1 { "" } else { "s" }
                    ));
                }
                if has_dirty {
                    warnings.push("Some workspaces have uncommitted changes".to_string());
                }
                if has_unmerged {
                    warnings.push("Some branches are not merged to main".to_string());
                }

                Ok(RemoveProjectDialogPreflightResult {
                    repo_name: repo.name,
                    warnings,
                    has_dirty,
                    has_unmerged,
                    workspace_count,
                })
            },
            move |result| AppEvent::RemoveProjectDialogPreflightCompleted { repo_id, result },
            "remove_project_dialog_preflight_completed",
        );
    }

    /// Execute project removal after confirmation
    fn execute_remove_project(&mut self, repo_id: uuid::Uuid) -> Effect {
        // Set spinner mode
        self.state.input_mode = InputMode::RemovingProject;
        self.mark_repo_busy(repo_id);

        Effect::RemoveProject { repo_id }
    }

    fn close_tab_at_index(&mut self, index: usize) {
        if let Some(session) = self.state.tab_manager.session(index) {
            if let Some(session_tab_dao) = self.session_tab_dao_clone() {
                if let Err(e) = session_tab_dao.set_open(session.id, false) {
                    tracing::warn!(error = %e, "Failed to mark session as closed");
                }
            }
        }
        self.state.tab_manager.close_tab(index);
    }

    /// Close any tabs that are using the specified workspace
    fn close_tabs_for_workspace(&mut self, workspace_id: uuid::Uuid) {
        // Unregister workspace from git tracker
        if let Some(ref tracker) = self.git_tracker {
            tracker.untrack_workspace(workspace_id);
        }

        // Find tabs with this workspace and close them (in reverse order to maintain indices)
        let indices_to_close: Vec<usize> = self
            .state
            .tab_manager
            .sessions()
            .iter()
            .enumerate()
            .filter_map(|(idx, session)| {
                if session.workspace_id == Some(workspace_id) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        for idx in indices_to_close.into_iter().rev() {
            self.stop_agent_for_tab(idx);
            self.close_tab_at_index(idx);
        }

        // Switch to sidebar navigation if all tabs are closed
        // But don't override if we're showing an error dialog
        if self.state.tab_manager.is_empty() && self.state.input_mode != InputMode::ShowingError {
            self.state.sidebar_state.visible = true;
            self.state.input_mode = InputMode::SidebarNavigation;
        }
    }

    /// Add a project to the sidebar (repository only, no workspace)
    /// Returns the repository ID - either existing or newly created
    fn add_project_to_sidebar(&mut self, path: std::path::PathBuf) -> Option<uuid::Uuid> {
        let repo_dao = self.repo_dao()?;

        // Check if project already exists
        if let Ok(Some(existing_repo)) = repo_dao.get_by_path(&path) {
            // Project already exists, just return its ID (caller will expand/select it)
            return Some(existing_repo.id);
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Create repository (without default workspace)
        let repo = Repository::from_local_path(&name, path);
        if repo_dao.create(&repo).is_err() {
            return None;
        }

        let repo_id = repo.id;

        // Refresh sidebar
        self.refresh_sidebar_data();

        Some(repo_id)
    }

    /// Add a repository from the custom path dialog
    /// Returns the repository ID - either existing or newly created
    fn add_repository(&mut self) -> Option<uuid::Uuid> {
        let path = self.state.add_repo_dialog_state.expanded_path();

        let repo_dao = self.repo_dao()?;

        // Check if project already exists
        if let Ok(Some(existing_repo)) = repo_dao.get_by_path(&path) {
            // Project already exists, just return its ID (caller will expand/select it)
            return Some(existing_repo.id);
        }

        let name = self
            .state
            .add_repo_dialog_state
            .repo_name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        // Create repository (without default workspace)
        let repo = Repository::from_local_path(&name, path);
        if repo_dao.create(&repo).is_err() {
            return None;
        }

        let repo_id = repo.id;

        // Refresh sidebar
        self.refresh_sidebar_data();

        Some(repo_id)
    }

    /// Create a new tab with the selected agent type
    fn create_tab_with_agent(&mut self, agent_type: AgentType) {
        let target_provider = if self
            .config()
            .is_provider_enabled_effective(agent_type, self.tools())
        {
            agent_type
        } else {
            self.preferred_provider_for_new_sessions()
                .unwrap_or(agent_type)
        };

        self.state.tab_manager.new_tab(target_provider);
        let model_id = self.config().default_model_for(target_provider);
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.model = Some(model_id);
            session.model_invalid = false;
            session.init_context_for_model();
            session.update_status();
        }
        self.state.input_mode = InputMode::Normal;
    }

    /// Replace the active session with a fresh one (same workspace, reset history).
    fn start_new_session_in_place(&mut self) {
        if self.state.tab_manager.is_empty() {
            self.state.set_timed_footer_message(
                "No active session to reset".to_string(),
                Duration::from_secs(3),
            );
            return;
        }

        let active_index = self.state.tab_manager.active_index();
        let (
            agent_type,
            working_dir,
            workspace_id,
            project_name,
            workspace_name,
            pr_number,
            is_processing,
        ) = match self.state.tab_manager.session(active_index) {
            Some(session) => (
                session.agent_type,
                session.working_dir.clone(),
                session.workspace_id,
                session.project_name.clone(),
                session.workspace_name.clone(),
                session.pr_number,
                session.is_processing,
            ),
            None => {
                self.state.set_timed_footer_message(
                    "No active session to reset".to_string(),
                    Duration::from_secs(3),
                );
                return;
            }
        };

        if is_processing {
            self.state.set_timed_footer_message(
                "Stop the agent before starting a new session".to_string(),
                Duration::from_secs(3),
            );
            return;
        }

        let mut new_session = if let Some(dir) = working_dir {
            AgentSession::with_working_dir(agent_type, dir)
        } else {
            AgentSession::new(agent_type)
        };
        new_session.workspace_id = workspace_id;
        new_session.project_name = project_name;
        new_session.workspace_name = workspace_name;
        new_session.pr_number = pr_number;
        new_session.model = Some(self.config().default_model_for(agent_type));
        new_session.model_invalid = false;
        new_session.init_context_for_model();
        new_session.update_status();

        if let Some(session) = self.state.tab_manager.session_mut(active_index) {
            *session = new_session;
        }

        self.state
            .set_timed_footer_message("Started a new session".to_string(), Duration::from_secs(3));
    }

    /// Create a new tab by importing an external session
    async fn create_imported_session_tab(
        &mut self,
        agent_type: AgentType,
        session_file: std::path::PathBuf,
        working_dir: std::path::PathBuf,
    ) -> anyhow::Result<()> {
        // Extract session ID from the file path
        let session_id_str = session_file
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Create a new session with working directory
        let mut session = AgentSession::with_working_dir(agent_type, working_dir);
        // Set both resume and agent session IDs so the session can be restored after restart
        let session_id = SessionId::from_string(&session_id_str);
        session.resume_session_id = Some(session_id.clone());
        if agent_type != AgentType::Codex {
            session.agent_session_id = Some(session_id);
        }

        // Load history based on agent type
        match agent_type {
            AgentType::Claude => {
                if let Ok((msgs, debug_entries, file_path)) =
                    load_claude_history_with_debug(&session_id_str)
                {
                    Self::populate_debug_from_history(
                        &mut session.raw_events_view,
                        &debug_entries,
                        &file_path,
                    );
                    for msg in msgs {
                        session.chat_view.push(msg);
                    }
                }
            }
            AgentType::Codex => {
                if let Ok((msgs, debug_entries, file_path)) =
                    load_codex_history_with_debug(&session_id_str)
                {
                    Self::populate_debug_from_history(
                        &mut session.raw_events_view,
                        &debug_entries,
                        &file_path,
                    );
                    for msg in msgs {
                        session.chat_view.push(msg);
                    }
                }
            }
            AgentType::Gemini => {
                session.resume_session_id = None;
                session.agent_session_id = None;
                session.chat_view.push(
                    MessageDisplay::System {
                        content: "Gemini CLI session import isn't supported yet.".to_string(),
                    }
                    .to_chat_message(),
                );
            }
            AgentType::Opencode => {
                if let Ok((msgs, debug_entries, file_path)) =
                    load_opencode_history_with_debug(&session_id_str)
                {
                    Self::populate_debug_from_history(
                        &mut session.raw_events_view,
                        &debug_entries,
                        &file_path,
                    );
                    for msg in msgs {
                        session.chat_view.push(msg);
                    }
                }
            }
        }

        session.update_status();

        // Add the session to the tab manager
        self.state.tab_manager.add_session(session);

        // Switch to the new tab
        let tab_count = self.state.tab_manager.sessions().len();
        self.state
            .tab_manager
            .switch_to(tab_count.saturating_sub(1));
        self.sync_footer_spinner();

        Ok(())
    }

    /// Handle a mouse click at the given position.
    async fn handle_mouse_click(
        &mut self,
        x: u16,
        y: u16,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();

        // Handle confirmation dialog - close on any click outside
        // Use same context-aware logic as Cancel action for consistent UX
        if self.state.input_mode == InputMode::Confirming
            && self.state.confirmation_dialog_state.visible
        {
            if self.is_blocking_confirmation_loading_dialog() {
                return Ok(effects);
            }
            self.state.input_mode = self.dismiss_confirmation_dialog();
            return Ok(effects);
        }

        // Handle model selector clicks first (it's a modal dialog)
        if self.state.input_mode == InputMode::SelectingModel
            && self.state.model_selector_state.is_visible()
        {
            effects.extend(self.handle_model_selector_click(x, y));
            return Ok(effects);
        }

        if self.state.input_mode == InputMode::SelectingReasoning
            && self.state.reasoning_selector_state.is_visible()
        {
            self.handle_reasoning_selector_click(x, y);
            return Ok(effects);
        }

        if self.state.input_mode == InputMode::SelectingProviders
            && self.state.provider_selector_state.is_visible()
        {
            self.handle_provider_selector_click(x, y);
            return Ok(effects);
        }

        if self.state.input_mode == InputMode::SettingsMenu
            || self.state.settings_menu_state.is_visible()
        {
            self.handle_settings_menu_click(x, y);
            return Ok(effects);
        }

        if self.state.input_mode == InputMode::WorkspaceDefaults
            || self.state.workspace_defaults_dialog_state.is_visible()
        {
            self.handle_workspace_defaults_click(x, y);
            return Ok(effects);
        }

        // Handle project picker clicks first (it's a modal dialog)
        if self.state.input_mode == InputMode::PickingProject
            && self.state.project_picker_state.is_visible()
        {
            self.handle_project_picker_click(x, y);
            return Ok(effects);
        }

        // Check sidebar first (if visible)
        if let Some(sidebar_area) = self.state.sidebar_area {
            if Self::point_in_rect(x, y, sidebar_area) {
                if let Some(effect) = self.handle_sidebar_click(x, y, sidebar_area) {
                    effects.push(effect);
                }
                return Ok(effects);
            }
        }

        // Check tab bar
        if let Some(tab_bar_area) = self.state.tab_bar_area {
            if Self::point_in_rect(x, y, tab_bar_area) {
                self.handle_tab_bar_click(x, y, tab_bar_area);
                return Ok(effects);
            }
        }

        // Check input area
        if let Some(input_area) = self.state.input_area {
            if Self::point_in_rect(x, y, input_area) {
                self.handle_input_click(x, y, input_area);
                return Ok(effects);
            }
        }

        // Check status bar
        if let Some(status_bar_area) = self.state.status_bar_area {
            if Self::point_in_rect(x, y, status_bar_area) {
                if let Some(effect) = self.handle_status_bar_click(x, y, status_bar_area) {
                    effects.push(effect);
                }
                return Ok(effects);
            }
        }

        // Check footer
        if let Some(footer_area) = self.state.footer_area {
            if Self::point_in_rect(x, y, footer_area) {
                if let Some(action) = self.handle_footer_click(x, y, footer_area) {
                    effects.extend(self.execute_action(action, terminal, guard).await?);
                }
                return Ok(effects);
            }
        }

        // Check raw events area (debug view)
        if self.state.view_mode == ViewMode::RawEvents {
            if let Some(raw_events_area) = self.state.raw_events_area {
                if Self::point_in_rect(x, y, raw_events_area) {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if let Some(click) =
                            session.raw_events_view.handle_click(x, y, raw_events_area)
                        {
                            match click {
                                RawEventsClick::SessionId => {
                                    if let Some(session_id) = session.raw_events_view.session_id() {
                                        let id_str = session_id.to_string();
                                        effects.push(Effect::CopyToClipboard(id_str.clone()));
                                        self.state.set_timed_footer_message(
                                            format!("Copied session ID: {}", id_str),
                                            Duration::from_secs(3),
                                        );
                                    }
                                    self.state.last_raw_events_click = None;
                                }
                                RawEventsClick::Event(clicked_index) => {
                                    // Check for double-click (same index within 500ms)
                                    let now = Instant::now();
                                    let is_double_click = if let Some((last_time, last_index)) =
                                        self.state.last_raw_events_click
                                    {
                                        last_index == clicked_index
                                            && now.duration_since(last_time)
                                                < Duration::from_millis(500)
                                    } else {
                                        false
                                    };

                                    if is_double_click {
                                        // Double-click: toggle detail panel
                                        session.raw_events_view.toggle_detail();
                                        self.state.last_raw_events_click = None;
                                    } else {
                                        // Single click: just select (already done in handle_click)
                                        self.state.last_raw_events_click =
                                            Some((now, clicked_index));
                                    }
                                }
                            }
                        }
                    }
                    return Ok(effects);
                }
            }
        }

        // Click in chat area - selection handled earlier in the mouse pipeline.
        // Clicking in chat area while in sidebar mode returns to normal.
        if self.state.input_mode == InputMode::SidebarNavigation {
            self.state.input_mode = InputMode::Normal;
            self.state.sidebar_state.set_focused(false);
        }

        Ok(effects)
    }

    /// Check if a point is within a rectangle
    fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }

    /// Handle click in sidebar area
    fn handle_sidebar_click(&mut self, x: u16, y: u16, sidebar_area: Rect) -> Option<Effect> {
        // Use centralized constant for header height (same as hover hit-testing)
        let tree_start_y = sidebar_area.y.saturating_add(SIDEBAR_HEADER_ROWS);
        if y < tree_start_y {
            return None; // Clicked on title or separator
        }

        // Check if clicking on "Add Project" button (when sidebar is empty)
        if let Some(button_area) = self.state.sidebar_state.add_project_button_area {
            if Self::point_in_rect(x, y, button_area) {
                // Trigger new project dialog (same logic as Action::NewProject)
                self.open_project_picker_or_base_dir();
                return None;
            }
        }

        // Always focus sidebar when clicking on it
        self.state.sidebar_state.set_focused(true);
        self.state.input_mode = InputMode::SidebarNavigation;

        let visual_row = (y - tree_start_y) as usize;
        let scroll_offset = self.state.sidebar_state.tree_state.offset;
        let clicked_index = self
            .state
            .sidebar_data
            .index_from_visual_row(visual_row, scroll_offset)?;

        // Detect double-click (same index within 500ms)
        let now = Instant::now();
        let is_double_click = if let Some((last_time, last_index)) = self.state.last_sidebar_click {
            last_index == clicked_index
                && now.duration_since(last_time) < Duration::from_millis(500)
        } else {
            false
        };

        // Update last click tracking
        self.state.last_sidebar_click = Some((now, clicked_index));

        // Get the node at this index
        if let Some(node) = self.state.sidebar_data.get_at(clicked_index) {
            use crate::ui::components::{ActionType, NodeType};

            // Update selection
            self.state.sidebar_state.tree_state.selected = clicked_index;

            // Handle based on node type
            match node.node_type {
                NodeType::Repository => {
                    // Toggle expand/collapse
                    self.state.sidebar_data.toggle_at(clicked_index);
                }
                NodeType::Workspace => {
                    // Single click: open workspace but keep sidebar open
                    // Double click: open workspace and close sidebar
                    self.open_workspace_with_options(node.id, is_double_click);
                }
                NodeType::Action(ActionType::NewWorkspace) => {
                    // Create new workspace
                    if let Some(parent_id) = node.parent_id {
                        return self.start_workspace_creation(parent_id);
                    }
                }
            }
        }

        None
    }

    fn build_tab_bar(&self, focused: bool) -> TabBar {
        let sessions = self.state.tab_manager.sessions();
        let mut pr_numbers = Vec::with_capacity(sessions.len());
        let mut processing_flags = Vec::with_capacity(sessions.len());
        let mut attention_flags = Vec::with_capacity(sessions.len());
        let mut awaiting_response_flags = Vec::with_capacity(sessions.len());
        for session in sessions {
            pr_numbers.push(session.pr_number);
            // Don't show processing spinner if awaiting response (inline prompt active)
            let has_inline_prompt = session.inline_prompt.is_some();
            processing_flags.push(session.is_processing && !has_inline_prompt);
            attention_flags.push(session.needs_attention);
            awaiting_response_flags.push(has_inline_prompt);
        }

        TabBar::new(
            self.state.tab_manager.tab_names(),
            self.state.tab_manager.active_index(),
        )
        .focused(focused)
        .with_tab_states(
            pr_numbers,
            processing_flags,
            attention_flags,
            awaiting_response_flags,
        )
        .with_spinner_frame(self.state.spinner_frame)
        .with_scroll_offset(self.state.tab_bar_scroll)
    }

    fn ensure_tab_bar_scroll(&mut self, area_width: u16, focused: bool) {
        if self.state.tab_manager.is_empty() {
            self.state.tab_bar_scroll = 0;
            self.state.tab_bar_last_active = None;
            return;
        }

        let tab_bar = self.build_tab_bar(focused);
        let max_scroll = tab_bar.max_scroll(area_width);
        if self.state.tab_bar_scroll > max_scroll {
            self.state.tab_bar_scroll = max_scroll;
        }

        let active = self.state.tab_manager.active_index();
        if self.state.tab_bar_last_active != Some(active) {
            self.state.tab_bar_scroll = tab_bar.adjust_scroll_to_active(area_width).min(max_scroll);
            self.state.tab_bar_last_active = Some(active);
        }
    }

    fn scroll_tab_bar(&mut self, area_width: u16, focused: bool, scroll_left: bool) -> bool {
        let tab_bar = self.build_tab_bar(focused);
        let new_offset = if scroll_left {
            tab_bar.scroll_left(area_width)
        } else {
            tab_bar.scroll_right(area_width)
        };

        if new_offset != self.state.tab_bar_scroll {
            self.state.tab_bar_scroll = new_offset;
            return true;
        }

        false
    }

    fn handle_tab_bar_wheel(&mut self, x: u16, y: u16, scroll_left: bool) -> bool {
        let Some(tab_bar_area) = self.state.tab_bar_area else {
            return false;
        };
        if !Self::point_in_rect(x, y, tab_bar_area) {
            return false;
        }

        let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
        self.scroll_tab_bar(tab_bar_area.width, tabs_focused, scroll_left);
        true
    }

    /// Handle click in tab bar area
    fn handle_tab_bar_click(&mut self, x: u16, _y: u16, tab_bar_area: Rect) {
        if self.state.input_mode == InputMode::SidebarNavigation {
            self.state.input_mode = InputMode::Normal;
            self.state.sidebar_state.set_focused(false);
        }

        let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
        let tab_bar = self.build_tab_bar(tabs_focused);

        match tab_bar.hit_test(tab_bar_area, x) {
            TabBarHitTarget::Tab(index) => {
                self.state.tab_manager.switch_to(index);
                self.sync_input_mode_for_active_tab();
                self.ensure_tab_bar_scroll(tab_bar_area.width, tabs_focused);
                self.sync_sidebar_to_active_tab();
                self.sync_footer_spinner();
            }
            TabBarHitTarget::ScrollLeft => {
                self.scroll_tab_bar(tab_bar_area.width, tabs_focused, true);
            }
            TabBarHitTarget::ScrollRight => {
                self.scroll_tab_bar(tab_bar_area.width, tabs_focused, false);
            }
            TabBarHitTarget::None => {
                if self.state.tab_manager.can_add_tab() {
                    self.state.close_overlays();
                    let default_provider = self
                        .preferred_provider_for_new_sessions()
                        .unwrap_or(self.config().default_agent);
                    self.state
                        .agent_selector_state
                        .show_with_default(default_provider);
                    self.state.input_mode = InputMode::SelectingAgent;
                }
            }
        }
    }

    /// Handle click in input area
    fn handle_input_click(&mut self, x: u16, y: u16, input_area: Rect) {
        // Switch to normal mode if we were in sidebar navigation
        if self.state.input_mode == InputMode::SidebarNavigation {
            self.state.input_mode = InputMode::Normal;
            self.state.sidebar_state.set_focused(false);
        }

        // Position cursor based on click
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.input_box.set_cursor_from_click(x, y, input_area);
        }
    }

    /// Handle click in status bar area
    fn handle_status_bar_click(
        &mut self,
        x: u16,
        _y: u16,
        status_bar_area: Rect,
    ) -> Option<Effect> {
        // Status bar format (with plan mode): "  Build  ModelName Agent"
        // Status bar format (without plan mode): "  ModelName Agent"
        //
        // Layout with positions:
        // - 2 chars: leading spaces
        // - For plan mode: 5 chars ("Build") or 4 chars ("Plan") + 2 chars separator
        // - Model name (variable length)
        // - 1 char space + Agent name

        let relative_x = x.saturating_sub(status_bar_area.x) as usize;

        // Extract info from session in a limited scope
        let (show_mode, mode_width, model_width, agent_width, model, agent_type, shell_mode) = {
            let session = self.state.tab_manager.active_session()?;

            let show_mode = session.capabilities.supports_plan_mode;
            let mode_width = if show_mode {
                session.agent_mode.display_name().len()
            } else {
                0
            };

            // Calculate model display name
            let shell_mode = session.input_box.is_shell_mode();
            let model_display = if shell_mode {
                "Shell".to_string()
            } else {
                let model_id = session.model.clone().unwrap_or_else(|| {
                    crate::agent::ModelRegistry::default_model(session.agent_type)
                });
                crate::agent::ModelRegistry::find_model(session.agent_type, &model_id)
                    .map(|m| m.display_name.to_string())
                    .unwrap_or(model_id)
            };
            let model_width = model_display.len();

            let agent_display = session.agent_type.display_name();
            let agent_width = agent_display.len();
            let model = session.model.clone();
            let agent_type = session.agent_type;

            (
                show_mode,
                mode_width,
                model_width,
                agent_width,
                model,
                agent_type,
                shell_mode,
            )
        };

        if shell_mode {
            return self.check_pr_badge_click(x, status_bar_area);
        }

        // Calculate positions with 1 char padding on each side
        // Leading spaces: 2 chars
        let leading: usize = 2;

        if show_mode {
            // Mode area: leading + mode_width (with 1 char padding each side)
            let mode_start = leading.saturating_sub(1); // 1 char before
            let mode_end = leading + mode_width + 1; // 1 char after

            // Model/Agent area starts after mode + 2 char separator
            let model_start = leading + mode_width + 2 - 1; // 1 char before model
            let model_end = leading + mode_width + 2 + model_width + 1 + agent_width + 1; // 1 char after agent

            if relative_x >= mode_start && relative_x < mode_end {
                // Click on mode area - toggle mode
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.capabilities.supports_plan_mode {
                        session.agent_mode = session.agent_mode.toggle();
                        session.update_status();
                    }
                }
            } else if relative_x >= model_start && relative_x < model_end && !shell_mode {
                let should_block_model_switch = self
                    .state
                    .tab_manager
                    .active_session()
                    .is_some_and(|session| {
                        session.is_processing
                            || session.tools_in_flight > 0
                            || session.pending_user_message.is_some()
                            || session.inline_prompt.is_some()
                    });
                if should_block_model_switch {
                    self.state.set_timed_footer_message(
                        "Finish the current response before switching models".to_string(),
                        Duration::from_secs(3),
                    );
                } else {
                    // Click on model/agent area - open model selector
                    let mut allowed = self.config().effective_enabled_providers(self.tools());
                    if !allowed.contains(&agent_type) {
                        let tool = Self::required_tool(agent_type);
                        if self.tools().is_available(tool) {
                            allowed.push(agent_type);
                        }
                    }
                    if allowed.is_empty() {
                        self.state.set_timed_footer_message(
                            "No enabled providers available. Use /providers.".to_string(),
                            Duration::from_secs(4),
                        );
                        return self.check_pr_badge_click(x, status_bar_area);
                    }
                    self.state.close_overlays();
                    let defaults = self.model_selector_defaults();
                    self.state
                        .model_selector_state
                        .set_allowed_providers(Some(allowed));
                    self.state.model_selector_state.show(model, defaults);
                    self.state.model_picker_context = ModelPickerContext::SessionSelection;
                    self.state.input_mode = InputMode::SelectingModel;
                }
            }
        } else {
            // No mode area, just model/agent
            let model_start = leading.saturating_sub(1); // 1 char before model
            let model_end = leading + model_width + 1 + agent_width + 1; // 1 char after agent

            if relative_x >= model_start && relative_x < model_end && !shell_mode {
                let should_block_model_switch = self
                    .state
                    .tab_manager
                    .active_session()
                    .is_some_and(|session| {
                        session.is_processing
                            || session.tools_in_flight > 0
                            || session.pending_user_message.is_some()
                            || session.inline_prompt.is_some()
                    });
                if should_block_model_switch {
                    self.state.set_timed_footer_message(
                        "Finish the current response before switching models".to_string(),
                        Duration::from_secs(3),
                    );
                } else {
                    let mut allowed = self.config().effective_enabled_providers(self.tools());
                    if !allowed.contains(&agent_type) {
                        let tool = Self::required_tool(agent_type);
                        if self.tools().is_available(tool) {
                            allowed.push(agent_type);
                        }
                    }
                    if allowed.is_empty() {
                        self.state.set_timed_footer_message(
                            "No enabled providers available. Use /providers.".to_string(),
                            Duration::from_secs(4),
                        );
                        return self.check_pr_badge_click(x, status_bar_area);
                    }
                    self.state.close_overlays();
                    let defaults = self.model_selector_defaults();
                    self.state
                        .model_selector_state
                        .set_allowed_providers(Some(allowed));
                    self.state.model_selector_state.show(model, defaults);
                    self.state.model_picker_context = ModelPickerContext::SessionSelection;
                    self.state.input_mode = InputMode::SelectingModel;
                }
            }
        }

        // Check for PR badge click on the right side
        self.check_pr_badge_click(x, status_bar_area)
    }

    /// Check if click is on the PR badge and return an effect to open PR in browser
    fn check_pr_badge_click(&self, x: u16, status_bar_area: Rect) -> Option<Effect> {
        // Get PR info and calculate right content width from current session
        let session = self.state.tab_manager.active_session()?;

        let working_dir = session.working_dir.clone()?;

        // If no PR, nothing to click
        let num = session.pr_number?;

        // Calculate PR badge width: " PR #N " = 5 + digits + 1
        let pr_badge_width = 5 + num.to_string().len() + 1;

        // Calculate total right content width to find where it starts
        // Format: [PR badge] [· +N -M] [· branch] [  ]
        let mut right_content_width = pr_badge_width;

        // Git stats (if any)
        let stats = session.status_bar.git_diff_stats();
        if stats.has_changes() {
            right_content_width += 3; // " · "
            if stats.additions > 0 {
                right_content_width += 1 + stats.additions.to_string().len(); // "+N"
            }
            if stats.additions > 0 && stats.deletions > 0 {
                right_content_width += 1; // " "
            }
            if stats.deletions > 0 {
                right_content_width += 1 + stats.deletions.to_string().len(); // "-N"
            }
        }

        // Branch name
        if let Some(branch) = session.status_bar.branch_name() {
            right_content_width += 3; // " · "
            right_content_width += branch.len();
        }

        // Trailing padding
        right_content_width += 2;

        // Calculate where right content starts
        let status_width = status_bar_area.width as usize;
        if right_content_width > status_width {
            return None; // Content doesn't fit
        }

        let right_start_x = status_bar_area.x + (status_width - right_content_width) as u16;
        let pr_badge_end_x = right_start_x + pr_badge_width as u16;

        // Check if click is within PR badge
        if x >= right_start_x && x < pr_badge_end_x {
            Some(Effect::OpenPrInBrowser { working_dir })
        } else {
            None
        }
    }

    /// Handle click in model selector dialog
    fn handle_model_selector_click(&mut self, x: u16, y: u16) -> Vec<Effect> {
        let mut effects = Vec::new();
        const DIALOG_WIDTH: u16 = 60;
        const DIALOG_HEIGHT: u16 = 18;

        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen = Rect::new(0, 0, terminal_size.0, terminal_size.1);

        let dialog_width = DIALOG_WIDTH.min(screen.width.saturating_sub(4));
        let dialog_height = DIALOG_HEIGHT.min(screen.height.saturating_sub(2));
        let dialog_x = (screen.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (screen.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        };

        if x < dialog_area.x
            || x >= dialog_area.x + dialog_area.width
            || y < dialog_area.y
            || y >= dialog_area.y + dialog_area.height
        {
            self.state.model_selector_state.hide();
            if self.state.model_picker_context == ModelPickerContext::OnboardingDefaultSelection {
                self.state.pending_new_project_target = None;
            } else if self.state.model_picker_context
                == ModelPickerContext::SettingsDefaultSelection
            {
                self.state.model_picker_context = ModelPickerContext::SessionSelection;
                self.reopen_settings_menu();
                return effects;
            }
            self.state.model_picker_context = ModelPickerContext::SessionSelection;
            self.state.input_mode = InputMode::Normal;
            return effects;
        }

        let inner = dialog_content_area(dialog_area);

        if inner.height < 4 {
            return effects;
        }

        // Layout: search, separator, list, instructions
        let list_y = inner.y + 2;
        let list_height = inner.height.saturating_sub(3);

        if y >= list_y && y < list_y + list_height {
            let clicked_row = (y - list_y) as usize;
            if self.state.model_selector_state.select_at_row(clicked_row) {
                if let Some(model) = self.state.model_selector_state.selected_model().cloned() {
                    let required_tool = Self::required_tool(model.agent_type);
                    if !self.tools().is_available(required_tool) {
                        self.show_missing_tool(
                            required_tool,
                            format!(
                                "{} is required to use this model.",
                                required_tool.display_name()
                            ),
                        );
                        return effects;
                    }

                    if self.state.model_picker_context
                        == ModelPickerContext::OnboardingDefaultSelection
                    {
                        if self.persist_default_model_selection(&model) {
                            self.state.model_selector_state.hide();
                            self.state.model_picker_context = ModelPickerContext::SessionSelection;
                            self.continue_new_project_flow();
                        }
                        return effects;
                    }

                    if self.state.model_picker_context
                        == ModelPickerContext::SettingsDefaultSelection
                    {
                        if self.persist_default_model_selection(&model) {
                            self.state.model_selector_state.hide();
                            self.state.model_picker_context = ModelPickerContext::SessionSelection;
                            self.reopen_settings_menu();
                        }
                        return effects;
                    }

                    if self.state.model_picker_context == ModelPickerContext::HandoffSelection {
                        self.state.model_selector_state.hide();
                        self.state.model_picker_context = ModelPickerContext::SessionSelection;
                        self.state.input_mode = InputMode::Normal;
                        match self.execute_handoff_session(model.agent_type, model.id.clone()) {
                            Ok(new_effects) => effects.extend(new_effects),
                            Err(err) => self.show_error("Handoff Failed", &err.to_string()),
                        }
                        return effects;
                    }

                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if Self::reject_cross_agent_switch(session, model.agent_type) {
                            return effects;
                        }
                        let agent_changed =
                            session.set_agent_and_model(model.agent_type, Some(model.id.clone()));

                        let msg = if agent_changed {
                            format!(
                                "Switched to {} with model: {}",
                                model.agent_type, model.display_name
                            )
                        } else {
                            format!("Model changed to: {}", model.display_name)
                        };
                        let display = MessageDisplay::System { content: msg };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
                self.state.model_selector_state.hide();
                self.state.model_picker_context = ModelPickerContext::SessionSelection;
                self.state.input_mode = InputMode::Normal;
            }
        }

        effects
    }

    fn handle_reasoning_selector_click(&mut self, x: u16, y: u16) {
        const DIALOG_WIDTH: u16 = 58;
        const DIALOG_HEIGHT: u16 = 14;

        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen = Rect::new(0, 0, terminal_size.0, terminal_size.1);

        let dialog_width = DIALOG_WIDTH.min(screen.width.saturating_sub(4));
        let dialog_height = DIALOG_HEIGHT.min(screen.height.saturating_sub(2));
        let dialog_x = (screen.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (screen.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        };

        if !Self::point_in_rect(x, y, dialog_area) {
            self.state.reasoning_selector_state.hide();
            self.state.input_mode = InputMode::Normal;
            return;
        }

        let inner = dialog_content_area(dialog_area);
        if inner.height < 4 {
            return;
        }

        // Layout: search, separator, list, hint
        let list_y = inner.y + 2;
        let list_height = inner.height.saturating_sub(3);
        self.state
            .reasoning_selector_state
            .set_max_visible(list_height.saturating_sub(1) as usize);

        if y >= list_y && y < list_y + list_height {
            let clicked_row = (y - list_y) as usize;
            if self
                .state
                .reasoning_selector_state
                .select_at_row(clicked_row)
            {
                if let Some(option) = self.state.reasoning_selector_state.selected_option() {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if Self::session_started(session) {
                            let display = MessageDisplay::Error {
                                content: "Changing reasoning effort after a session has started is not supported. Start a new session/tab."
                                    .to_string(),
                            };
                            session.chat_view.push(display.to_chat_message());
                            return;
                        }
                        session.set_reasoning_effort(option.effort);
                        let msg = match option.effort {
                            Some(effort) => {
                                format!("Reasoning effort set to: {}", effort.display_name())
                            }
                            None => "Reasoning effort set to: Auto".to_string(),
                        };
                        let display = MessageDisplay::System { content: msg };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
                self.state.reasoning_selector_state.hide();
                self.state.input_mode = InputMode::Normal;
            }
        }
    }

    fn handle_provider_selector_click(&mut self, x: u16, y: u16) {
        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen = Rect::new(0, 0, terminal_size.0, terminal_size.1);
        let dialog_area = ProviderSelector::dialog_area(screen);

        if !Self::point_in_rect(x, y, dialog_area) {
            self.state.provider_selector_state.hide();
            self.state.pending_new_project_target = None;
            if !self.return_to_settings_menu_if_needed() {
                self.state.input_mode = InputMode::Normal;
            }
            return;
        }

        let list_area = ProviderSelector::list_area(screen);
        if y >= list_area.y && y < list_area.y + list_area.height {
            let clicked_row = (y - list_area.y) as usize;
            if self
                .state
                .provider_selector_state
                .select_at_row(clicked_row)
            {
                self.state.provider_selector_state.toggle_selected();
            }
        }
    }

    fn handle_settings_menu_click(&mut self, x: u16, y: u16) {
        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen = Rect::new(0, 0, terminal_size.0, terminal_size.1);
        let dialog_area = SettingsMenu::dialog_area(screen);

        if !Self::point_in_rect(x, y, dialog_area) {
            self.state.settings_menu_state.hide();
            self.state.input_mode = InputMode::Normal;
            return;
        }

        let list_area = SettingsMenu::list_area(screen);
        if y >= list_area.y && y < list_area.y + list_area.height {
            let clicked_row = (y - list_area.y) as usize;
            if self.state.settings_menu_state.select_at_row(clicked_row) {
                self.open_selected_setting();
            }
        }
    }

    fn handle_workspace_defaults_click(&mut self, x: u16, y: u16) {
        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen = Rect::new(0, 0, terminal_size.0, terminal_size.1);
        let dialog_area = WorkspaceDefaultsDialog::dialog_area(screen);

        if !Self::point_in_rect(x, y, dialog_area) {
            self.state.workspace_defaults_dialog_state.hide();
            if !self.return_to_settings_menu_if_needed() {
                self.state.input_mode = InputMode::Normal;
            }
            return;
        }

        let list_area = WorkspaceDefaultsDialog::list_area(screen);
        if y >= list_area.y && y < list_area.y + list_area.height {
            let clicked_row = (y - list_area.y) as usize;
            self.state
                .workspace_defaults_dialog_state
                .select_at_row(clicked_row);
        }
    }

    /// Handle click in project picker dialog
    fn handle_project_picker_click(&mut self, x: u16, y: u16) {
        // Calculate dialog position based on terminal size
        // The dialog is 60 wide and centered, height is 7 + list_height
        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen_width = terminal_size.0;
        let screen_height = terminal_size.1;

        let dialog_width: u16 = 60;
        let list_height = self.state.project_picker_state.list.visible_len() as u16;
        let dialog_height = 6 + list_height;

        // Calculate dialog position (centered)
        let dialog_x = screen_width.saturating_sub(dialog_width) / 2;
        let dialog_y = screen_height.saturating_sub(dialog_height) / 2;

        let dialog_area = Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        };

        let inner = dialog_content_area(dialog_area);

        // List area starts at row 2 within inner area (after search_label, separator)
        // Layout: [0] search label, [1] separator, [2..] list
        let list_y = inner.y + 2;
        let list_height_actual = inner.height.saturating_sub(3);

        // Check if click is in the list area
        if x >= inner.x
            && x < inner.x + inner.width
            && y >= list_y
            && y < list_y + list_height_actual
        {
            // Calculate which row was clicked
            let clicked_row = (y - list_y) as usize;

            // Select the item and trigger double-click detection
            if self.state.project_picker_state.select_at_row(clicked_row) {
                // Check for double-click (would need timing - for now just select)
                // Could add double-click to open in future
            }
        }
    }

    /// Handle click in footer area
    /// Returns an action to execute if a valid hint was clicked
    fn handle_footer_click(&mut self, x: u16, _y: u16, footer_area: Rect) -> Option<Action> {
        // Use the same hints as GlobalFooter to stay in sync
        // Sidebar focus takes precedence over file viewer / view_mode
        let hints: Vec<(&str, &str)> = if self.state.input_mode == InputMode::SidebarNavigation {
            GlobalFooter::sidebar_hints()
        } else if self.state.tab_manager.active_is_file() {
            GlobalFooter::file_viewer_hints()
        } else {
            match self.state.view_mode {
                ViewMode::Chat => GlobalFooter::chat_hints(),
                ViewMode::RawEvents => GlobalFooter::raw_events_hints(),
            }
        };

        // Calculate click position relative to footer
        let relative_x = x.saturating_sub(footer_area.x) as usize;

        // Match the layout from GlobalFooter::render:
        // " [key] action   [key] action ..."
        // Leading space = 1, key has " key " (len+2), action has " action" (len+1), spacing = 3
        let mut current_x: usize = 1; // Leading space

        for (key, action_name) in hints {
            // Format: " key " (key.len + 2) + " action" (action_name.len + 1) + spacing (3)
            let key_width = key.len() + 2;
            let action_width = action_name.len() + 1;
            let hint_width = key_width + action_width + 3;

            if relative_x >= current_x && relative_x < current_x + hint_width {
                // Clicked on this hint - look up action from keybinding config
                return self.lookup_footer_action(key);
            }
            current_x += hint_width;
        }
        None
    }

    /// Look up the action for a footer key hint using the keybinding config
    fn lookup_footer_action(&self, key: &str) -> Option<Action> {
        // Handle compound keys like "j/k" by taking the first one
        let primary_key = key.split('/').next().unwrap_or(key);

        // Special case for "CR" which should be "<CR>"
        let key_notation = if primary_key == "CR" {
            "<CR>".to_string()
        } else {
            primary_key.to_string()
        };

        // Parse the key notation
        let key_combo = parse_key_notation(&key_notation).ok()?;

        // Determine context from current mode and active tab type.
        let context = self.key_context_for_active_tab();

        // Look up action in keybinding config
        self.config()
            .keybindings
            .get_action(&key_combo, context)
            .cloned()
    }

    fn key_context_for_active_tab(&self) -> KeyContext {
        match self.state.input_mode {
            InputMode::Normal | InputMode::Scrolling | InputMode::FileViewer => {
                if self.state.tab_manager.active_is_file() {
                    KeyContext::FileViewer
                } else {
                    KeyContext::from_input_mode(self.state.input_mode, self.state.view_mode)
                }
            }
            _ => KeyContext::from_input_mode(self.state.input_mode, self.state.view_mode),
        }
    }

    async fn handle_app_event(&mut self, event: AppEvent) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();
        match event {
            AppEvent::Agent { session_id, event } => {
                self.handle_agent_event(session_id, event).await?;
            }
            AppEvent::Quit => {
                self.state.should_quit = true;
                effects.push(Effect::SaveSessionState);
            }
            AppEvent::Error(msg) => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let display = MessageDisplay::Error { content: msg };
                    session.chat_view.push(display.to_chat_message());
                    session.stop_processing();
                    self.state.stop_footer_spinner();
                }
            }
            AppEvent::PrPreflightCompleted {
                tab_index,
                working_dir,
                result,
            } => {
                effects.extend(self.handle_pr_preflight_result(tab_index, working_dir, result));
            }
            AppEvent::OpenPrCompleted { result: Err(err) } => {
                self.show_error(
                    "Failed to Open PR",
                    &format!("Could not open PR in browser: {}", err),
                );
            }
            AppEvent::OpenPrCompleted { result: Ok(_) } => {}
            AppEvent::DebugDumped { result } => match result {
                Ok(path) => {
                    self.show_error_with_details(
                        "Debug Export Complete",
                        "Session debug info has been exported.",
                        &format!("File saved to:\n{}", path),
                    );
                }
                Err(err) => {
                    self.show_error("Export Failed", &err);
                }
            },
            AppEvent::WorkspaceCreated { repo_id, result } => {
                self.clear_repo_action_busy(repo_id);
                match result {
                    Ok(created) => {
                        self.refresh_sidebar_data();
                        self.state.sidebar_data.expand_repo(created.repo_id);
                        if let Some(index) = self.find_workspace_index(created.workspace_id) {
                            self.state.sidebar_state.tree_state.selected = index;
                        }
                        // Open workspace, close sidebar, and focus prompt box
                        self.open_workspace_with_options(created.workspace_id, true);
                    }
                    Err(err) => {
                        self.show_error("Workspace Creation Failed", &err);
                    }
                }
            }
            AppEvent::ForkWorkspaceCreated {
                parent_workspace_id,
                result,
            } => {
                self.clear_workspace_busy(parent_workspace_id);
                match result {
                    Ok(created) => {
                        self.refresh_sidebar_data();
                        self.state.sidebar_data.expand_repo(created.repo_id);
                        if let Some(index) = self.find_workspace_index(created.workspace_id) {
                            self.state.sidebar_state.tree_state.selected = index;
                        }
                        match self.finish_fork_session(created.workspace_id) {
                            Ok(mut fork_effects) => {
                                effects.append(&mut fork_effects);
                            }
                            Err(err) => {
                                // Clean up fork seed
                                if let Some(pending) = self.state.pending_fork_request.take() {
                                    if let Some(seed_id) = pending.fork_seed_id {
                                        if let Some(dao) = self.fork_seed_dao() {
                                            if let Err(e) = dao.delete(seed_id) {
                                                tracing::debug!(
                                                    error = %e,
                                                    seed_id = %seed_id,
                                                    "Failed to delete fork seed after fork error"
                                                );
                                            }
                                        }
                                    }
                                }
                                // Attempt to clean up the created workspace
                                let cleanup_msg = self
                                    .cleanup_fork_workspace(created.workspace_id, created.repo_id);
                                let error_msg = match cleanup_msg {
                                    Some(cleanup_err) => format!(
                                        "{}\n\nWorkspace cleanup failed: {}. \
                                         You may need to manually remove it from the sidebar.",
                                        err, cleanup_err
                                    ),
                                    None => err.to_string(),
                                };
                                self.show_error("Fork Failed", &error_msg);
                            }
                        }
                    }
                    Err(err) => {
                        if let Some(pending) = self.state.pending_fork_request.take() {
                            if let Some(seed_id) = pending.fork_seed_id {
                                if let Some(dao) = self.fork_seed_dao() {
                                    if let Err(e) = dao.delete(seed_id) {
                                        tracing::debug!(
                                            error = %e,
                                            seed_id = %seed_id,
                                            "Failed to delete fork seed after fork error"
                                        );
                                    }
                                }
                            }
                        }
                        self.show_error("Fork Failed", &err);
                    }
                }
            }
            AppEvent::ArchiveWorkspaceDialogPreflightCompleted {
                workspace_id,
                result,
            } => {
                let is_active_preflight = self.state.confirmation_dialog_state.loading
                    && matches!(
                        self.state.confirmation_dialog_state.context,
                        Some(ConfirmationContext::ArchiveWorkspacePreflightInProgress {
                            workspace_id: id
                        }) if id == workspace_id
                    );
                if !is_active_preflight {
                    return Ok(effects);
                }

                match result {
                    Ok(preflight) => {
                        let confirmation_type = match (preflight.has_dirty, preflight.has_unmerged)
                        {
                            (true, true) => ConfirmationType::Danger,
                            (true, false) | (false, true) => ConfirmationType::Warning,
                            (false, false) => {
                                if preflight.warnings.is_empty() {
                                    ConfirmationType::Info
                                } else {
                                    ConfirmationType::Warning
                                }
                            }
                        };

                        self.state.confirmation_dialog_state.show(
                            format!("Archive \"{}\"?", preflight.workspace_name),
                            preflight.message,
                            preflight.warnings,
                            confirmation_type,
                            "Archive",
                            Some(ConfirmationContext::ArchiveWorkspace(workspace_id)),
                        );
                        self.state.input_mode = InputMode::Confirming;
                    }
                    Err(err) => {
                        self.state.confirmation_dialog_state.hide();
                        self.show_error("Archive Failed", &err);
                    }
                }
            }
            AppEvent::RemoveProjectDialogPreflightCompleted { repo_id, result } => {
                let is_active_preflight = self.state.confirmation_dialog_state.loading
                    && matches!(
                        self.state.confirmation_dialog_state.context,
                        Some(ConfirmationContext::RemoveProjectPreflightInProgress {
                            repo_id: id
                        }) if id == repo_id
                    );
                if !is_active_preflight {
                    return Ok(effects);
                }

                match result {
                    Ok(preflight) => {
                        let confirmation_type = match (preflight.has_dirty, preflight.has_unmerged)
                        {
                            (true, true) => ConfirmationType::Danger,
                            (true, false) | (false, true) => ConfirmationType::Warning,
                            (false, false) => {
                                if preflight.workspace_count > 0 {
                                    ConfirmationType::Warning
                                } else {
                                    ConfirmationType::Info
                                }
                            }
                        };

                        self.state.confirmation_dialog_state.show(
                            format!("Remove \"{}\"?", preflight.repo_name),
                            "This will archive all workspaces and remove the project.",
                            preflight.warnings,
                            confirmation_type,
                            "Remove",
                            Some(ConfirmationContext::RemoveProject(repo_id)),
                        );
                        self.state.input_mode = InputMode::Confirming;
                    }
                    Err(err) => {
                        self.state.confirmation_dialog_state.hide();
                        self.show_error("Project Removal Failed", &err);
                    }
                }
            }
            AppEvent::ForkSessionDialogPreflightCompleted {
                parent_workspace_id,
                result,
            } => {
                let is_active_preflight = self.state.confirmation_dialog_state.loading
                    && matches!(
                        self.state.confirmation_dialog_state.context,
                        Some(ConfirmationContext::ForkSessionPreflightInProgress {
                            parent_workspace_id: id
                        }) if id == parent_workspace_id
                    );
                if !is_active_preflight {
                    return Ok(effects);
                }

                match result {
                    Ok(preflight) => {
                        let Some(pending) = self.state.pending_fork_request.clone() else {
                            self.state.confirmation_dialog_state.hide();
                            self.state.input_mode = InputMode::Normal;
                            return Ok(effects);
                        };

                        if pending.parent_workspace_id != parent_workspace_id {
                            return Ok(effects);
                        }

                        let usage_pct = if pending.context_window > 0 {
                            (pending.token_estimate as f64 / pending.context_window as f64) * 100.0
                        } else {
                            0.0
                        };

                        let mut warnings = Vec::new();
                        let has_dirty = if let Some(desc) = preflight.dirty_warning {
                            warnings.push(desc);
                            warnings.push("Commit before forking to preserve changes.".to_string());
                            true
                        } else {
                            false
                        };

                        if usage_pct >= 100.0 {
                            warnings.push(format!(
                                "Seed exceeds context window ({} / {} tokens, ~{:.0}%).",
                                pending.token_estimate, pending.context_window, usage_pct
                            ));
                        } else if usage_pct >= 80.0 {
                            warnings.push(format!(
                                "Seed uses ~{:.0}% of context window ({} / {}).",
                                usage_pct, pending.token_estimate, pending.context_window
                            ));
                        }

                        let confirmation_type = if usage_pct >= 100.0 {
                            ConfirmationType::Danger
                        } else if has_dirty || usage_pct >= 80.0 {
                            ConfirmationType::Warning
                        } else {
                            ConfirmationType::Info
                        };

                        let message = format!(
                            "Fork this session into a new workspace based on branch \"{}\".\nSeed size: {} / {} tokens (~{:.0}%).",
                            preflight.base_branch,
                            pending.token_estimate,
                            pending.context_window,
                            usage_pct
                        );

                        self.state.confirmation_dialog_state.show(
                            "Fork session?",
                            message,
                            warnings,
                            confirmation_type,
                            "Fork",
                            Some(ConfirmationContext::ForkSession {
                                parent_workspace_id,
                                base_branch: preflight.base_branch,
                            }),
                        );
                        self.state.input_mode = InputMode::Confirming;
                    }
                    Err(err) => {
                        self.state.pending_fork_request = None;
                        self.state.confirmation_dialog_state.hide();
                        self.show_error("Fork Failed", &err);
                    }
                }
            }
            AppEvent::ArchiveWorkspacePreflightCompleted {
                workspace_id,
                result,
            } => match result {
                Ok(preflight) => {
                    if preflight.should_prompt_remote_delete {
                        self.clear_workspace_busy(workspace_id);
                        self.hide_archive_progress_dialog(workspace_id);

                        let Some(workspace_dao) = self.workspace_dao() else {
                            self.show_error("Archive Failed", "Workspace database unavailable.");
                            return Ok(effects);
                        };

                        match workspace_dao.get_by_id(workspace_id) {
                            Ok(Some(workspace)) => {
                                self.prompt_archive_remote_delete(&workspace);
                            }
                            Ok(None) => {
                                self.show_error("Archive Failed", "Workspace not found.");
                            }
                            Err(err) => {
                                self.show_error(
                                    "Archive Failed",
                                    &format!("Failed to load workspace: {}", err),
                                );
                            }
                        }
                    } else {
                        effects.push(Effect::ArchiveWorkspace {
                            workspace_id,
                            delete_remote: false,
                        });
                    }
                }
                Err(err) => {
                    self.clear_workspace_busy(workspace_id);
                    self.hide_archive_progress_dialog(workspace_id);
                    self.show_error("Archive Failed", &err);
                }
            },
            AppEvent::WorkspaceArchived {
                workspace_id,
                result,
            } => {
                self.clear_workspace_busy(workspace_id);
                self.hide_archive_progress_dialog(workspace_id);
                match result {
                    Ok(archived) => {
                        if !archived.warnings.is_empty() {
                            self.show_error_with_details(
                                "Archive Warning",
                                "Workspace archived with warnings",
                                &archived.warnings.join("\n"),
                            );
                        }

                        self.close_tabs_for_workspace(archived.workspace_id);

                        let current_selection = self.state.sidebar_state.tree_state.selected;
                        self.refresh_sidebar_data();

                        let visible_count = self.state.sidebar_data.visible_nodes().len();
                        if visible_count > 0 {
                            let new_selection = if current_selection > 0 {
                                current_selection - 1
                            } else {
                                0
                            };
                            self.state.sidebar_state.tree_state.selected =
                                new_selection.min(visible_count - 1);
                        } else {
                            self.state.sidebar_state.tree_state.selected = 0;
                        }

                        if archived.warnings.is_empty() {
                            self.state.set_timed_footer_message(
                                "Workspace archived".to_string(),
                                Duration::from_secs(3),
                            );
                        }
                    }
                    Err(err) => {
                        self.show_error("Archive Failed", &err);
                    }
                }
            }
            AppEvent::ProjectsDiscovered { base_dir, result } => {
                if !self.state.project_picker_state.visible
                    || self.state.project_picker_state.base_dir != base_dir
                {
                    return Ok(effects);
                }

                match result {
                    Ok(entries) => {
                        let projects: Vec<ProjectEntry> = entries
                            .into_iter()
                            .map(|entry| ProjectEntry {
                                name: entry.name,
                                path: entry.path,
                            })
                            .collect();
                        self.state.project_picker_state.load_projects(projects);
                    }
                    Err(err) => {
                        self.state.project_picker_state.set_error(err);
                    }
                }
            }
            AppEvent::ProjectRemoved { result } => {
                self.clear_repo_busy(result.repo_id);
                self.clear_repo_action_busy(result.repo_id);
                for workspace_id in &result.workspace_ids {
                    self.close_tabs_for_workspace(*workspace_id);
                }

                let has_errors = !result.errors.is_empty();
                if has_errors {
                    tracing::warn!(
                        repo_id = %result.repo_id,
                        error_count = result.errors.len(),
                        errors = ?result.errors,
                        "Project removal completed with errors"
                    );
                    self.show_error_with_details(
                        "Project Removal Errors",
                        "Some operations failed during project removal",
                        &result.errors.join("\n"),
                    );
                }

                let current_selection = self.state.sidebar_state.tree_state.selected;
                self.refresh_sidebar_data();

                let visible_count = self.state.sidebar_data.visible_nodes().len();
                if visible_count > 0 {
                    let new_selection = if current_selection > 0 {
                        current_selection - 1
                    } else {
                        0
                    };
                    self.state.sidebar_state.tree_state.selected =
                        new_selection.min(visible_count - 1);
                    if !has_errors {
                        self.state.input_mode = InputMode::SidebarNavigation;
                    }
                } else {
                    self.state.sidebar_state.tree_state.selected = 0;
                    self.state.show_first_time_splash = true;
                    if !has_errors {
                        self.state.input_mode = InputMode::Normal;
                    }
                }
            }
            AppEvent::AgentStarted {
                session_id,
                pid,
                input_tx,
            } => {
                // Store the PID for interrupt support
                let Some(tab_index) = self.state.tab_manager.session_index_by_id(session_id) else {
                    tracing::debug!(
                        %session_id,
                        "AgentStarted for unknown session; ignoring"
                    );
                    return Ok(effects);
                };
                if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                    session.agent_pid = Some(pid);
                    session.agent_pid_start_time = Self::pid_start_time(pid);
                    session.agent_input_tx = input_tx;
                    tracing::debug!(
                        session_id = %session_id,
                        "Agent started with PID {} for tab {}",
                        pid,
                        tab_index
                    );

                    // Display fork success message once when agent has started successfully
                    if session.fork_seed_id.is_some() && !session.fork_welcome_shown {
                        session.fork_welcome_shown = true;
                        let display = MessageDisplay::System {
                            content:
                                "Fork created; context injected. Waiting for your next prompt."
                                    .to_string(),
                        };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
            }
            AppEvent::AgentStartFailed { session_id, error } => {
                let Some(tab_index) = self.state.tab_manager.session_index_by_id(session_id) else {
                    tracing::debug!(
                        %session_id,
                        "AgentStartFailed for unknown session; ignoring"
                    );
                    return Ok(effects);
                };
                let is_active_tab = self.state.tab_manager.active_index() == tab_index;
                if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                    session.stop_processing();
                    session.chat_view.finalize_streaming();
                    session.tools_in_flight = 0;
                    session.set_processing_state(ProcessingState::Thinking);
                    session.agent_input_tx = None;
                    let display = MessageDisplay::Error { content: error };
                    session.chat_view.push(display.to_chat_message());
                }
                if is_active_tab {
                    self.state.stop_footer_spinner();
                }
            }
            AppEvent::AgentTerminationResult {
                session_id,
                pid,
                context,
                success,
            } => {
                if !success {
                    tracing::warn!(
                        pid,
                        context = %context,
                        "Agent termination did not complete"
                    );
                    if session_id
                        .and_then(|id| self.state.tab_manager.session_index_by_id(id))
                        .is_some()
                    {
                        self.state.set_timed_footer_message(
                            "Failed to terminate agent; process may still be running".to_string(),
                            Duration::from_secs(5),
                        );
                    }
                }
            }
            AppEvent::AgentStreamEnded { session_id } => {
                let Some(tab_index) = self.state.tab_manager.session_index_by_id(session_id) else {
                    tracing::debug!(
                        %session_id,
                        "AgentStreamEnded for unknown session; ignoring"
                    );
                    return Ok(effects);
                };
                // Agent event stream ended (process exited) - ensure processing is stopped
                let is_active_tab = self.state.tab_manager.active_index() == tab_index;
                let was_processing =
                    if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                        // Clear PID since process has exited
                        session.agent_pid = None;
                        session.agent_pid_start_time = None;
                        session.agent_input_tx = None;
                        // Safety: don't let fork-seed suppression leak into future runs
                        session.suppress_next_assistant_reply = false;
                        session.suppress_next_turn_summary = false;
                        let was_processing = if session.is_processing {
                            session.stop_processing();
                            true
                        } else {
                            false
                        };

                        Self::flush_pending_agent_output(session);
                        session.tools_in_flight = 0;
                        was_processing
                    } else {
                        false
                    };
                // Only stop footer spinner if this was the active tab
                if was_processing && is_active_tab {
                    self.state.stop_footer_spinner();
                }

                match self.drain_queue_for_tab(tab_index) {
                    Ok(mut queued_effects) => effects.append(&mut queued_effects),
                    Err(err) => {
                        tracing::warn!(error = %err, "Failed to drain queued messages");
                    }
                }
            }
            AppEvent::SessionsCacheLoaded { sessions } => {
                // Load cached sessions immediately - fast path
                self.state.session_import_state.load_sessions(sessions);
                // Keep loading=true since background refresh continues
            }
            AppEvent::SessionUpdated { session } => {
                // Add or update single session during background refresh
                self.state.session_import_state.upsert_session(session);
            }
            AppEvent::SessionRemoved { file_path } => {
                // Remove session by file path (file no longer exists)
                self.state
                    .session_import_state
                    .remove_session_by_path(&file_path);
            }
            AppEvent::SessionDiscoveryComplete => {
                // Background refresh done - stop spinner
                self.state.session_import_state.set_loading(false);
            }
            AppEvent::GitTracker(update) => {
                self.handle_git_tracker_update(update);
            }
            AppEvent::ShellCommandCompleted {
                session_id,
                message_index,
                result,
            } => {
                let Some(session) = self.state.tab_manager.session_by_id_mut(session_id) else {
                    tracing::debug!(
                        %session_id,
                        "ShellCommandCompleted for unknown session; ignoring"
                    );
                    return Ok(effects);
                };

                let (output, exit_code) = match result {
                    Ok(output) => (output.output, output.exit_code),
                    Err(err) => (format!("Error: {}", err), Some(1)),
                };

                if !session
                    .chat_view
                    .update_tool_at(message_index, output, exit_code)
                {
                    tracing::warn!(
                        session_id = %session_id,
                        message_index,
                        "ShellCommandCompleted: no matching tool message found to update"
                    );
                }
            }
            AppEvent::OpencodeQuestionResponseCompleted { session_id, result } => {
                let is_active_tab = self
                    .state
                    .tab_manager
                    .active_session()
                    .map(|active| active.id == session_id)
                    .unwrap_or(false);
                let Some(session) = self.state.tab_manager.session_by_id_mut(session_id) else {
                    tracing::debug!(
                        %session_id,
                        "OpencodeQuestionResponseCompleted for unknown session; ignoring"
                    );
                    return Ok(effects);
                };

                let mut should_stop_footer_spinner = false;
                session.tools_in_flight = match session.tools_in_flight.checked_sub(1) {
                    Some(value) => value,
                    None => {
                        tracing::warn!("tools_in_flight underflow on OpencodeQuestionResponse");
                        0
                    }
                };
                session.set_processing_state(ProcessingState::Thinking);

                if session.tools_in_flight == 0 {
                    session.stop_processing();
                    should_stop_footer_spinner = is_active_tab;
                }

                if let Err(err) = result {
                    session.chat_view.push(
                        MessageDisplay::Error {
                            content: format!("OpenCode question response failed: {}", err),
                        }
                        .to_chat_message(),
                    );
                }
                if should_stop_footer_spinner {
                    self.state.stop_footer_spinner();
                }
            }
            AppEvent::TitleGenerated { session_id, result } => {
                // Single lookup - session must exist to proceed
                let Some(session) = self.state.tab_manager.session_by_id_mut(session_id) else {
                    tracing::debug!(
                        %session_id,
                        "Stale TitleGenerated event: session no longer exists"
                    );
                    return Ok(effects);
                };
                // Clear pending flag once, regardless of result
                session.title_generation_pending = false;

                match result {
                    Ok(generated) => {
                        tracing::info!(
                            %session_id,
                            title = %generated.title,
                            new_branch = ?generated.new_branch,
                            "Session title generated"
                        );

                        // Update session title and branch display
                        session.title = Some(generated.title.clone());
                        if let Some(new_branch) = &generated.new_branch {
                            session.status_bar.set_branch_name(Some(new_branch.clone()));
                        }

                        if generated.used_fallback {
                            let tool = generated.tool_used.as_deref().unwrap_or("fallback tool");
                            self.state.set_timed_footer_message(
                                format!("Title generated via {}", tool),
                                Duration::from_secs(4),
                            );
                        }

                        // Update sidebar directly with new branch name
                        // (avoids stale DB read if DB update failed but git rename succeeded)
                        if let (Some(ws_id), Some(ref new_branch)) =
                            (generated.workspace_id, &generated.new_branch)
                        {
                            self.state
                                .sidebar_data
                                .update_workspace_branch(ws_id, Some(new_branch.clone()));
                        }

                        // Save session state to persist the title
                        effects.push(Effect::SaveSessionState);
                    }
                    Err(e) => {
                        tracing::warn!(%session_id, error = %e, "Failed to generate session title");
                        // Show transient footer message (less noisy than chat message)
                        self.state.set_timed_footer_message(
                            format!("Title generation failed: {}", e),
                            Duration::from_secs(5),
                        );
                    }
                }
            }
            _ => {}
        }

        Ok(effects)
    }

    /// Handle updates from the background git tracker
    fn handle_git_tracker_update(&mut self, update: crate::ui::git_tracker::GitTrackerUpdate) {
        use crate::ui::git_tracker::GitTrackerUpdate;

        match update {
            GitTrackerUpdate::PrStatusChanged {
                workspace_id,
                status,
            } => {
                tracing::debug!(
                    workspace_id = %workspace_id,
                    pr_exists = status.as_ref().map(|s| s.exists),
                    pr_number = status.as_ref().and_then(|s| s.number),
                    pr_state = ?status.as_ref().map(|s| s.state),
                    check_state = ?status.as_ref().map(|s| s.checks.state()),
                    merge_readiness = ?status.as_ref().map(|s| s.merge_readiness),
                    "Received PR status update"
                );
                let is_stale_pr = status.as_ref().is_some_and(|s| {
                    matches!(
                        s.state,
                        crate::git::PrState::Merged | crate::git::PrState::Closed
                    )
                });
                let mut any_session_updated = false;
                // Update all sessions with this workspace
                for session in self.state.tab_manager.sessions_mut() {
                    if session.workspace_id == Some(workspace_id) {
                        // CRITICAL: Stale PR Prevention
                        // If session has no PR yet, don't auto-associate merged/closed PRs.
                        // This prevents "ghost" PRs from reused branch names from being resurrected.
                        let is_new_association = session.pr_number.is_none();

                        if is_new_association && is_stale_pr {
                            tracing::debug!(
                                workspace_id = %workspace_id,
                                pr_number = status.as_ref().and_then(|s| s.number),
                                "Ignoring stale (merged/closed) PR for new session"
                            );
                            self.state
                                .sidebar_data
                                .clear_workspace_pr_status(workspace_id);
                            continue;
                        }

                        if let Some(status) = status.clone() {
                            Self::apply_pr_status_to_session(session, status);
                            any_session_updated = true;
                        }
                    }
                }
                // Update sidebar data when we have an accepted association or when not stale.
                if !is_stale_pr || any_session_updated {
                    self.state
                        .sidebar_data
                        .update_workspace_pr_status(workspace_id, status);
                } else {
                    self.state
                        .sidebar_data
                        .clear_workspace_pr_status(workspace_id);
                }
            }
            GitTrackerUpdate::GitStatsChanged {
                workspace_id,
                stats,
            } => {
                tracing::info!(
                    workspace_id = %workspace_id,
                    additions = stats.additions,
                    deletions = stats.deletions,
                    files_changed = stats.files_changed,
                    "Received GitStatsChanged event"
                );

                // Update all sessions with this workspace
                for session in self.state.tab_manager.sessions_mut() {
                    if session.workspace_id == Some(workspace_id) {
                        session.status_bar.set_git_diff_stats(stats.clone());
                    }
                }
                // Also update sidebar data
                self.state
                    .sidebar_data
                    .update_workspace_git_stats(workspace_id, stats);
            }
            GitTrackerUpdate::BranchChanged {
                workspace_id,
                branch,
            } => {
                if self.state.busy_workspaces.contains(&workspace_id) {
                    tracing::debug!(
                        workspace_id = %workspace_id,
                        "Skipping branch update for busy workspace"
                    );
                    self.state
                        .pending_branch_updates
                        .insert(workspace_id, branch);
                    return;
                }
                self.apply_branch_update(workspace_id, branch);
            }
        }
    }

    fn apply_branch_update(&mut self, workspace_id: uuid::Uuid, branch: Option<String>) {
        for session in self.state.tab_manager.sessions_mut() {
            if session.workspace_id == Some(workspace_id) {
                session.status_bar.set_branch_name(branch.clone());
            }
        }
        self.state
            .sidebar_data
            .update_workspace_branch(workspace_id, branch);
    }

    fn flush_pending_agent_output(session: &mut crate::ui::session::AgentSession) {
        // Safety: ensure no partial streaming buffer remains before pushing buffered messages.
        session.chat_view.finalize_streaming();
        if let Some(summary) = session.pending_turn_summary.take() {
            session.chat_view.push(ChatMessage::turn_summary(summary));
        }
    }

    async fn handle_agent_event(
        &mut self,
        session_id: uuid::Uuid,
        event: AgentEvent,
    ) -> anyhow::Result<()> {
        let Some(tab_index) = self.state.tab_manager.session_index_by_id(session_id) else {
            tracing::debug!(
                %session_id,
                "Agent event for unknown session; ignoring"
            );
            return Ok(());
        };
        // Check if this is a non-active tab receiving content - mark as needing attention
        let is_active_tab = self.state.tab_manager.active_index() == tab_index;
        let is_content_event = matches!(
            &event,
            AgentEvent::AssistantMessage(_)
                | AgentEvent::AssistantReasoning(_)
                | AgentEvent::ToolStarted(_)
                | AgentEvent::ToolCompleted(_)
                | AgentEvent::CommandOutput(_)
                | AgentEvent::TurnCompleted(_)
                | AgentEvent::TurnFailed(_)
        );

        // Track whether we need to stop footer spinner (done after session borrow ends)
        let mut should_stop_footer_spinner = false;
        let mut should_start_footer_spinner = false;
        let mut pending_sidebar_pr_update: Option<(Uuid, PrStatus)> = None;
        let mut pending_model_invalidation = false;
        let mut should_drain_queue = false;
        let mut pending_observed_context_window: Option<(AgentType, String, i64)> = None;

        {
            let Some(session) = self.state.tab_manager.session_mut(tab_index) else {
                return Ok(());
            };

            // Mark non-active tabs as needing attention when content arrives
            // Exclude suppressed assistant messages (like fork seed ACKs)
            let is_suppressed_assistant = matches!(&event, AgentEvent::AssistantMessage(_))
                && session.suppress_next_assistant_reply;
            if !is_active_tab && is_content_event && !is_suppressed_assistant {
                session.needs_attention = true;
            }

            // Record raw event for debug view
            let (event_type, raw_json) = match &event {
                AgentEvent::Raw { data } => {
                    let event_type = data
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("Raw");
                    (event_type.to_string(), data.clone())
                }
                _ => match serde_json::to_value(&event) {
                    Ok(raw_json) => (event.event_type_name().to_string(), raw_json),
                    Err(error) => {
                        let event_type = event.event_type_name();
                        tracing::warn!(
                            event_type,
                            error = %error,
                            "Failed to serialize agent event for raw events view"
                        );
                        let fallback = serde_json::json!({
                            "type": event_type,
                            "serialize_failed": true,
                            "error": error.to_string(),
                        });
                        (event_type.to_string(), fallback)
                    }
                },
            };
            session.record_raw_event(EventDirection::Received, event_type, raw_json);

            match event {
                AgentEvent::SessionInit(init) => {
                    session.agent_session_id = Some(init.session_id);
                    // Clear pending message - agent has confirmed receipt
                    session.pending_user_message = None;
                    session.update_status();
                }
                AgentEvent::TurnStarted => {
                    session.is_processing = true;
                    session.update_status();
                }
                AgentEvent::TurnCompleted(completed) => {
                    session.add_usage(completed.usage);
                    session.stop_processing();
                    if session.inline_prompt.is_none() {
                        session.agent_input_tx = None;
                    }
                    if session.inline_prompt.is_none() && !session.queued_messages.is_empty() {
                        should_drain_queue = true;
                    }
                    // Safety net: avoid suppressing a future real assistant message
                    // (in case the final assistant message event never arrived)
                    session.suppress_next_assistant_reply = false;
                    // Only stop footer spinner if this is the active tab
                    if is_active_tab {
                        should_stop_footer_spinner = true;
                    }
                    session.chat_view.finalize_streaming();
                    // Add turn summary to chat
                    if session.suppress_next_turn_summary {
                        session.suppress_next_turn_summary = false;
                    } else {
                        if session.pending_turn_summary.is_some() {
                            Self::flush_pending_agent_output(session);
                        }
                        let summary = session.current_turn_summary.clone();
                        session.pending_turn_summary = Some(summary);
                        if session.chat_view.streaming_buffer().is_none() {
                            Self::flush_pending_agent_output(session);
                        }
                    }
                }
                AgentEvent::TurnFailed(failed) => {
                    session.stop_processing();
                    session.chat_view.finalize_streaming();
                    session.tools_in_flight = 0;
                    session.set_processing_state(ProcessingState::Thinking);
                    session.agent_input_tx = None;
                    // Only stop footer spinner if this is the active tab
                    if is_active_tab {
                        should_stop_footer_spinner = true;
                    }
                    session.suppress_next_assistant_reply = false;
                    session.suppress_next_turn_summary = false;
                    let display = MessageDisplay::Error {
                        content: failed.error,
                    };
                    session.chat_view.push(display.to_chat_message());
                }
                AgentEvent::AssistantReasoning(reasoning) => {
                    let token_estimate = (reasoning.text.len() / 4).max(1);
                    session.add_streaming_tokens(token_estimate);
                    session
                        .chat_view
                        .stream_append_role(MessageRole::Reasoning, &reasoning.text);
                }
                AgentEvent::AssistantMessage(msg) => {
                    if session.suppress_next_assistant_reply {
                        if msg.is_final {
                            session.suppress_next_assistant_reply = false;
                        }
                        // Skip rendering the fork seed acknowledgement
                        return Ok(());
                    }
                    // Track streaming tokens (rough estimate: ~4 chars per token)
                    let token_estimate = (msg.text.len() / 4).max(1);
                    session.add_streaming_tokens(token_estimate);

                    // Check for PR URL in the message and capture PR number
                    if session.pr_number.is_none() {
                        if let Some(pr_num) = Self::extract_pr_number_from_text(&msg.text) {
                            pending_sidebar_pr_update =
                                Self::apply_pr_number_to_session(session, pr_num);
                        }
                    }

                    session.chat_view.stream_append(&msg.text);
                    if msg.is_final {
                        Self::flush_pending_agent_output(session);
                    }
                }
                AgentEvent::ToolStarted(tool) => {
                    // Check for special interactive tools that use inline prompts
                    let is_inline_prompt_tool = if tool.tool_name == "AskUserQuestion" {
                        // Parse the questions from the tool arguments
                        match serde_json::from_value::<AskUserQuestionWrapper>(
                            tool.arguments.clone(),
                        ) {
                            Ok(wrapper) => {
                                session.inline_prompt = Some(InlinePromptState::new_ask_user(
                                    tool.tool_id.clone(),
                                    wrapper.questions,
                                ));
                                // Scroll to bottom so prompt is visible
                                session.chat_view.scroll_to_bottom();
                                // Don't push to chat - the inline prompt will be rendered as extra lines
                                session.tools_in_flight = session.tools_in_flight.saturating_add(1);
                                // Stop footer spinner since we're now awaiting user response
                                should_stop_footer_spinner = true;
                                true
                            }
                            Err(e) => {
                                tracing::warn!(
                                    tool_id = %tool.tool_id,
                                    tool_name = %tool.tool_name,
                                    arguments = %serde_json::to_string(&tool.arguments).unwrap_or_default(),
                                    error = %e,
                                    "Failed to deserialize AskUserQuestion arguments"
                                );
                                // Surface error to user so they know why prompt didn't appear
                                let display = MessageDisplay::Error {
                                    content: format!("Failed to parse AskUserQuestion: {}", e),
                                };
                                session.chat_view.push(display.to_chat_message());
                                false
                            }
                        }
                    } else if tool.tool_name == "ExitPlanMode" {
                        // Use plan content from tool arguments when available
                        let (plan_content, plan_path) =
                            match serde_json::from_value::<ExitPlanModeWrapper>(
                                tool.arguments.clone(),
                            ) {
                                Ok(wrapper) => {
                                    let plan_path = Self::read_plan_file_path_for_session(session)
                                        .unwrap_or_else(|| ".claude/plans/plan.md".to_string());
                                    (wrapper.plan, plan_path)
                                }
                                Err(e) => {
                                    // Fall back to reading plan from file
                                    tracing::debug!(
                                        tool_id = %tool.tool_id,
                                        error = %e,
                                        "ExitPlanMode arguments missing plan, falling back to file"
                                    );
                                    Self::read_plan_file_for_session(session)
                                }
                            };

                        session.inline_prompt = Some(InlinePromptState::new_exit_plan(
                            tool.tool_id.clone(),
                            plan_content,
                            plan_path,
                        ));
                        // Scroll to bottom so prompt is visible
                        session.chat_view.scroll_to_bottom();
                        // Don't push to chat - the inline prompt will be rendered as extra lines
                        session.tools_in_flight = session.tools_in_flight.saturating_add(1);
                        // Stop footer spinner since we're now awaiting user response
                        should_stop_footer_spinner = true;
                        true
                    } else {
                        false
                    };

                    // Skip normal tool processing for inline prompt tools
                    if !is_inline_prompt_tool {
                        // Update processing state to show tool name
                        session
                            .set_processing_state(ProcessingState::ToolUse(tool.tool_name.clone()));
                        // ToolStarted pairs with ToolCompleted for non-shell tools or CommandOutput
                        // for shell tools; these events are mutually exclusive in agent runners.
                        session.tools_in_flight = session.tools_in_flight.saturating_add(1);

                        let args_str = if tool.arguments.is_null() {
                            String::new()
                        } else {
                            // Compact single-line for display
                            serde_json::to_string(&tool.arguments).unwrap_or_default()
                        };
                        let display = MessageDisplay::Tool {
                            name: MessageDisplay::tool_display_name_owned(&tool.tool_name),
                            args: args_str,
                            output: "Running...".to_string(),
                            exit_code: None,
                            file_size: None, // Only set for Read tool on images via update_last_tool
                        };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
                AgentEvent::ControlRequest(request) => {
                    if let Some(tool_use_id) = request.tool_use_id.clone() {
                        session
                            .pending_tool_permissions
                            .insert(tool_use_id.clone(), request.request_id.clone());

                        if let Some(response_payload) = session
                            .pending_tool_permission_responses
                            .remove(&tool_use_id)
                        {
                            if let Ok(jsonl) = Self::build_control_response_jsonl(
                                &request.request_id,
                                response_payload,
                            ) {
                                if let Some(ref input_tx) = session.agent_input_tx {
                                    let input_tx = input_tx.clone();
                                    tokio::spawn(async move {
                                        if let Err(err) =
                                            input_tx.send(AgentInput::ClaudeJsonl(jsonl)).await
                                        {
                                            tracing::warn!(
                                                "Failed to send deferred control response: {}",
                                                err
                                            );
                                        }
                                    });
                                    session.start_processing();
                                    session.set_processing_state(ProcessingState::Thinking);
                                    if is_active_tab {
                                        should_start_footer_spinner = true;
                                    }
                                }
                            }
                            session.pending_tool_permissions.remove(&tool_use_id);
                        }
                    } else {
                        tracing::warn!(
                            tool_name = request.tool_name,
                            "Control request missing tool_use_id"
                        );
                    }
                }
                AgentEvent::ToolCompleted(tool) => {
                    tracing::info!(
                        "ToolCompleted event: tool_id={}, success={}, result_len={}",
                        tool.tool_id,
                        tool.success,
                        tool.result.as_ref().map(|r| r.len()).unwrap_or(0)
                    );

                    // Return to thinking state
                    session.set_processing_state(ProcessingState::Thinking);
                    session.tools_in_flight = match session.tools_in_flight.checked_sub(1) {
                        Some(value) => value,
                        None => {
                            tracing::warn!("tools_in_flight underflow on ToolCompleted");
                            0
                        }
                    };

                    // Track file changes for write/edit tools
                    if tool.success {
                        let tool_name_lower = tool.tool_id.to_lowercase();
                        if tool_name_lower.contains("edit")
                            || tool_name_lower.contains("write")
                            || tool_name_lower.contains("multiedit")
                        {
                            // Try to extract filename from result or use generic name
                            if let Some(ref result) = tool.result {
                                // Simple heuristic: look for file paths in result
                                if let Some(filename) = Self::extract_filename(result) {
                                    // Rough estimate of changes (can be refined)
                                    session.record_file_change(filename, 5, 2);
                                }
                            }
                        }
                    }

                    let output = if tool.success {
                        tool.result.unwrap_or_else(|| "Completed".to_string())
                    } else {
                        format!("Error: {}", tool.error.unwrap_or_default())
                    };
                    // Update the existing "Running..." message instead of pushing a new one
                    if !session.chat_view.update_last_tool(output, None) {
                        tracing::warn!("ToolCompleted: no matching tool message found to update");
                    }
                }
                AgentEvent::CommandOutput(cmd) => {
                    // Check for PR URL in command output (e.g., from gh pr create)
                    if session.pr_number.is_none() {
                        if let Some(pr_num) = Self::extract_pr_number_from_text(&cmd.output) {
                            pending_sidebar_pr_update =
                                Self::apply_pr_number_to_session(session, pr_num);
                        }
                    }

                    // Update the existing "Running..." message instead of pushing a new one
                    if !session
                        .chat_view
                        .update_last_tool(cmd.output.clone(), cmd.exit_code)
                    {
                        tracing::warn!("CommandOutput: no matching tool message found to update");
                    }
                    if !cmd.is_streaming {
                        session.tools_in_flight = match session.tools_in_flight.checked_sub(1) {
                            Some(value) => value,
                            None => {
                                tracing::warn!(
                                    "tools_in_flight underflow on CommandOutput (non-streaming)"
                                );
                                0
                            }
                        };
                    }
                }
                AgentEvent::Error(err) => {
                    let display = MessageDisplay::Error {
                        content: err.message,
                    };
                    session.chat_view.push(display.to_chat_message());
                    if err.code.as_deref() == Some("model_not_found") {
                        session.model = None;
                        session.model_invalid = true;
                        session.update_status();
                        pending_model_invalidation = true;
                    }
                    if err.is_fatal {
                        session.stop_processing();
                        session.chat_view.finalize_streaming();
                        session.tools_in_flight = 0;
                        session.set_processing_state(ProcessingState::Thinking);
                        session.agent_input_tx = None;
                        // Only stop footer spinner if this is the active tab
                        if is_active_tab {
                            should_stop_footer_spinner = true;
                        }
                    }
                }
                AgentEvent::TokenUsage(usage_event) => {
                    session.update_context_usage(&usage_event);
                    if let Some(context_window) = usage_event.context_window {
                        if context_window > 0 {
                            let model_id = session.model.clone().unwrap_or_else(|| {
                                ModelRegistry::default_model(session.agent_type)
                            });
                            pending_observed_context_window =
                                Some((session.agent_type, model_id, context_window));
                        }
                    }

                    // Check if we need to show a warning notification
                    if let Some(warning) = session.pending_context_warning.take() {
                        use crate::agent::events::ContextWarningLevel;
                        let display = match warning.level {
                            ContextWarningLevel::Critical => MessageDisplay::Error {
                                content: warning.message,
                            },
                            ContextWarningLevel::High | ContextWarningLevel::Medium => {
                                MessageDisplay::System {
                                    content: format!("⚠️ {}", warning.message),
                                }
                            }
                            ContextWarningLevel::Normal => MessageDisplay::System {
                                content: format!("ℹ️ {}", warning.message),
                            },
                        };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
                AgentEvent::ContextCompaction(compaction_event) => {
                    use crate::agent::events::ContextWindowState;
                    session.handle_compaction(compaction_event.clone());

                    // Always show compaction notification in chat
                    let display = MessageDisplay::System {
                        content: format!(
                            "🔄 Context compacted: {} → {} tokens (reason: {})",
                            ContextWindowState::format_tokens(compaction_event.tokens_before),
                            ContextWindowState::format_tokens(compaction_event.tokens_after),
                            compaction_event.reason
                        ),
                    };
                    session.chat_view.push(display.to_chat_message());

                    // Clear any pending warning since we just compacted
                    session.pending_context_warning = None;
                }
                _ => {}
            }
        } // End session borrow scope

        if let Some((workspace_id, status)) = pending_sidebar_pr_update {
            self.state
                .sidebar_data
                .update_workspace_pr_status(workspace_id, Some(status));
        }
        if let Some((agent_type, model_id, context_window)) = pending_observed_context_window {
            ContextWindowService::record_observed(
                &self.core,
                agent_type,
                &model_id,
                context_window,
            );
        }
        if pending_model_invalidation {
            if let Some(session_tab_dao) = self.session_tab_dao_clone() {
                if let Ok(Some(mut tab)) = session_tab_dao.get_by_id(session_id) {
                    tab.model = None;
                    tab.model_invalid = true;
                    if let Err(err) = session_tab_dao.update(&tab) {
                        tracing::warn!(
                            error = %err,
                            session_id = %session_id,
                            "Failed to persist model invalidation"
                        );
                    }
                } else {
                    tracing::warn!(
                        session_id = %session_id,
                        "Failed to load session for model invalidation"
                    );
                }
            }
        }

        if should_drain_queue {
            match self.drain_queue_for_tab(tab_index) {
                Ok(effects) if !effects.is_empty() => {
                    self.run_effects(effects).await?;
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to drain queued messages");
                }
            }
        }

        // Stop footer spinner after session borrow is released
        if should_stop_footer_spinner {
            self.state.stop_footer_spinner();
        }
        if should_start_footer_spinner {
            self.state.start_footer_spinner(None);
        }

        Ok(())
    }

    fn submit_prompt(
        &mut self,
        prompt: String,
        images: Vec<PathBuf>,
        image_placeholders: Vec<String>,
    ) -> anyhow::Result<Vec<Effect>> {
        let tab_index = self.state.tab_manager.active_index();
        self.submit_prompt_for_tab(tab_index, prompt, images, image_placeholders, false, None)
    }

    fn submit_prompt_hidden(
        &mut self,
        prompt: String,
        images: Vec<PathBuf>,
        image_placeholders: Vec<String>,
    ) -> anyhow::Result<Vec<Effect>> {
        let tab_index = self.state.tab_manager.active_index();
        self.submit_prompt_for_tab(tab_index, prompt, images, image_placeholders, true, None)
    }

    fn submit_prompt_hidden_jsonl(&mut self, payload: String) -> anyhow::Result<Vec<Effect>> {
        let tab_index = self.state.tab_manager.active_index();
        self.submit_prompt_for_tab(
            tab_index,
            String::new(),
            Vec::new(),
            Vec::new(),
            true,
            Some(payload),
        )
    }

    /// Send a tool result back to the agent by resuming the session with a hidden prompt.
    ///
    /// Claude Code CLI in headless mode accepts structured stdin input, so we resume the
    /// session with a tool_result payload over stream-json.
    ///
    /// For AskUserQuestion: The result contains the user's answers
    /// For ExitPlanMode: The result indicates approval or feedback
    fn send_tool_result(
        &mut self,
        tool_id: &str,
        content: String,
        tool_use_result: Option<serde_json::Value>,
    ) -> Vec<Effect> {
        let payload = Self::build_tool_result_jsonl(tool_id, &content, tool_use_result);
        match payload {
            Ok(jsonl) => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.agent_type == AgentType::Claude {
                        if let Some(ref input_tx) = session.agent_input_tx {
                            let input_tx = input_tx.clone();
                            let jsonl_to_send = jsonl.clone();
                            tokio::spawn(async move {
                                if let Err(err) =
                                    input_tx.send(AgentInput::ClaudeJsonl(jsonl_to_send)).await
                                {
                                    tracing::warn!(
                                        "Failed to send tool result via streaming input: {}",
                                        err
                                    );
                                }
                            });
                            let pending_tools = session.tools_in_flight;
                            session.start_processing();
                            session.tools_in_flight = pending_tools.saturating_sub(1);
                            session.set_processing_state(ProcessingState::Thinking);
                            self.state.start_footer_spinner(None);
                            return Vec::new();
                        }
                    }
                }

                match self.submit_prompt_hidden_jsonl(jsonl) {
                    Ok(effects) => effects,
                    Err(e) => {
                        tracing::error!("Failed to send tool result: {}", e);
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to build tool result payload: {}", e);
                Vec::new()
            }
        }
    }

    fn send_opencode_question_response(
        &mut self,
        request_id: &str,
        answers: Option<Vec<Vec<String>>>,
    ) -> Vec<Effect> {
        let (input_tx, session_id, should_start_footer_spinner, should_stop_footer_spinner, abort) = {
            let Some(session) = self.state.tab_manager.active_session_mut() else {
                return Vec::new();
            };
            if session.agent_type != AgentType::Opencode {
                return Vec::new();
            }
            let pending_tools = session.tools_in_flight;
            session.start_processing();
            session.tools_in_flight = pending_tools.saturating_add(1);
            session.set_processing_state(ProcessingState::Thinking);
            let mut should_start_footer_spinner = true;
            let mut should_stop_footer_spinner = false;
            let mut abort = false;

            if session.agent_input_tx.is_none() {
                session.chat_view.push(
                    MessageDisplay::Error {
                        content: "OpenCode question response failed: session not ready."
                            .to_string(),
                    }
                    .to_chat_message(),
                );
                session.tools_in_flight = session.tools_in_flight.saturating_sub(1);
                session.stop_processing();
                session.set_processing_state(ProcessingState::Thinking);
                should_start_footer_spinner = false;
                should_stop_footer_spinner = true;
                abort = true;
            }

            (
                session.agent_input_tx.clone(),
                Some(session.id),
                should_start_footer_spinner,
                should_stop_footer_spinner,
                abort,
            )
        };

        if should_start_footer_spinner {
            self.state.start_footer_spinner(None);
        }
        if should_stop_footer_spinner {
            self.state.stop_footer_spinner();
        }
        if abort {
            return Vec::new();
        }

        let Some(input_tx) = input_tx else {
            return Vec::new();
        };
        let session_id = match session_id {
            Some(session_id) => session_id,
            None => return Vec::new(),
        };

        let request_id = request_id.to_string();
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = input_tx
                .send(AgentInput::OpencodeQuestion {
                    request_id,
                    answers,
                })
                .await
                .map_err(|err| err.to_string());
            let _ = send_app_event(
                &event_tx,
                AppEvent::OpencodeQuestionResponseCompleted { session_id, result },
                "opencode_question_response",
            );
        });
        Vec::new()
    }

    fn send_control_response(
        &mut self,
        request_id: &str,
        response_payload: serde_json::Value,
    ) -> Vec<Effect> {
        let payload = Self::build_control_response_jsonl(request_id, response_payload);
        match payload {
            Ok(jsonl) => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.agent_type == AgentType::Claude {
                        if let Some(ref input_tx) = session.agent_input_tx {
                            let input_tx = input_tx.clone();
                            let jsonl_to_send = jsonl.clone();
                            tokio::spawn(async move {
                                if let Err(err) =
                                    input_tx.send(AgentInput::ClaudeJsonl(jsonl_to_send)).await
                                {
                                    tracing::warn!(
                                        "Failed to send control response via streaming input: {}",
                                        err
                                    );
                                }
                            });
                            // Preserve tools_in_flight count, then decrement after starting processing
                            // (mirrors send_tool_result behavior for consistency)
                            let pending_tools = session.tools_in_flight;
                            session.start_processing();
                            session.tools_in_flight = pending_tools.saturating_sub(1);
                            session.set_processing_state(ProcessingState::Thinking);
                            self.state.start_footer_spinner(None);
                            return Vec::new();
                        }
                    }
                }

                tracing::warn!("Unable to send control response: missing Claude input channel");
                // Surface error to user and clean up state
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.stop_processing();
                    let display = MessageDisplay::Error {
                        content: "Cannot reply to prompt: missing streaming input channel. Try restarting the session.".to_string(),
                    };
                    session.chat_view.push(display.to_chat_message());
                }
                self.state.stop_footer_spinner();
                Vec::new()
            }
            Err(e) => {
                tracing::error!("Failed to build control response payload: {}", e);
                // Surface error to user
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.stop_processing();
                    let display = MessageDisplay::Error {
                        content: format!("Failed to send response: {}", e),
                    };
                    session.chat_view.push(display.to_chat_message());
                }
                self.state.stop_footer_spinner();
                Vec::new()
            }
        }
    }

    fn build_tool_result_jsonl(
        tool_id: &str,
        content: &str,
        tool_use_result: Option<serde_json::Value>,
    ) -> anyhow::Result<String> {
        let mut payload = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_id,
                    "content": content,
                    "is_error": false,
                }]
            }
        });

        if let Some(value) = tool_use_result {
            if let serde_json::Value::Object(obj) = &mut payload {
                obj.insert("toolUseResult".to_string(), value);
            }
        }

        let json = serde_json::to_string(&payload)?;
        Ok(format!("{json}\n"))
    }

    fn build_control_response_jsonl(
        request_id: &str,
        response_payload: serde_json::Value,
    ) -> anyhow::Result<String> {
        let payload = serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": response_payload,
            }
        });
        let json = serde_json::to_string(&payload)?;
        Ok(format!("{json}\n"))
    }

    /// Encode an image file to base64 and determine its media type
    fn encode_image_to_base64(path: &PathBuf) -> anyhow::Result<(String, String)> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let data = std::fs::read(path)
            .map_err(|e| anyhow!("Failed to read image file {}: {}", path.display(), e))?;

        let media_type = match path.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/png", // Default to PNG for unknown extensions
        };

        let base64_data = STANDARD.encode(&data);
        Ok((base64_data, media_type.to_string()))
    }

    fn build_user_prompt_jsonl(prompt: &str, images: &[PathBuf]) -> anyhow::Result<String> {
        let mut content_blocks: Vec<serde_json::Value> = Vec::new();

        tracing::info!(
            "build_user_prompt_jsonl: building JSONL with {} images, prompt_len={}",
            images.len(),
            prompt.len()
        );

        // Add image content blocks first (Claude works best with images before text)
        for (i, path) in images.iter().enumerate() {
            tracing::info!(
                "build_user_prompt_jsonl: processing image {} at {:?}",
                i,
                path
            );
            match Self::encode_image_to_base64(path) {
                Ok((base64_data, media_type)) => {
                    tracing::info!(
                        "build_user_prompt_jsonl: encoded image {} successfully, media_type={}, base64_len={}",
                        i,
                        media_type,
                        base64_data.len()
                    );
                    // Add image label if multiple images
                    if images.len() > 1 {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": format!("Image {}:", i + 1),
                        }));
                    }
                    content_blocks.push(serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": base64_data,
                        }
                    }));
                }
                Err(e) => {
                    tracing::warn!("Failed to encode image {}: {}", path.display(), e);
                    // Fall back to mentioning the file path
                    content_blocks.push(serde_json::json!({
                        "type": "text",
                        "text": format!("[Failed to load image: {}]", path.display()),
                    }));
                }
            }
        }

        // Add text content block
        if !prompt.is_empty() {
            content_blocks.push(serde_json::json!({
                "type": "text",
                "text": prompt,
            }));
        }

        let payload = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": content_blocks,
            }
        });
        let json = serde_json::to_string(&payload)?;
        tracing::info!(
            "build_user_prompt_jsonl: final JSONL payload size={} bytes, content_blocks={}",
            json.len(),
            content_blocks.len()
        );
        Ok(format!("{json}\n"))
    }

    fn build_permission_allow_response(
        updated_input: serde_json::Value,
        tool_use_id: Option<&str>,
    ) -> serde_json::Value {
        let mut response = serde_json::Map::new();
        response.insert(
            "behavior".to_string(),
            serde_json::Value::String("allow".to_string()),
        );
        response.insert("updatedInput".to_string(), updated_input);
        if let Some(tool_use_id) = tool_use_id {
            response.insert(
                "toolUseID".to_string(),
                serde_json::Value::String(tool_use_id.to_string()),
            );
        }
        serde_json::Value::Object(response)
    }

    fn build_permission_deny_response(
        message: String,
        tool_use_id: Option<&str>,
    ) -> serde_json::Value {
        let mut response = serde_json::Map::new();
        response.insert(
            "behavior".to_string(),
            serde_json::Value::String("deny".to_string()),
        );
        response.insert("message".to_string(), serde_json::Value::String(message));
        if let Some(tool_use_id) = tool_use_id {
            response.insert(
                "toolUseID".to_string(),
                serde_json::Value::String(tool_use_id.to_string()),
            );
        }
        serde_json::Value::Object(response)
    }

    fn build_ask_user_updated_input(
        prompt: &InlinePromptState,
        answers: &std::collections::HashMap<String, PromptAnswer>,
    ) -> serde_json::Value {
        let questions = match &prompt.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => questions.clone(),
            _ => Vec::new(),
        };

        let mut answers_map = serde_json::Map::new();
        for (question, answer) in answers {
            let formatted = Self::format_prompt_answer(answer);
            answers_map.insert(question.clone(), serde_json::Value::String(formatted));
        }

        serde_json::json!({
            "questions": questions,
            "answers": serde_json::Value::Object(answers_map),
        })
    }

    fn build_exit_plan_updated_input(prompt: &InlinePromptState) -> serde_json::Value {
        match &prompt.prompt_type {
            InlinePromptType::ExitPlanMode { plan_content, .. } => {
                serde_json::json!({ "plan": plan_content })
            }
            _ => serde_json::Value::Null,
        }
    }

    fn build_ask_user_tool_result(
        prompt: &InlinePromptState,
        answers: &std::collections::HashMap<String, PromptAnswer>,
    ) -> (String, Option<serde_json::Value>) {
        let mut parts = Vec::new();
        for (question, answer) in answers {
            let formatted = Self::format_prompt_answer(answer);
            parts.push(format!("\"{}\"=\"{}\"", question, formatted));
        }

        let content = if parts.is_empty() {
            "User has answered your questions. You can now continue with the user's answers in mind."
                .to_string()
        } else {
            format!(
                "User has answered your questions: {}. You can now continue with the user's answers in mind.",
                parts.join(", ")
            )
        };

        let tool_use_result = match &prompt.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                let mut answers_map = serde_json::Map::new();
                for (question, answer) in answers {
                    let formatted = Self::format_prompt_answer(answer);
                    answers_map.insert(question.clone(), serde_json::Value::String(formatted));
                }
                Some(serde_json::json!({
                    "questions": questions,
                    "answers": serde_json::Value::Object(answers_map),
                }))
            }
            _ => None,
        };

        (content, tool_use_result)
    }

    fn build_opencode_question_answers(
        prompt: &InlinePromptState,
        answers: &std::collections::HashMap<String, PromptAnswer>,
    ) -> Vec<Vec<String>> {
        let questions = match &prompt.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => questions,
            _ => return Vec::new(),
        };

        questions
            .iter()
            .map(|question| {
                answers
                    .get(&question.question)
                    .map(|answer| match answer {
                        PromptAnswer::Single(text) => vec![text.clone()],
                        PromptAnswer::Multiple(items) => items.clone(),
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    fn build_exit_plan_tool_result(
        prompt: &InlinePromptState,
        approved: bool,
        feedback: Option<String>,
    ) -> (String, Option<serde_json::Value>) {
        let (plan_content, plan_file_path) = match &prompt.prompt_type {
            InlinePromptType::ExitPlanMode {
                plan_content,
                plan_file_path,
            } => (plan_content.clone(), plan_file_path.clone()),
            _ => (String::new(), ".claude/plans/plan.md".to_string()),
        };

        let tool_use_result = Some(serde_json::json!({
            "plan": plan_content.clone(),
            "isAgent": false,
            "filePath": plan_file_path.clone(),
        }));

        let content = if approved {
            format!(
                "User has approved your plan. You can now start coding. Start with updating your todo list if applicable\n\nYour plan has been saved to: {}\nYou can refer back to it if needed during implementation.\n\n## Approved Plan:\n{}",
                plan_file_path,
                plan_content
            )
        } else if let Some(feedback) = feedback {
            format!("User feedback on plan: {}", feedback)
        } else {
            "User feedback on plan.".to_string()
        };

        (content, tool_use_result)
    }

    fn format_prompt_answer(answer: &PromptAnswer) -> String {
        match answer {
            PromptAnswer::Single(text) => text.clone(),
            PromptAnswer::Multiple(items) => items.join(", "),
        }
    }

    fn submit_prompt_for_tab(
        &mut self,
        tab_index: usize,
        prompt: String,
        images: Vec<PathBuf>,
        image_placeholders: Vec<String>,
        hidden: bool,
        stdin_payload: Option<String>,
    ) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();

        if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
            Self::flush_pending_agent_output(session);
        }

        // Extract session info in a limited borrow scope
        // NOTE: We don't take() resume_session_id here because early returns below
        // (e.g., working_dir validation) would consume it incorrectly. We only
        // consume resume_session_id later when we're committed to spawning the agent.
        // Get default working dir before the mutable borrow
        let default_working_dir = self.config().working_dir.clone();

        let (
            agent_type,
            agent_mode,
            model,
            reasoning_effort,
            model_invalid,
            session_id_to_use,
            working_dir,
            is_new_session_for_title,
            session_id,
        ) = {
            let Some(session) = self.state.tab_manager.session_mut(tab_index) else {
                return Ok(effects);
            };

            // "New session" for auto-title purposes == no visible user message has ever been shown.
            // This intentionally ignores hidden prompts (e.g., fork seeds), which don't push a
            // chat user message and shouldn't suppress auto-title on the first real user message.
            let has_visible_user_message = session
                .chat_view
                .messages()
                .iter()
                .any(|m| m.role == MessageRole::User);

            let agent_type = session.agent_type;
            let agent_mode = session.agent_mode;
            let model = session.model.clone();
            let reasoning_effort = session.reasoning_effort;
            let model_invalid = session.model_invalid;
            // Use agent_session_id if available (set by agent after first prompt)
            // Fall back to resume_session_id (clone, don't take - we consume it later)
            let session_id_to_use = if agent_type == AgentType::Codex
                && session.agent_input_tx.is_none()
                && session.agent_session_id.is_none()
            {
                None
            } else {
                session
                    .agent_session_id
                    .clone()
                    .or_else(|| session.resume_session_id.clone())
            };
            // Use session's working_dir if set, otherwise fall back to config
            let working_dir = session.working_dir.clone().unwrap_or(default_working_dir);
            let session_id = session.id;

            tracing::debug!(
                session_id = %session_id,
                agent = %agent_type,
                has_input_tx = session.agent_input_tx.is_some(),
                agent_session_id = session.agent_session_id.as_ref().map(|id| id.as_str()),
                resume_session_id = session.resume_session_id.as_ref().map(|id| id.as_str()),
                selected_session_id = session_id_to_use.as_ref().map(|id| id.as_str()),
                "submit_prompt_for_tab resolved session state"
            );

            (
                agent_type,
                agent_mode,
                model,
                reasoning_effort,
                model_invalid,
                session_id_to_use,
                working_dir,
                !has_visible_user_message,
                session_id,
            )
        };

        let resolved_input = CommandResolver::resolve(&prompt, &working_dir, agent_type);
        match &resolved_input {
            ResolveResult::ConduitCommand { command, .. } => {
                return self.execute_resolved_conduit_command(tab_index, *command);
            }
            ResolveResult::ListRequest { trigger } => {
                self.open_resolver_menu(*trigger);
                return Ok(effects);
            }
            _ => {}
        }

        let display_prompt = prompt;
        let mut agent_prompt = display_prompt.clone();
        let mut codex_skill = None;
        let mut stdin_payload = stdin_payload;
        let use_inline_plan_prompt = Self::plan_prompt_inline_enabled();

        if let ResolveResult::ProviderPrompt(ResolvedPrompt {
            agent_text,
            codex_skill: resolved_skill,
            ..
        }) = resolved_input
        {
            agent_prompt = agent_text;
            codex_skill = resolved_skill;
        }

        // Validate working directory exists before showing user message
        if !working_dir.exists() {
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                let display = MessageDisplay::Error {
                    content: format!(
                        "Working directory does not exist: {}",
                        working_dir.display()
                    ),
                };
                session.chat_view.push(display.to_chat_message());
            }
            return Ok(effects);
        }

        if model_invalid || model.is_none() {
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                session.model_invalid = true;
                let display = MessageDisplay::Error {
                    content: "Select a model to continue.".to_string(),
                };
                session.chat_view.push(display.to_chat_message());
            }
            return Ok(effects);
        }

        // Capture original user message for title generation BEFORE agent-specific transformations
        // (e.g., Codex placeholder stripping, Claude image-path appends)
        let prompt_for_title = display_prompt.clone();
        let working_dir_for_title = working_dir.clone();

        // Add user message to chat and start processing (after validation passes)
        // For hidden prompts (like fork seeds), skip showing in chat and pending_user_message
        if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
            if !hidden {
                let display = MessageDisplay::User {
                    content: display_prompt.clone(),
                };
                session.chat_view.push(display.to_chat_message());
                // Store pending message for persistence (cleared on agent confirmation)
                session.pending_user_message = Some(display_prompt.clone());
            }
            session.start_processing();
        }
        if self.state.tab_manager.active_index() == tab_index {
            self.state.start_footer_spinner(None);
        }

        // Start agent
        if matches!(agent_type, AgentType::Gemini | AgentType::Opencode) && !images.is_empty() {
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                session.stop_processing();
                session.pending_user_message = None;
                let display = MessageDisplay::Error {
                    content: match agent_type {
                        AgentType::Gemini => {
                            "Image attachments aren't supported for Gemini in Conduit yet."
                                .to_string()
                        }
                        AgentType::Opencode => {
                            "Image attachments aren't supported for OpenCode in Conduit yet."
                                .to_string()
                        }
                        _ => "Image attachments aren't supported for this agent.".to_string(),
                    },
                };
                session.chat_view.push(display.to_chat_message());
            }
            if self.state.tab_manager.active_index() == tab_index {
                self.state.stop_footer_spinner();
            }
            return Ok(effects);
        }

        // Strip placeholders for agents that send images out-of-band.
        if matches!(
            agent_type,
            AgentType::Codex | AgentType::Claude | AgentType::Gemini | AgentType::Opencode
        ) {
            agent_prompt = Self::strip_image_placeholders(agent_prompt, &image_placeholders);
        }

        if agent_prompt.trim().is_empty() && images.is_empty() && stdin_payload.is_none() {
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                session.stop_processing();
                let display = MessageDisplay::Error {
                    content: "Cannot submit: prompt is empty after processing".to_string(),
                };
                session.chat_view.push(display.to_chat_message());
            }
            if self.state.tab_manager.active_index() == tab_index {
                self.state.stop_footer_spinner();
            }
            return Ok(effects);
        }

        if !hidden {
            let mode_prompt = self
                .state
                .tab_manager
                .session_mut(tab_index)
                .and_then(|session| Self::take_mode_prompt(session, use_inline_plan_prompt));
            if let Some(mode_prompt) = mode_prompt {
                agent_prompt = Self::prepend_mode_prompt(&mode_prompt, &agent_prompt);
            }
        }

        // Record user input for debug view (post-processing)
        // For hidden prompts (like fork seeds), redact content to avoid storing ~500KB
        let mut debug_payload = serde_json::json!({
            "agent_type": agent_type.as_str(),
            "hidden": hidden,
        });
        if hidden {
            debug_payload["prompt_len"] = serde_json::json!(agent_prompt.len());
            debug_payload["prompt_hash"] =
                serde_json::json!(app_prompt::compute_seed_prompt_hash(&agent_prompt));
            if let Some(ref payload) = stdin_payload {
                debug_payload["stdin_payload_len"] = serde_json::json!(payload.len());
                debug_payload["stdin_payload_hash"] =
                    serde_json::json!(app_prompt::compute_seed_prompt_hash(payload));
            }
        } else {
            debug_payload["prompt"] = serde_json::json!(&agent_prompt);
        }
        if !images.is_empty() {
            let image_paths: Vec<String> = images
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            debug_payload["images"] = serde_json::json!(image_paths);
        }
        if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
            session.record_raw_event(EventDirection::Sent, "UserPrompt", debug_payload);
        }

        let mut use_stream_json = false;
        if agent_type == AgentType::Claude {
            use_stream_json = true;
            if stdin_payload.is_none() {
                tracing::info!(
                    "submit_prompt_for_tab: building JSONL for Claude with {} images",
                    images.len()
                );
                stdin_payload = Some(Self::build_user_prompt_jsonl(&agent_prompt, &images)?);
            }
        }

        if agent_type == AgentType::Claude {
            let is_active_tab = self.state.tab_manager.active_index() == tab_index;
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                if let Some(ref input_tx) = session.agent_input_tx {
                    if let Some(payload) = stdin_payload.clone() {
                        let input_tx = input_tx.clone();
                        tokio::spawn(async move {
                            if let Err(err) = input_tx.send(AgentInput::ClaudeJsonl(payload)).await
                            {
                                tracing::warn!("Failed to send streaming prompt: {}", err);
                            }
                        });

                        session.start_processing();
                        session.set_processing_state(ProcessingState::Thinking);
                        if is_active_tab {
                            self.state.start_footer_spinner(None);
                        }
                        return Ok(Vec::new());
                    }
                }
            }
        }

        if matches!(agent_type, AgentType::Codex | AgentType::Opencode) {
            let is_active_tab = self.state.tab_manager.active_index() == tab_index;
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                if let Some(ref input_tx) = session.agent_input_tx {
                    let input_tx = input_tx.clone();
                    let prompt_to_send = agent_prompt.clone();
                    let images_to_send = images.clone();
                    tokio::spawn(async move {
                        let input = AgentInput::CodexPrompt {
                            text: prompt_to_send,
                            images: images_to_send,
                            model: model.clone(),
                            skill: codex_skill.clone(),
                        };
                        if let Err(err) = input_tx.send(input).await {
                            tracing::warn!("Failed to send prompt: {}", err);
                        }
                    });

                    session.start_processing();
                    session.set_processing_state(ProcessingState::Thinking);
                    if is_active_tab {
                        self.state.start_footer_spinner(None);
                    }
                    return Ok(Vec::new());
                }
            }
        }

        let prompt_for_agent = if agent_type == AgentType::Claude {
            String::new()
        } else {
            agent_prompt.clone()
        };

        let mut config = AgentStartConfig::new(prompt_for_agent, working_dir)
            .with_tools(self.config().claude_allowed_tools.clone())
            .with_images(images)
            .with_agent_mode(agent_mode);

        if let Some(skill) = codex_skill {
            config = config.with_skill(skill);
        }

        // Add model if specified
        if let Some(model_id) = model {
            config = config.with_model(model_id);
        }
        if let Some(effort) = reasoning_effort {
            config = config.with_reasoning_effort(effort);
        }

        // Structured stdin payload (used for tool results / stream-json input)
        if let Some(payload) = stdin_payload {
            config = config
                .with_input_format("stream-json")
                .with_stdin_payload(payload);
        } else if use_stream_json {
            config = config.with_input_format("stream-json");
        }

        // Add session ID to continue existing conversation
        if let Some(session_id) = session_id_to_use {
            config = config.with_resume(session_id);
        }

        // Now that we're committed to spawning the agent, consume the resume_session_id
        // to prevent it from being used again on subsequent submits
        if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
            session.resume_session_id.take();
        }

        effects.push(Effect::StartAgent {
            session_id,
            agent_type,
            config,
        });

        // Generate title on first user message of a NEW session (no title yet, not already pending)
        // Skip for hidden prompts (e.g., fork seeds) - those are not "first user messages"
        // Use is_new_session_for_title (based on session ID presence) instead of turn_count
        // because restored sessions have turn_count == 0 but loaded history
        let should_generate_title = !hidden
            && is_new_session_for_title
            && self
                .state
                .tab_manager
                .session(tab_index)
                .is_some_and(|s| s.title.is_none() && !s.title_generation_pending);

        if should_generate_title {
            if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                let session_id = session.id;
                let workspace_id = session.workspace_id;

                // Get current branch from status_bar (most accurate source from git tracker)
                let current_branch = session
                    .status_bar
                    .branch_name()
                    .unwrap_or_default()
                    .to_string();

                // Mark as pending to prevent duplicate calls
                session.title_generation_pending = true;

                effects.push(Effect::GenerateTitleAndBranch {
                    session_id,
                    user_message: prompt_for_title.clone(),
                    working_dir: working_dir_for_title.clone(),
                    workspace_id,
                    current_branch,
                });
            }
        }

        Ok(effects)
    }

    fn handle_submit_action(&mut self, mode: QueuedMessageMode) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();
        let mut immediate_submit: Option<(String, Vec<PathBuf>, Vec<String>)> = None;
        let mut interrupt_before_submit = false;
        let mut prompt_fallback_id: Option<Uuid> = None;
        let mut footer_message: Option<String> = None;
        let mut shell_command: Option<(Uuid, usize, String, Option<PathBuf>)> = None;
        let mut shell_error: Option<String> = None;
        let mut queued_handled = false;

        // Extract config values before the mutable borrow
        let steer_behavior = self.config().steer.behavior;
        let steer_fallback = self.config().steer.fallback;

        {
            let Some(session) = self.state.tab_manager.active_session_mut() else {
                return Ok(effects);
            };

            if session.input_box.is_empty() {
                session.chat_view.scroll_to_bottom();
                return Ok(effects);
            }

            let submission = session.input_box.submit();
            if submission.text.trim().is_empty() && submission.image_paths.is_empty() {
                return Ok(effects);
            }

            let submission_text = submission.text;
            let submission_image_paths = submission.image_paths;
            let submission_image_placeholders = submission.image_placeholders;

            let handled_by_shell = session.input_box.is_shell_mode();
            if handled_by_shell {
                let command = submission_text.trim().to_string();
                if command.is_empty() {
                    shell_error = Some("Shell command is empty".to_string());
                } else {
                    let args = serde_json::json!({ "command": command }).to_string();
                    session.chat_view.push(ChatMessage::tool_with_exit(
                        "Bash",
                        args,
                        "Running...".to_string(),
                        None,
                    ));
                    let message_index = session.chat_view.len().saturating_sub(1);
                    session.input_box.set_shell_mode(false);
                    session.update_status();
                    shell_command = Some((
                        session.id,
                        message_index,
                        command,
                        session.working_dir.clone(),
                    ));
                }
                queued_handled = true;
            }

            if !queued_handled {
                let effective_mode = if mode == QueuedMessageMode::Steer
                    && steer_behavior == crate::config::SteerBehavior::Soft
                {
                    QueuedMessageMode::FollowUp
                } else {
                    mode
                };

                if session.is_processing {
                    let images = submission_image_paths
                        .iter()
                        .cloned()
                        .zip(submission_image_placeholders.iter().cloned())
                        .map(|(path, placeholder)| QueuedImageAttachment { path, placeholder })
                        .collect::<Vec<_>>();
                    let queued = QueuedMessage {
                        id: Uuid::new_v4(),
                        mode: effective_mode,
                        text: submission_text.clone(),
                        images,
                        created_at: Utc::now(),
                    };

                    if mode == QueuedMessageMode::Steer
                        && effective_mode == QueuedMessageMode::Steer
                    {
                        match steer_fallback {
                            crate::config::SteerFallback::Interrupt => {
                                let (text, image_paths, image_placeholders) =
                                    app_queue::queued_to_submission(&queued);
                                immediate_submit = Some((text, image_paths, image_placeholders));
                                interrupt_before_submit = true;
                                queued_handled = true;
                            }
                            crate::config::SteerFallback::Prompt => {
                                session.queue_message(queued.clone());
                                prompt_fallback_id = Some(queued.id);
                                footer_message = Some(
                                    "Steering queued · press Enter to confirm interrupt"
                                        .to_string(),
                                );
                                queued_handled = true;
                            }
                            crate::config::SteerFallback::Queue => {
                                session.queue_message(queued);
                                footer_message = Some("Steering queued".to_string());
                                queued_handled = true;
                            }
                        }
                    } else {
                        session.queue_message(queued);
                        footer_message = Some(if mode == QueuedMessageMode::Steer {
                            "Steering queued (soft mode)".to_string()
                        } else {
                            "Message queued".to_string()
                        });
                        queued_handled = true;
                    }
                }

                if !queued_handled {
                    immediate_submit = Some((
                        submission_text,
                        submission_image_paths,
                        submission_image_placeholders,
                    ));
                }
            }
        }

        if let Some(message) = shell_error {
            self.state
                .set_timed_footer_message(message, Duration::from_secs(3));
            return Ok(effects);
        }

        if let Some((session_id, message_index, command, working_dir)) = shell_command {
            effects.push(Effect::RunShellCommand {
                session_id,
                message_index,
                command,
                working_dir,
            });
            return Ok(effects);
        }

        if let Some(message) = footer_message {
            self.state
                .set_timed_footer_message(message, Duration::from_secs(3));
        }

        if let Some(message_id) = prompt_fallback_id {
            self.show_steer_fallback_prompt(message_id);
            return Ok(effects);
        }

        if let Some((text, images, placeholders)) = immediate_submit {
            if interrupt_before_submit {
                self.interrupt_agent();
            }
            effects.extend(self.submit_prompt(text, images, placeholders)?);
        }

        Ok(effects)
    }

    fn strip_image_placeholders(prompt: String, placeholders: &[String]) -> String {
        if placeholders.is_empty() {
            return prompt;
        }

        let mut cleaned = prompt;
        for placeholder in placeholders {
            cleaned = cleaned.replace(placeholder, "");
        }

        cleaned.trim().to_string()
    }

    fn plan_prompt_inline_enabled() -> bool {
        env::var(PLAN_MODE_INLINE_REMINDER_ENV)
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }

    fn plan_file_prompt_info(session: &AgentSession) -> (String, bool) {
        if let Some(path) = Self::read_plan_file_path_for_session(session) {
            return (path, true);
        }

        let path = if let Some(ref working_dir) = session.working_dir {
            working_dir.join(".claude").join("plans").join("plan.md")
        } else if let Some(home_dir) = dirs::home_dir() {
            home_dir.join(".claude").join("plans").join("plan.md")
        } else {
            PathBuf::from(".claude").join("plans").join("plan.md")
        };

        (path.display().to_string(), false)
    }

    fn take_mode_prompt(
        session: &mut AgentSession,
        use_inline_plan_prompt: bool,
    ) -> Option<String> {
        if !session.capabilities.supports_plan_mode {
            return None;
        }

        match session.agent_mode {
            AgentMode::Plan => {
                if session.last_mode_prompt == Some(AgentMode::Plan) {
                    return None;
                }
                let prompt = if use_inline_plan_prompt {
                    let (plan_path, exists) = Self::plan_file_prompt_info(session);
                    app_prompt::build_plan_mode_prompt_inline(&plan_path, exists)
                } else {
                    app_prompt::plan_mode_prompt_default().to_string()
                };
                session.last_mode_prompt = Some(AgentMode::Plan);
                Some(prompt)
            }
            AgentMode::Build => {
                if session.last_mode_prompt == Some(AgentMode::Plan) {
                    session.last_mode_prompt = Some(AgentMode::Build);
                    Some(app_prompt::build_switch_prompt().to_string())
                } else {
                    None
                }
            }
        }
    }

    fn prepend_mode_prompt(mode_prompt: &str, prompt: &str) -> String {
        if prompt.trim().is_empty() {
            mode_prompt.to_string()
        } else {
            format!("{mode_prompt}\n\n{prompt}")
        }
    }

    fn resolve_external_editor(&self) -> Option<Vec<String>> {
        let editor = env::var("VISUAL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                env::var("EDITOR")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })?;

        let parts: Vec<String> = editor
            .split_whitespace()
            .map(|part| part.to_string())
            .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts)
        }
    }

    fn reinitialize_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        terminal.clear()?;
        Ok(())
    }

    fn edit_prompt_external(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<()> {
        if self.state.input_mode != InputMode::Normal {
            self.state.set_timed_footer_message(
                "External editor only works in chat input".to_string(),
                Duration::from_secs(3),
            );
            return Ok(());
        }

        let editor_parts = match self.resolve_external_editor() {
            Some(parts) => parts,
            None => {
                self.state.set_timed_footer_message(
                    "Set $VISUAL or $EDITOR to use external editor".to_string(),
                    Duration::from_secs(3),
                );
                return Ok(());
            }
        };

        let (expanded_input, attachments) = {
            let Some(session) = self.state.tab_manager.active_session_mut() else {
                return Ok(());
            };
            (
                session.input_box.expanded_input(),
                session.input_box.attachments_snapshot(),
            )
        };

        let temp = Builder::new()
            .prefix("conduit-prompt-")
            .suffix(".txt")
            .tempfile()?;
        std::fs::write(temp.path(), expanded_input)?;

        guard.cleanup_for_suspend()?;

        let status = {
            let mut parts = editor_parts.into_iter();
            let command = match parts.next() {
                Some(cmd) => cmd,
                None => {
                    self.reinitialize_terminal(terminal)?;
                    self.state.set_timed_footer_message(
                        "External editor is not configured".to_string(),
                        Duration::from_secs(3),
                    );
                    return Ok(());
                }
            };
            let args: Vec<String> = parts.collect();
            Command::new(command).args(args).arg(temp.path()).status()
        };

        self.reinitialize_terminal(terminal)?;

        let status = status?;

        if !status.success() {
            self.state.set_timed_footer_message(
                "External editor cancelled".to_string(),
                Duration::from_secs(3),
            );
            return Ok(());
        }

        let edited = std::fs::read_to_string(temp.path())?;
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session
                .input_box
                .set_input_with_attachments(edited, attachments);
            session.input_box.move_end();
        }

        Ok(())
    }

    #[cfg(unix)]
    fn suspend_app(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<()> {
        guard.cleanup_for_suspend()?;
        let result = unsafe { libc::raise(libc::SIGTSTP) };
        if result == -1 {
            let err = io::Error::last_os_error();
            self.reinitialize_terminal(terminal)?;
            return Err(anyhow!("SIGTSTP failed: {}", err));
        }
        self.reinitialize_terminal(terminal)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn suspend_app(
        &mut self,
        _terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        _guard: &mut TerminalGuard,
    ) -> anyhow::Result<()> {
        self.state.set_timed_footer_message(
            "Suspend is not supported on this platform".to_string(),
            Duration::from_secs(3),
        );
        Ok(())
    }

    /// Initiate handoff flow - capture source context and open model selection.
    fn initiate_handoff_session(&mut self) {
        let (
            source_agent_type,
            source_agent_mode,
            source_reasoning_effort,
            workspace_id,
            working_dir,
            project_name,
            workspace_name,
            pr_number,
            handoff_prompt,
        ) = {
            let Some(session) = self.state.tab_manager.active_session() else {
                self.state.set_timed_footer_message(
                    "No active session to hand off".to_string(),
                    Duration::from_secs(3),
                );
                return;
            };

            if session.is_processing {
                self.show_error("Cannot Handoff", "Wait for the current response to finish.");
                return;
            }

            (
                session.agent_type,
                session.agent_mode,
                session.reasoning_effort,
                session.workspace_id,
                session.working_dir.clone(),
                session.project_name.clone(),
                session.workspace_name.clone(),
                session.pr_number,
                app_prompt::build_handoff_prompt(session.chat_view.messages()),
            )
        };

        self.state.close_overlays();
        self.state.pending_handoff_request = Some(PendingHandoffRequest {
            source_agent_type,
            agent_mode: source_agent_mode,
            reasoning_effort: source_reasoning_effort,
            workspace_id,
            working_dir,
            project_name,
            workspace_name,
            pr_number,
            handoff_prompt: Arc::from(handoff_prompt),
        });

        let mut allowed = self.config().effective_enabled_providers(self.tools());
        if !allowed.contains(&source_agent_type) {
            let tool = Self::required_tool(source_agent_type);
            if self.tools().is_available(tool) {
                allowed.push(source_agent_type);
            }
        }

        if allowed.is_empty() {
            self.state.pending_handoff_request = None;
            self.state.set_timed_footer_message(
                "No enabled providers available. Use /providers.".to_string(),
                Duration::from_secs(4),
            );
            return;
        }

        let target_provider = self.preferred_provider_for_handoff(source_agent_type);
        let target_model = self.config().default_model_for(target_provider);
        let defaults = self.model_selector_defaults();
        self.state
            .model_selector_state
            .set_allowed_providers(Some(allowed));
        self.state.model_selector_state.show_with_title(
            Some(target_model),
            defaults,
            "Handoff Target Model".to_string(),
        );
        self.state.model_picker_context = ModelPickerContext::HandoffSelection;
        self.state.input_mode = InputMode::SelectingModel;
    }

    /// Execute handoff after selecting target provider.
    fn execute_handoff_session(
        &mut self,
        target_agent: AgentType,
        target_model: String,
    ) -> anyhow::Result<Vec<Effect>> {
        let Some(pending) = self.state.pending_handoff_request.clone() else {
            return Err(anyhow!("No pending handoff request."));
        };

        let target_provider = if self
            .config()
            .is_provider_enabled_effective(target_agent, self.tools())
        {
            target_agent
        } else {
            self.preferred_provider_for_new_sessions()
                .unwrap_or(target_agent)
        };
        let target_model = if target_provider == target_agent {
            target_model
        } else {
            self.config().default_model_for(target_provider)
        };

        // Keep track of where we came from so we can recover cleanly on failure.
        let prev_index = self.state.tab_manager.active_index();
        let prev_sidebar_visible = self.state.sidebar_state.visible;
        let prev_sidebar_focused = self.state.sidebar_state.focused;
        let prev_input_mode = self.state.input_mode;
        let prev_tree_selected = self.state.sidebar_state.tree_state.selected;

        let mut session = if let Some(dir) = pending.working_dir.clone() {
            AgentSession::with_working_dir(target_provider, dir)
        } else {
            AgentSession::new(target_provider)
        };
        session.workspace_id = pending.workspace_id;
        session.project_name = pending.project_name.clone();
        session.workspace_name = pending.workspace_name.clone();
        session.pr_number = pending.pr_number;
        session.model = Some(target_model);
        session.init_context_for_model();
        session.model_invalid = false;
        session.agent_mode = if session.capabilities.supports_plan_mode {
            pending.agent_mode
        } else {
            AgentMode::Build
        };
        session.reasoning_effort = if Self::reasoning_supported(target_provider) {
            pending.reasoning_effort
        } else {
            None
        };
        session.suppress_next_assistant_reply = true;
        session.suppress_next_turn_summary = true;
        session.update_status();

        let Some(new_index) = self.state.tab_manager.add_session(session) else {
            self.state.pending_handoff_request = None;
            return Err(anyhow!("Maximum number of tabs reached."));
        };

        self.state.tab_manager.switch_to(new_index);
        self.sync_footer_spinner();
        self.state.sidebar_state.hide();
        self.state.input_mode = InputMode::Normal;

        let rollback = |app: &mut Self| {
            app.close_tab_at_index(new_index);
            let fallback = prev_index.min(app.state.tab_manager.len().saturating_sub(1));
            app.state.tab_manager.switch_to(fallback);
            if prev_sidebar_visible {
                app.state.sidebar_state.show();
            } else {
                app.state.sidebar_state.hide();
            }
            app.state.sidebar_state.set_focused(prev_sidebar_focused);
            app.state.input_mode = prev_input_mode;
            app.state.sidebar_state.tree_state.selected = prev_tree_selected;
            app.sync_footer_spinner();
        };

        let effects =
            match self.submit_prompt_hidden(pending.handoff_prompt.to_string(), vec![], vec![]) {
                Ok(effects) if effects.is_empty() => {
                    rollback(self);
                    self.state.pending_handoff_request = None;
                    return Err(anyhow!(
                        "Failed to start handoff session: no start-agent effect produced."
                    ));
                }
                Ok(effects) => effects,
                Err(err) => {
                    rollback(self);
                    self.state.pending_handoff_request = None;
                    return Err(err);
                }
            };

        if let Some(session) = self.state.tab_manager.session_mut(new_index) {
            let display = MessageDisplay::System {
                content: "Handoff context was injected. Waiting for your next prompt.".to_string(),
            };
            session.chat_view.push(display.to_chat_message());
        }

        self.state.pending_handoff_request = None;

        Ok(effects)
    }

    /// Initiate fork session flow - validate and show confirmation dialog
    fn initiate_fork_session(&mut self) {
        let Some(session) = self.state.tab_manager.active_session() else {
            return;
        };

        if session.is_processing {
            self.show_error("Cannot Fork", "Wait for the current response to finish.");
            return;
        }

        let parent_workspace_id = match session.workspace_id {
            Some(id) => id,
            None => {
                self.show_error(
                    "Cannot Fork",
                    "This session is not attached to a workspace.",
                );
                return;
            }
        };

        if self.fork_seed_dao().is_none() {
            self.show_error("Cannot Fork", "Fork metadata store unavailable.");
            return;
        }

        let seed_prompt = app_prompt::build_fork_seed_prompt(session.chat_view.messages());
        let model_id = session
            .model
            .clone()
            .unwrap_or_else(|| ModelRegistry::default_model(session.agent_type));
        let context_window =
            ContextWindowService::resolve(&self.core, session.agent_type, &model_id).tokens;
        let heuristic_tokens = Self::estimate_tokens(&seed_prompt);
        let observed_tokens = session.context_state.current_tokens.max(0);
        let token_estimate = heuristic_tokens.max(observed_tokens);

        self.state.pending_fork_request = Some(PendingForkRequest {
            agent_type: session.agent_type,
            agent_mode: session.agent_mode,
            model: session.model.clone(),
            reasoning_effort: session.reasoning_effort,
            parent_session_id: session
                .agent_session_id
                .as_ref()
                .map(|s| s.as_str().to_string()),
            parent_workspace_id,
            seed_prompt: Arc::from(seed_prompt),
            token_estimate,
            context_window,
            fork_seed_id: None,
        });

        self.show_blocking_confirmation_loading(
            "Fork Session",
            "Analyzing workspace state...",
            ConfirmationContext::ForkSessionPreflightInProgress {
                parent_workspace_id,
            },
        );

        let workspace_dao = self.workspace_dao_clone();
        let worktree_manager = self.worktree_manager().clone();
        self.spawn_blocking_preflight(
            move || {
                let workspace_dao =
                    workspace_dao.ok_or_else(|| "Workspace database unavailable".to_string())?;

                let workspace = workspace_dao
                    .get_by_id(parent_workspace_id)
                    .map_err(|e| format!("Failed to load workspace: {}", e))?
                    .ok_or_else(|| "Workspace not found.".to_string())?;

                let base_branch = worktree_manager
                    .get_current_branch(&workspace.path)
                    .unwrap_or_else(|_| workspace.branch.clone());

                let dirty_warning = match worktree_manager.get_branch_status(&workspace.path) {
                    Ok(status) if status.is_dirty => Some(
                        status
                            .dirty_description
                            .unwrap_or_else(|| "Uncommitted changes detected".to_string()),
                    ),
                    _ => None,
                };

                Ok(ForkSessionDialogPreflightResult {
                    base_branch,
                    dirty_warning,
                })
            },
            move |result| AppEvent::ForkSessionDialogPreflightCompleted {
                parent_workspace_id,
                result,
            },
            "fork_session_dialog_preflight_completed",
        );
    }

    /// Execute fork session after confirmation
    fn execute_fork_session(
        &mut self,
        parent_workspace_id: uuid::Uuid,
        base_branch: String,
    ) -> Option<Effect> {
        let Some(mut pending) = self.state.pending_fork_request.clone() else {
            self.show_error("Fork Failed", "No pending fork request.");
            return None;
        };

        if pending.parent_workspace_id != parent_workspace_id {
            self.show_error("Fork Failed", "Fork request does not match workspace.");
            self.state.pending_fork_request = None;
            return None;
        }

        let fork_seed_dao = match self.fork_seed_dao() {
            Some(dao) => dao,
            None => {
                self.show_error("Fork Failed", "Fork metadata store unavailable.");
                self.state.pending_fork_request = None;
                return None;
            }
        };

        let seed_prompt_hash = app_prompt::compute_seed_prompt_hash(&pending.seed_prompt);
        let fork_seed = ForkSeed::new(
            pending.agent_type,
            pending.parent_session_id.clone(),
            Some(pending.parent_workspace_id),
            seed_prompt_hash,
            None,
            pending.token_estimate,
            pending.context_window,
        );

        if let Err(e) = fork_seed_dao.create(&fork_seed) {
            self.show_error(
                "Fork Failed",
                &format!("Failed to save fork metadata: {}", e),
            );
            self.state.pending_fork_request = None;
            return None;
        }

        pending.fork_seed_id = Some(fork_seed.id);
        self.state.pending_fork_request = Some(pending);

        self.mark_workspace_busy(parent_workspace_id);
        Some(Effect::ForkWorkspace {
            parent_workspace_id,
            base_branch,
        })
    }

    fn finish_fork_session(&mut self, workspace_id: uuid::Uuid) -> anyhow::Result<Vec<Effect>> {
        let Some(pending) = self.state.pending_fork_request.clone() else {
            return Err(anyhow!("No pending fork data."));
        };

        let fork_seed_id = match pending.fork_seed_id {
            Some(id) => id,
            None => return Err(anyhow!("Fork metadata was not saved.")),
        };

        let workspace_dao = self
            .workspace_dao()
            .ok_or_else(|| anyhow!("Workspace database unavailable."))?;

        let repo_dao = self
            .repo_dao()
            .ok_or_else(|| anyhow!("Repository database unavailable."))?;

        let workspace = workspace_dao
            .get_by_id(workspace_id)
            .map_err(|e| anyhow!("Failed to load workspace: {}", e))?
            .ok_or_else(|| anyhow!("Workspace not found."))?;

        let project_name = repo_dao
            .get_by_id(workspace.repository_id)
            .ok()
            .flatten()
            .map(|repo| repo.name);

        // Keep track of where we came from so we can recover cleanly on failure
        let prev_index = self.state.tab_manager.active_index();
        let prev_sidebar_visible = self.state.sidebar_state.visible;
        let prev_input_mode = self.state.input_mode;
        let prev_tree_selected = self.state.sidebar_state.tree_state.selected;

        let mut session =
            AgentSession::with_working_dir(pending.agent_type, workspace.path.clone());
        session.workspace_id = Some(workspace_id);
        session.project_name = project_name;
        session.workspace_name = Some(workspace.name.clone());
        session.model = pending.model.clone();
        session.reasoning_effort = pending.reasoning_effort;
        session.model_invalid = false;
        session.agent_mode = pending.agent_mode;
        session.fork_seed_id = Some(fork_seed_id);
        session.suppress_next_assistant_reply = true;
        session.suppress_next_turn_summary = true;
        session.update_status();

        let new_index = self
            .state
            .tab_manager
            .add_session(session)
            .ok_or_else(|| anyhow!("Maximum number of tabs reached."))?;

        self.state.tab_manager.switch_to(new_index);
        self.sync_footer_spinner();

        if let Some(ref tracker) = self.git_tracker {
            tracker.track_workspace(workspace_id, workspace.path.clone());
        }

        self.state.sidebar_state.hide();
        self.state.input_mode = InputMode::Normal;

        // Note: suppress flags already set on session before add_session, no need to set again

        // Use submit_prompt_hidden - don't add 500KB seed to chat transcript
        let effects =
            match self.submit_prompt_hidden(pending.seed_prompt.to_string(), vec![], vec![]) {
                Ok(effects) if effects.is_empty() => {
                    // Remove the broken tab and untrack workspace
                    if let Some(ref tracker) = self.git_tracker {
                        tracker.untrack_workspace(workspace_id);
                    }
                    self.close_tab_at_index(new_index);
                    let fallback = prev_index.min(self.state.tab_manager.len().saturating_sub(1));
                    self.state.tab_manager.switch_to(fallback);
                    // Restore pre-fork UI state
                    if prev_sidebar_visible {
                        self.state.sidebar_state.show();
                    }
                    self.state.input_mode = prev_input_mode;
                    self.state.sidebar_state.tree_state.selected = prev_tree_selected;
                    return Err(anyhow!(
                        "Failed to start forked agent: no start-agent effect produced."
                    ));
                }
                Ok(effects) => effects,
                Err(e) => {
                    // Remove the broken tab and untrack workspace
                    if let Some(ref tracker) = self.git_tracker {
                        tracker.untrack_workspace(workspace_id);
                    }
                    self.close_tab_at_index(new_index);
                    let fallback = prev_index.min(self.state.tab_manager.len().saturating_sub(1));
                    self.state.tab_manager.switch_to(fallback);
                    // Restore pre-fork UI state
                    if prev_sidebar_visible {
                        self.state.sidebar_state.show();
                    }
                    self.state.input_mode = prev_input_mode;
                    self.state.sidebar_state.tree_state.selected = prev_tree_selected;
                    return Err(e);
                }
            };

        self.state.pending_fork_request = None;

        Ok(effects)
    }

    /// Attempt to clean up a fork workspace after finish_fork_session fails.
    /// Returns Some(error_message) if cleanup failed or partial cleanup occurred,
    /// None only if all cleanup operations succeeded.
    fn cleanup_fork_workspace(
        &mut self,
        workspace_id: uuid::Uuid,
        repo_id: uuid::Uuid,
    ) -> Option<String> {
        // Untrack workspace from git tracker first (must happen even on early returns)
        if let Some(ref tracker) = self.git_tracker {
            tracker.untrack_workspace(workspace_id);
        }

        let workspace_dao = self.workspace_dao()?;
        let repo_dao = self.repo_dao()?;

        // Safety: only allow deletion of paths under the managed workspaces directory
        let managed_root = crate::util::workspaces_dir();

        // Get workspace and repo info for worktree cleanup
        let workspace = match workspace_dao.get_by_id(workspace_id) {
            Ok(Some(ws)) => ws,
            Ok(None) => return None, // Already gone
            Err(e) => return Some(format!("Failed to load workspace: {}", e)),
        };

        // Check if workspace path is under managed root using canonicalization (security guard)
        // This prevents path traversal attacks like /managed/root/../../../etc
        let path_is_managed = match (
            std::fs::canonicalize(&managed_root),
            std::fs::canonicalize(&workspace.path),
        ) {
            (Ok(canonical_root), Ok(canonical_path)) => canonical_path.starts_with(&canonical_root),
            (Err(e), _) => {
                tracing::warn!(
                    error = %e,
                    managed_root = %managed_root.display(),
                    "Cannot canonicalize managed root; refusing removal for safety"
                );
                false
            }
            (_, Err(e)) => {
                // Path doesn't exist or can't be canonicalized - may already be deleted
                // Log but don't treat as managed (safe default)
                tracing::debug!(
                    error = %e,
                    path = %workspace.path.display(),
                    "Cannot canonicalize workspace path; may already be deleted"
                );
                // Try to prune stale worktree metadata since the path may have been deleted
                if let Ok(Some(repo)) = repo_dao.get_by_id(workspace.repository_id) {
                    if let Some(base_path) = &repo.base_path {
                        let repo_settings = resolve_repo_workspace_settings(self.config(), &repo);
                        if let Err(prune_err) = self
                            .worktree_manager()
                            .prune_workspaces(repo_settings.mode, base_path)
                        {
                            tracing::debug!(
                                error = %prune_err,
                                "Failed to prune stale worktrees"
                            );
                        }
                    }
                }
                false
            }
        };

        let repo = match repo_dao.get_by_id(repo_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                // Repo not found; try best-effort directory removal then delete from DB
                if path_is_managed {
                    if let Err(e) = std::fs::remove_dir_all(&workspace.path) {
                        tracing::warn!(
                            error = %e,
                            workspace_id = %workspace_id,
                            "Best-effort workspace directory removal failed (repo not found)"
                        );
                    }
                } else {
                    tracing::warn!(
                        workspace_id = %workspace_id,
                        path = %workspace.path.display(),
                        managed_root = %managed_root.display(),
                        "Refusing to remove non-managed workspace path (repo not found)"
                    );
                }
                if let Err(e) = workspace_dao.delete(workspace_id) {
                    return Some(format!("Failed to delete workspace from database: {}", e));
                }
                self.refresh_sidebar_data();
                return None;
            }
            Err(e) => {
                // Repo load failed; try best-effort directory removal then delete from DB
                if path_is_managed {
                    if let Err(fs_err) = std::fs::remove_dir_all(&workspace.path) {
                        tracing::warn!(
                            error = %fs_err,
                            workspace_id = %workspace_id,
                            "Best-effort workspace directory removal failed (repo load error)"
                        );
                    }
                } else {
                    tracing::warn!(
                        workspace_id = %workspace_id,
                        path = %workspace.path.display(),
                        managed_root = %managed_root.display(),
                        "Refusing to remove non-managed workspace path (repo load error)"
                    );
                }
                if let Err(db_err) = workspace_dao.delete(workspace_id) {
                    return Some(format!(
                        "Failed to load repository: {}; also failed to delete workspace from database: {}",
                        e, db_err
                    ));
                }
                self.refresh_sidebar_data();
                return Some(format!(
                    "Failed to load repository: {} (workspace deleted from DB)",
                    e
                ));
            }
        };
        let settings = resolve_repo_workspace_settings(self.config(), &repo);

        // Collect cleanup warnings for resources that may need manual cleanup
        let mut cleanup_warnings: Vec<String> = Vec::new();

        // Try to remove the worktree first (only if path is under managed root)
        if let Some(base_path) = &repo.base_path {
            if !path_is_managed {
                tracing::warn!(
                    workspace_id = %workspace_id,
                    path = %workspace.path.display(),
                    managed_root = %managed_root.display(),
                    "Refusing to remove worktree: workspace path is outside managed directory"
                );
                cleanup_warnings.push(format!(
                    "Worktree at {} may need manual removal (outside managed directory)",
                    workspace.path.display()
                ));
            } else if let Err(e) =
                self.worktree_manager()
                    .remove_workspace(settings.mode, base_path, &workspace.path)
            {
                tracing::warn!(
                    error = %e,
                    workspace_id = %workspace_id,
                    "Failed to remove worktree during fork cleanup"
                );
                cleanup_warnings.push(format!(
                    "Worktree at {} may need manual removal",
                    workspace.path.display()
                ));
            }

            // Also try to delete the branch (only if we successfully managed the worktree path)
            if path_is_managed {
                if let Err(e) = self.worktree_manager().delete_branch(
                    settings.mode,
                    base_path,
                    &workspace.path,
                    &workspace.branch,
                ) {
                    tracing::warn!(
                        error = %e,
                        workspace_id = %workspace_id,
                        branch = %workspace.branch,
                        "Failed to delete branch during fork cleanup"
                    );
                    cleanup_warnings.push(format!(
                        "Branch '{}' may need manual deletion",
                        workspace.branch
                    ));
                }
            } else {
                cleanup_warnings.push(format!(
                    "Branch '{}' not auto-deleted (workspace path outside managed directory)",
                    workspace.branch
                ));
            }
        } else {
            // No base_path available; try best-effort directory removal
            if path_is_managed {
                if let Err(e) = std::fs::remove_dir_all(&workspace.path) {
                    tracing::warn!(
                        error = %e,
                        workspace_id = %workspace_id,
                        "Best-effort workspace directory removal failed (no base_path)"
                    );
                    cleanup_warnings.push(format!(
                        "Workspace at {} may need manual removal",
                        workspace.path.display()
                    ));
                }
            } else {
                tracing::warn!(
                    workspace_id = %workspace_id,
                    path = %workspace.path.display(),
                    managed_root = %managed_root.display(),
                    "Refusing to remove non-managed workspace path (no base_path)"
                );
                cleanup_warnings.push(format!(
                    "Workspace at {} may need manual removal (outside managed directory)",
                    workspace.path.display()
                ));
            }
            // Note: Can't delete branch without base_path
            cleanup_warnings.push(format!(
                "Branch '{}' may need manual deletion (no repo base path)",
                workspace.branch
            ));
        }

        // Delete workspace from database
        if let Err(e) = workspace_dao.delete(workspace_id) {
            return Some(format!("Failed to delete workspace from database: {}", e));
        }

        self.refresh_sidebar_data();

        // Return cleanup warnings if any resources may need manual cleanup
        if cleanup_warnings.is_empty() {
            None
        } else {
            Some(format!("Partial cleanup: {}", cleanup_warnings.join("; ")))
        }
    }

    /// Handle the result of the PR preflight check
    fn handle_pr_preflight_result(
        &mut self,
        tab_index: usize,
        working_dir: std::path::PathBuf,
        preflight: crate::git::PrPreflightResult,
    ) -> Vec<Effect> {
        let effects = Vec::new();
        let mut sidebar_pr_update: Option<(Uuid, PrStatus)> = None;
        let mut sidebar_pr_clear: Option<Uuid> = None;
        // Tab indices may shift while preflight runs; only trust tab_index if it still matches.
        let mut initiating_session_id = self
            .state
            .tab_manager
            .session(tab_index)
            .and_then(|session| {
                let still_same_dir = session
                    .working_dir
                    .as_ref()
                    .is_some_and(|dir| dir == &working_dir);
                still_same_dir.then_some(session.id)
            })
            // Fallback: resolve by working_dir (more stable than tab index).
            .or_else(|| {
                self.state
                    .tab_manager
                    .sessions()
                    .iter()
                    .find(|session| {
                        session
                            .working_dir
                            .as_ref()
                            .is_some_and(|dir| dir == &working_dir)
                    })
                    .map(|session| session.id)
            });
        let preflight_workspace_id = initiating_session_id.and_then(|id| {
            self.state
                .tab_manager
                .sessions()
                .iter()
                .find(|session| session.id == id)
                .and_then(|session| session.workspace_id)
        });
        // Handle blocking errors
        if !preflight.gh_installed {
            self.state.confirmation_dialog_state.hide();
            // Show missing tool dialog with context about PR creation
            self.state.close_overlays();
            self.state.missing_tool_dialog_state.show_with_context(
                crate::util::Tool::Gh,
                "GitHub CLI (gh) is required for PR operations.",
            );
            self.state.input_mode = crate::ui::events::InputMode::MissingTool;
            return effects;
        }

        if !preflight.gh_authenticated {
            self.state.confirmation_dialog_state.hide();
            self.show_error_with_details(
                "Not Authenticated",
                "GitHub CLI is not authenticated.",
                "Run: gh auth login",
            );
            return effects;
        }

        if preflight.on_main_branch {
            self.state.confirmation_dialog_state.hide();
            self.show_error(
                "Cannot Create PR",
                &format!(
                    "You're on the '{}' branch. Create a feature branch first.",
                    preflight.branch_name
                ),
            );
            return effects;
        }

        // If we explicitly determined no PR exists, clear any stale PR UI state.
        if matches!(preflight.existing_pr.as_ref(), Some(pr) if !pr.exists) {
            if let Some(workspace_id) = preflight_workspace_id {
                for session in self.state.tab_manager.sessions_mut() {
                    if session.workspace_id == Some(workspace_id) {
                        session.pr_number = None;
                        session.status_bar.set_pr_status(None);
                    }
                }
                sidebar_pr_clear = Some(workspace_id);
            } else if let Some(session_id) = initiating_session_id.take() {
                if let Some(session) = self.state.tab_manager.session_by_id_mut(session_id) {
                    session.pr_number = None;
                    session.status_bar.set_pr_status(None);
                }
            }
        }

        // If PR exists, show confirmation dialog to open in browser
        if let Some(ref pr) = preflight.existing_pr {
            if pr.exists {
                // Update session's pr_number
                if let Some(workspace_id) = preflight_workspace_id {
                    let status = pr.clone();
                    for session in self.state.tab_manager.sessions_mut() {
                        if session.workspace_id == Some(workspace_id) {
                            Self::apply_pr_status_to_session(session, status.clone());
                        }
                    }
                    sidebar_pr_update = Some((workspace_id, status));
                } else if let Some(session_id) = initiating_session_id.take() {
                    if let Some(session) = self.state.tab_manager.session_by_id_mut(session_id) {
                        let status = pr.clone();
                        Self::apply_pr_status_to_session(session, status);
                    }
                }

                let pr_url = pr.url.clone().unwrap_or_else(|| "Unknown URL".to_string());
                self.state.close_overlays();
                self.state.confirmation_dialog_state.show(
                    "Pull Request Exists",
                    format!(
                        "PR #{} already exists for branch '{}'.\n\nOpen in browser?",
                        pr.number.unwrap_or(0),
                        preflight.branch_name
                    ),
                    vec![],
                    ConfirmationType::Info,
                    "Open PR",
                    Some(ConfirmationContext::OpenExistingPr {
                        working_dir,
                        pr_url,
                    }),
                );
                if let Some((workspace_id, status)) = sidebar_pr_update {
                    self.state
                        .sidebar_data
                        .update_workspace_pr_status(workspace_id, Some(status));
                }
                // Already in Confirming mode
                return effects;
            }
        }

        if let Some(workspace_id) = sidebar_pr_clear {
            self.state
                .sidebar_data
                .clear_workspace_pr_status(workspace_id);
        }

        // Build warnings for confirmation dialog
        let mut warnings = Vec::new();
        if preflight.uncommitted_count > 0 {
            warnings.push(format!(
                "{} file(s) will be auto-committed",
                preflight.uncommitted_count
            ));
        }
        if !preflight.has_upstream {
            warnings.push("Branch will be pushed to remote".to_string());
        }

        // Show confirmation dialog (replace loading state)
        self.state.close_overlays();
        self.state.confirmation_dialog_state.show(
            "Create Pull Request",
            format!(
                "Branch: {}\nTarget: {}",
                preflight.branch_name, preflight.target_branch
            ),
            warnings,
            ConfirmationType::Info,
            "Create PR",
            Some(ConfirmationContext::CreatePullRequest {
                tab_index,
                working_dir,
                preflight,
            }),
        );
        // Already in Confirming mode
        effects
    }

    /// Submit the PR workflow prompt to the current chat
    fn submit_pr_workflow(
        &mut self,
        tab_index: usize,
        working_dir: std::path::PathBuf,
        preflight: crate::git::PrPreflightResult,
    ) -> anyhow::Result<Vec<Effect>> {
        let target_tab_index = self
            .state
            .tab_manager
            .session(tab_index)
            .and_then(|session| {
                let matches_dir = session
                    .working_dir
                    .as_ref()
                    .is_some_and(|dir| dir == &working_dir);
                matches_dir.then_some(tab_index)
            })
            .or_else(|| {
                self.state
                    .tab_manager
                    .sessions()
                    .iter()
                    .position(|session| {
                        session
                            .working_dir
                            .as_ref()
                            .is_some_and(|dir| dir == &working_dir)
                    })
            });
        // Generate prompt for PR creation
        let prompt = PrManager::generate_pr_prompt(&preflight);

        let Some(target_tab_index) = target_tab_index else {
            self.show_error(
                "Cannot Create PR",
                "No session found for the PR preflight workspace.",
            );
            return Ok(Vec::new());
        };

        // Submit to the intended chat session
        self.submit_prompt_for_tab(
            target_tab_index,
            prompt,
            Vec::new(),
            Vec::new(),
            false,
            None,
        )
    }

    fn restore_queued_to_input(&mut self, message: crate::data::QueuedMessage) {
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            let attachments = message
                .images
                .iter()
                .map(|img| (img.path.clone(), img.placeholder.clone()))
                .collect();
            session
                .input_box
                .set_input_with_attachments(message.text, attachments);
            session.input_box.move_end();
        }
    }

    fn open_queue_editor(&mut self) {
        let has_queue = {
            let Some(session) = self.state.tab_manager.active_session_mut() else {
                return;
            };
            !session.queued_messages.is_empty()
        };

        if !has_queue {
            self.state
                .set_timed_footer_message("No queued messages".to_string(), Duration::from_secs(3));
            return;
        }

        self.state.close_overlays();
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            if session.queue_selection.is_none() {
                session.queue_selection = Some(session.queued_messages.len() - 1);
            }
        }
        self.state.input_mode = InputMode::QueueEditing;
    }

    fn close_queue_editor(&mut self) {
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.queue_selection = None;
        }
        self.state.input_mode = InputMode::Normal;
    }

    fn show_steer_fallback_prompt(&mut self, message_id: Uuid) {
        self.state.close_overlays();
        self.state.confirmation_dialog_state.show(
            "Interrupt to Steer",
            "Steering isn't supported by this harness.\nInterrupt the current run and send now?",
            vec![
                "In-flight tool execution will be stopped.".to_string(),
                "Queued message will be sent immediately.".to_string(),
            ],
            ConfirmationType::Warning,
            "Interrupt",
            Some(ConfirmationContext::SteerFallback { message_id }),
        );
        self.state.input_mode = InputMode::Confirming;
    }

    fn confirm_steer_fallback(&mut self, message_id: Uuid) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();
        let mut queued: Option<QueuedMessage> = None;

        {
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                if let Some(idx) = session
                    .queued_messages
                    .iter()
                    .position(|msg| msg.id == message_id)
                {
                    queued = session.remove_queue_at(idx);
                }
            }
        }

        if let Some(message) = queued {
            self.interrupt_agent();
            let (text, images, placeholders) = app_queue::queued_to_submission(&message);
            effects.extend(self.submit_prompt(text, images, placeholders)?);
        } else {
            self.state.set_timed_footer_message(
                "Queued steering message not found".to_string(),
                Duration::from_secs(3),
            );
        }

        Ok(effects)
    }

    fn drain_queue_for_tab(&mut self, tab_index: usize) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();
        let mut queued: Vec<QueuedMessage> = Vec::new();
        let (queue_mode, queue_delivery) = (self.config().queue.mode, self.config().queue.delivery);

        {
            let Some(session) = self.state.tab_manager.session_mut(tab_index) else {
                return Ok(effects);
            };

            if session.queued_messages.is_empty() {
                return Ok(effects);
            }

            let mut remaining = Vec::new();
            match queue_mode {
                crate::config::QueueMode::OneAtATime => {
                    let idx = session
                        .queued_messages
                        .iter()
                        .position(|msg| msg.mode == QueuedMessageMode::Steer)
                        .unwrap_or(0);
                    for (pos, msg) in session.queued_messages.drain(..).enumerate() {
                        if pos == idx {
                            queued.push(msg);
                        } else {
                            remaining.push(msg);
                        }
                    }
                }
                crate::config::QueueMode::All => {
                    let mut steers = Vec::new();
                    let mut followups = Vec::new();
                    for msg in session.queued_messages.drain(..) {
                        if msg.mode == QueuedMessageMode::Steer {
                            steers.push(msg);
                        } else {
                            followups.push(msg);
                        }
                    }
                    queued.extend(steers);
                    queued.extend(followups);
                }
            }

            if queue_delivery == crate::config::QueueDelivery::Separate && queued.len() > 1 {
                let mut requeue = queued.split_off(1);
                requeue.extend(remaining);
                session.queued_messages = requeue;
            } else {
                session.queued_messages = remaining;
            }
            session.queue_selection = None;
            session.update_status();
        }

        if queued.is_empty() {
            return Ok(effects);
        }

        let (prompt, images, placeholders) =
            app_queue::build_queued_submission(&queued, queue_delivery);
        effects.extend(self.submit_prompt_for_tab(
            tab_index,
            prompt,
            images,
            placeholders,
            false,
            None,
        )?);

        Ok(effects)
    }

    fn draw(&mut self, f: &mut Frame) {
        let size = f.area();
        {
            use ratatui::style::Style;
            use ratatui::widgets::{Block, Widget};

            let background =
                Block::default().style(Style::default().bg(crate::ui::components::bg_base()));
            background.render(size, f.buffer_mut());
        }

        // Calculate sidebar width
        let sidebar_width = if self.state.sidebar_state.visible {
            30u16
        } else {
            0
        };

        // First, split horizontally for sidebar
        let (sidebar_area, right_area) = if sidebar_width > 0 {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(sidebar_width), Constraint::Min(20)])
                .split(size);
            (chunks[0], chunks[1])
        } else {
            // No sidebar - use full width
            (Rect::default(), size)
        };

        // Split right area vertically to reserve bottom row for footer
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // Content area (chat + status bar + gap)
                Constraint::Length(1), // Footer (only in content area)
            ])
            .split(right_area);

        let content_area = right_chunks[0];
        let footer_area = right_chunks[1];

        // Store sidebar area for mouse hit-testing
        self.state.sidebar_area = if self.state.sidebar_state.visible {
            Some(sidebar_area)
        } else {
            None
        };

        // Render sidebar if visible
        if self.state.sidebar_state.visible {
            let sidebar =
                Sidebar::new(&self.state.sidebar_data).with_spinner_frame(self.state.spinner_frame);
            ratatui::widgets::StatefulWidget::render(
                sidebar,
                sidebar_area,
                f.buffer_mut(),
                &mut self.state.sidebar_state,
            );
        }

        // Check if active tab is a file viewer - render it separately
        if self.state.tab_manager.active_is_file() {
            self.render_file_viewer_tab(content_area, footer_area, f);
        } else {
            match self.state.view_mode {
                ViewMode::Chat => {
                    // Handle empty state - no tabs open
                    if self.state.tab_manager.is_empty() {
                        use crate::ui::components::{text_muted, FooterContext};
                        use ratatui::style::Style;
                        use ratatui::text::{Line, Span};
                        use ratatui::widgets::{Paragraph, Widget};

                        // Layout with tab bar + content (footer is rendered in reserved footer_area)
                        let chunks = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Length(1), // Tab bar
                                Constraint::Min(5),    // Content area
                            ])
                            .split(content_area);

                        // Store areas for mouse hit-testing
                        self.state.tab_bar_area = Some(chunks[0]);
                        self.state.chat_area = None;
                        self.state.file_viewer_area = None;
                        self.state.raw_events_area = None;
                        self.state.input_area = None;
                        self.state.status_bar_area = None;
                        self.state.footer_area = Some(footer_area);

                        // Render tab bar
                        let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
                        self.ensure_tab_bar_scroll(chunks[0].width, tabs_focused);
                        let tab_bar = self.build_tab_bar(tabs_focused);
                        tab_bar.render(chunks[0], f.buffer_mut());

                        // Empty state message - different for first-time users vs returning users
                        let is_first_time = self.state.show_first_time_splash;

                        // Render animated logo with shine effect
                        let mut lines = self.state.logo_shine.render_logo_lines();
                        lines.push(Line::from(""));
                        lines.push(Line::from(""));
                        lines.push(Line::from(""));

                        if is_first_time {
                            // First-time user - simpler message
                            lines.push(Line::from(Span::styled(
                                "Add your first project with Ctrl+N",
                                Style::default().fg(text_muted()),
                            )));
                        } else {
                            // Returning user - full message
                            lines.push(Line::from(Span::styled(
                                "Add a new project with Ctrl+N",
                                Style::default().fg(text_muted()),
                            )));
                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                "- or -",
                                Style::default().fg(text_muted()),
                            )));
                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                "Select a project from the sidebar",
                                Style::default().fg(text_muted()),
                            )));
                        }

                        let paragraph =
                            Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);

                        // Center vertically in the content area (chunks[1])
                        let message_area = chunks[1];
                        // First-time: 7 logo + 3 blank + 1 message = 11 lines
                        // Returning: 7 logo + 3 blank + 5 message = 15 lines
                        let text_height = if is_first_time { 11u16 } else { 15u16 };
                        let vertical_offset = message_area.height.saturating_sub(text_height) / 2;
                        let centered_area = Rect {
                            x: message_area.x,
                            y: message_area.y + vertical_offset,
                            width: message_area.width,
                            height: text_height,
                        };

                        paragraph.render(centered_area, f.buffer_mut());

                        // Render dialogs over empty state
                        if self.state.base_dir_dialog_state.is_visible() {
                            let dialog = BaseDirDialog::new();
                            dialog.render(size, f.buffer_mut(), &self.state.base_dir_dialog_state);
                        } else if self.state.provider_selector_state.is_visible() {
                            let selector = ProviderSelector::new();
                            selector.render(
                                size,
                                f.buffer_mut(),
                                &self.state.provider_selector_state.dialog,
                            );
                        } else if self.state.project_picker_state.is_visible() {
                            let picker = ProjectPicker::new();
                            picker.render(size, f.buffer_mut(), &self.state.project_picker_state);
                        } else if self.state.add_repo_dialog_state.is_visible() {
                            let dialog = AddRepoDialog::new();
                            dialog.render(size, f.buffer_mut(), &self.state.add_repo_dialog_state);
                        } else if self.state.session_import_state.is_visible() {
                            let picker = SessionImportPicker::new();
                            picker.render(size, f.buffer_mut(), &self.state.session_import_state);
                        } else if self.state.model_selector_state.is_visible() {
                            self.state.model_selector_state.update_viewport(size);
                            let selector = ModelSelector::new();
                            selector.render(size, f.buffer_mut(), &self.state.model_selector_state);
                        } else if self.state.reasoning_selector_state.is_visible() {
                            let selector = ReasoningSelector::new();
                            selector.render(
                                size,
                                f.buffer_mut(),
                                &self.state.reasoning_selector_state,
                            );
                        } else if self.state.theme_picker_state.is_visible() {
                            self.render_theme_picker(size, f.buffer_mut());
                        }

                        // Draw agent selector dialog if needed
                        if self.state.agent_selector_state.is_visible() {
                            let selector = AgentSelector::new();
                            selector.render(size, f.buffer_mut(), &self.state.agent_selector_state);
                        }

                        // Draw confirmation dialog if open
                        if self.state.confirmation_dialog_state.visible {
                            use ratatui::widgets::Widget;
                            let dialog =
                                ConfirmationDialog::new(&self.state.confirmation_dialog_state);
                            dialog.render(size, f.buffer_mut());
                        }

                        // Draw error dialog if open
                        if self.state.error_dialog_state.visible {
                            use ratatui::widgets::Widget;
                            let dialog = ErrorDialog::new(&self.state.error_dialog_state);
                            dialog.render(size, f.buffer_mut());
                        }

                        // Draw missing tool dialog if open
                        if self.state.missing_tool_dialog_state.is_visible() {
                            use ratatui::widgets::Widget;
                            let dialog =
                                MissingToolDialog::new(&self.state.missing_tool_dialog_state);
                            dialog.render(size, f.buffer_mut());
                        }

                        // Draw help dialog if open
                        if self.state.help_dialog_state.is_visible() {
                            HelpDialog::new().render(
                                size,
                                f.buffer_mut(),
                                &mut self.state.help_dialog_state,
                            );
                        }

                        if self.state.input_mode == InputMode::SettingsMenu
                            || self.state.settings_menu_state.is_visible()
                        {
                            SettingsMenu::new().render(
                                size,
                                f.buffer_mut(),
                                &self.state.settings_menu_state,
                            );
                        }

                        // Draw command palette (on top of everything)
                        if self.state.command_palette_state.is_visible() {
                            CommandPalette::new().render(
                                size,
                                f.buffer_mut(),
                                &self.state.command_palette_state,
                            );
                        }

                        if self.state.input_mode == InputMode::WorkspaceDefaults
                            || self.state.workspace_defaults_dialog_state.is_visible()
                        {
                            WorkspaceDefaultsDialog::new().render(
                                size,
                                f.buffer_mut(),
                                &self.state.workspace_defaults_dialog_state,
                            );
                        }

                        // Draw footer for empty state (sidebar-aware)
                        let footer_context =
                            if self.state.input_mode == InputMode::SidebarNavigation {
                                FooterContext::Sidebar
                            } else {
                                FooterContext::Empty
                            };
                        let footer = GlobalFooter::for_context(footer_context)
                            .with_spinner(self.state.footer_spinner.as_ref())
                            .with_message(self.state.footer_message.as_deref());
                        footer.render(footer_area, f.buffer_mut());

                        return;
                    }

                    // Margins for input area (constants to avoid duplication)
                    const INPUT_MARGIN_LEFT: u16 = 4;
                    const INPUT_MARGIN_RIGHT: u16 = 4;
                    let input_total_margin = INPUT_MARGIN_LEFT + INPUT_MARGIN_RIGHT;

                    // Calculate dynamic input height (max 30% of screen)
                    // When inline prompt is active, set to 0 so chat area expands
                    let max_input_height = (content_area.height as f32 * 0.30).ceil() as u16;
                    let input_width = content_area.width.saturating_sub(input_total_margin);
                    let has_inline_prompt = self
                        .state
                        .tab_manager
                        .active_session()
                        .map(|s| s.inline_prompt.is_some())
                        .unwrap_or(false);

                    let input_height = if has_inline_prompt {
                        0 // No input box when inline prompt is active
                    } else if let Some(session) = self.state.tab_manager.active_session() {
                        session
                            .input_box
                            .desired_height(max_input_height, input_width)
                    } else {
                        3 // Minimum height
                    };

                    // When inline prompt is active, hide status bar and gap too
                    let status_bar_height = if has_inline_prompt { 0 } else { 1 };
                    let gap_height = if has_inline_prompt { 0 } else { 1 };

                    // Chat layout with session header, input box, status bar, and gap
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1),                 // Tab bar
                            Constraint::Length(1),                 // Session header
                            Constraint::Min(5),                    // Chat view
                            Constraint::Length(input_height),      // Input box (dynamic)
                            Constraint::Length(status_bar_height), // Status bar (hidden during inline prompt)
                            Constraint::Length(gap_height),        // Gap row before footer
                        ])
                        .split(content_area);

                    // Extract named areas to avoid brittle numeric indices
                    let tab_bar_chunk = chunks[0];
                    let header_chunk = chunks[1];
                    let chat_chunk = chunks[2];
                    let input_chunk = chunks[3];
                    let status_bar_chunk = chunks[4];
                    let gap_chunk = chunks[5];

                    // Create margin-adjusted areas for input, status bar, and gap rows
                    let input_area_inner = Rect {
                        x: input_chunk.x + INPUT_MARGIN_LEFT,
                        y: input_chunk.y,
                        width: input_chunk.width.saturating_sub(input_total_margin),
                        height: input_chunk.height,
                    };
                    let status_bar_area_inner = Rect {
                        x: status_bar_chunk.x + INPUT_MARGIN_LEFT,
                        y: status_bar_chunk.y,
                        width: status_bar_chunk.width.saturating_sub(input_total_margin),
                        height: status_bar_chunk.height,
                    };
                    let gap_area_inner = Rect {
                        x: gap_chunk.x + INPUT_MARGIN_LEFT,
                        y: gap_chunk.y,
                        width: gap_chunk.width.saturating_sub(input_total_margin),
                        height: gap_chunk.height,
                    };

                    // Fill margin areas so they match the app background.
                    let buf = f.buffer_mut();
                    let fill_margins = |buf: &mut ratatui::buffer::Buffer, row_area: Rect, bg| {
                        let style = ratatui::style::Style::default().bg(bg);
                        let left_width = INPUT_MARGIN_LEFT.min(row_area.width);
                        if left_width > 0 {
                            buf.set_style(
                                Rect {
                                    x: row_area.x,
                                    y: row_area.y,
                                    width: left_width,
                                    height: row_area.height,
                                },
                                style,
                            );
                        }
                        let right_width =
                            INPUT_MARGIN_RIGHT.min(row_area.width.saturating_sub(left_width));
                        if right_width > 0 {
                            let right_start =
                                row_area.x + row_area.width.saturating_sub(right_width);
                            buf.set_style(
                                Rect {
                                    x: right_start,
                                    y: row_area.y,
                                    width: right_width,
                                    height: row_area.height,
                                },
                                style,
                            );
                        }
                    };

                    use crate::ui::components::bg_base;
                    let margin_bg = bg_base();
                    fill_margins(buf, input_chunk, margin_bg);
                    fill_margins(buf, status_bar_chunk, margin_bg);
                    fill_margins(buf, gap_chunk, margin_bg);

                    // Draw separator line in the gap row (▀ characters)
                    // Foreground = status bar bg, background = base bg (creates rounded bottom edge)
                    // Skip when inline prompt is active (gap row is hidden)
                    if !has_inline_prompt {
                        use crate::ui::components::status_bar_bg;
                        for x in gap_area_inner.x..gap_area_inner.x + gap_area_inner.width {
                            buf[(x, gap_area_inner.y)]
                                .set_char('▀')
                                .set_fg(status_bar_bg());
                        }
                    }

                    // Store layout areas for mouse hit-testing
                    // Set hidden areas to None when inline prompt is active to avoid hit-testing confusion
                    self.state.tab_bar_area = Some(tab_bar_chunk);
                    self.state.chat_area = Some(chat_chunk);
                    self.state.file_viewer_area = None;
                    self.state.raw_events_area = None;
                    self.state.input_area = if has_inline_prompt {
                        None
                    } else {
                        Some(input_area_inner)
                    };
                    self.state.status_bar_area = if has_inline_prompt {
                        None
                    } else {
                        Some(status_bar_area_inner)
                    };
                    self.state.footer_area = Some(footer_area);

                    // Draw tab bar (unfocused when sidebar is focused)
                    let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
                    self.ensure_tab_bar_scroll(tab_bar_chunk.width, tabs_focused);
                    let tab_bar = self.build_tab_bar(tabs_focused);
                    tab_bar.render(tab_bar_chunk, f.buffer_mut());

                    // Draw session header (below tab bar)
                    let session_title = self
                        .state
                        .tab_manager
                        .active_session()
                        .and_then(|s| s.title.as_deref());
                    SessionHeader::new(session_title).render(header_chunk, f.buffer_mut());

                    // Draw active session components
                    let is_command_mode = self.state.input_mode == InputMode::Command;
                    let show_chat_scrollbar = self.config().ui.show_chat_scrollbar;
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        // Use full chat area - prompt is now rendered as part of scrollable content
                        let chat_area = chat_chunk;

                        self.state.chat_area = if chat_area.height == 0 {
                            None
                        } else {
                            Some(chat_area)
                        };

                        // Render chat with thinking indicator if processing (but not during inline prompt)
                        let thinking_line =
                            if session.is_processing && session.inline_prompt.is_none() {
                                Some(session.thinking_indicator.render())
                            } else {
                                None
                            };
                        let input_mode = self.state.input_mode;
                        let queue_lines =
                            app_queue::build_queue_lines(session, chat_area.width, input_mode);

                        // Build prompt lines from inline_prompt (renders as part of scrollable chat)
                        let prompt_lines = session
                            .inline_prompt
                            .as_ref()
                            .map(|p| p.render_as_lines(chat_area.width as usize));

                        session.chat_view.render_with_indicator(
                            chat_area,
                            f.buffer_mut(),
                            thinking_line,
                            queue_lines,
                            prompt_lines,
                            show_chat_scrollbar,
                        );

                        // Check if inline prompt is active
                        let has_inline_prompt = session.inline_prompt.is_some();

                        // Render input box (not in command mode, not when inline prompt active)
                        if !is_command_mode && !has_inline_prompt {
                            session.input_box.render(input_area_inner, f.buffer_mut());
                        }
                        // Update and render status bar (skip when inline prompt is active)
                        if !has_inline_prompt {
                            session.status_bar.set_metrics(
                                self.state.show_metrics,
                                self.state.metrics.draw_time,
                                self.state.metrics.event_time,
                                self.state.metrics.fps,
                                self.state.metrics.scroll_latency,
                                self.state.metrics.scroll_latency_avg,
                                self.state.metrics.scroll_lines_per_sec,
                                self.state.metrics.scroll_events_per_sec,
                                self.state.metrics.scroll_active,
                            );
                            session
                                .status_bar
                                .set_spinner_frame(self.state.spinner_frame);
                            session
                                .status_bar
                                .render(status_bar_area_inner, f.buffer_mut());
                        }

                        // Set cursor position (accounting for scroll)
                        if self.state.input_mode == InputMode::Normal {
                            // Inline prompt uses visual cursor (reversed style) in the rendered lines,
                            // so no cursor positioning needed. Only set cursor for normal input box.
                            if !has_inline_prompt {
                                let scroll_offset = session.input_box.scroll_offset();
                                let (cx, cy) = session
                                    .input_box
                                    .cursor_position(input_area_inner, scroll_offset);
                                f.set_cursor_position((cx, cy));
                            }
                        }
                    }

                    // Render command prompt if in command mode (outside session borrow)
                    if is_command_mode {
                        self.render_command_prompt(input_area_inner, f.buffer_mut());
                        // Cursor at end of command buffer (after prompt in padded area)
                        let prompt = format!("  cmd › {}", self.state.command_buffer);
                        let prompt_width = prompt.width() as u16;
                        let max_x = input_area_inner.x + input_area_inner.width.saturating_sub(1);
                        let cx = (input_area_inner.x + prompt_width).min(max_x);
                        let cy = input_area_inner.y + 1; // top padding
                        f.set_cursor_position((cx, cy));
                    }

                    if self.state.slash_menu_state.is_visible() && !has_inline_prompt {
                        self.render_slash_menu(chat_chunk, input_area_inner, f.buffer_mut());
                    }

                    // Draw footer (full width) - context-aware based on input mode
                    let footer = GlobalFooter::from_state(
                        self.state.view_mode,
                        self.state.input_mode,
                        !self.state.tab_manager.is_empty(),
                    )
                    .with_spinner(self.state.footer_spinner.as_ref())
                    .with_message(self.state.footer_message.as_deref());
                    footer.render(footer_area, f.buffer_mut());
                }
                ViewMode::RawEvents => {
                    // Raw events layout - no input box, full height for events
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1), // Tab bar
                            Constraint::Length(1), // Session header
                            Constraint::Min(5),    // Raw events view (full height)
                        ])
                        .split(content_area);

                    // Extract named areas to avoid brittle numeric indices
                    let tab_bar_chunk = chunks[0];
                    let header_chunk = chunks[1];
                    let raw_events_chunk = chunks[2];

                    // Store layout areas for mouse hit-testing (no input/status in this mode)
                    self.state.tab_bar_area = Some(tab_bar_chunk);
                    self.state.chat_area = None;
                    self.state.file_viewer_area = None;
                    self.state.raw_events_area = Some(raw_events_chunk);
                    self.state.input_area = None;
                    self.state.status_bar_area = None;
                    self.state.footer_area = Some(footer_area);

                    // Draw tab bar (unfocused when sidebar is focused)
                    let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
                    self.ensure_tab_bar_scroll(tab_bar_chunk.width, tabs_focused);
                    let tab_bar = self.build_tab_bar(tabs_focused);
                    tab_bar.render(tab_bar_chunk, f.buffer_mut());

                    // Draw session header (below tab bar) - consistent with Chat view
                    let session_title = self
                        .state
                        .tab_manager
                        .active_session()
                        .and_then(|s| s.title.as_deref());
                    SessionHeader::new(session_title).render(header_chunk, f.buffer_mut());

                    // Draw raw events view
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session
                            .raw_events_view
                            .render(raw_events_chunk, f.buffer_mut());
                    }

                    // Draw footer (full width) - context-aware based on input mode
                    let footer = GlobalFooter::from_state(
                        self.state.view_mode,
                        self.state.input_mode,
                        !self.state.tab_manager.is_empty(),
                    )
                    .with_spinner(self.state.footer_spinner.as_ref())
                    .with_message(self.state.footer_message.as_deref());
                    footer.render(footer_area, f.buffer_mut());
                }
            }
        } // end of else block for agent tab rendering

        // Draw agent selector dialog if needed
        if self.state.agent_selector_state.is_visible() {
            let selector = AgentSelector::new();
            selector.render(size, f.buffer_mut(), &self.state.agent_selector_state);
        }

        if self.state.provider_selector_state.is_visible() {
            let selector = ProviderSelector::new();
            selector.render(
                size,
                f.buffer_mut(),
                &self.state.provider_selector_state.dialog,
            );
        }

        // Draw add repository dialog if open
        if self.state.add_repo_dialog_state.is_visible() {
            let dialog = AddRepoDialog::new();
            dialog.render(size, f.buffer_mut(), &self.state.add_repo_dialog_state);
        }

        // Draw model selector dialog if open
        if self.state.model_selector_state.is_visible() {
            self.state.model_selector_state.update_viewport(size);
            let model_selector = ModelSelector::new();
            model_selector.render(size, f.buffer_mut(), &self.state.model_selector_state);
        }

        if self.state.reasoning_selector_state.is_visible() {
            let selector = ReasoningSelector::new();
            selector.render(size, f.buffer_mut(), &self.state.reasoning_selector_state);
        }

        // Draw theme picker dialog if open
        self.render_theme_picker(size, f.buffer_mut());

        // Draw base directory dialog if open
        if self.state.base_dir_dialog_state.is_visible() {
            let dialog = BaseDirDialog::new();
            dialog.render(size, f.buffer_mut(), &self.state.base_dir_dialog_state);
        }

        // Draw project picker if open
        if self.state.project_picker_state.is_visible() {
            let picker = ProjectPicker::new();
            picker.render(size, f.buffer_mut(), &self.state.project_picker_state);
        }

        // Draw session import picker if open
        if self.state.session_import_state.is_visible() {
            let picker = SessionImportPicker::new();
            picker.render(size, f.buffer_mut(), &self.state.session_import_state);
        }

        // Draw confirmation dialog if open
        if self.state.confirmation_dialog_state.visible {
            use ratatui::widgets::Widget;
            let dialog = ConfirmationDialog::new(&self.state.confirmation_dialog_state);
            dialog.render(size, f.buffer_mut());
        }

        // Draw error dialog (on top of everything except spinner)
        if self.state.error_dialog_state.visible {
            use ratatui::widgets::Widget;
            let dialog = ErrorDialog::new(&self.state.error_dialog_state);
            dialog.render(size, f.buffer_mut());
        }

        // Draw missing tool dialog (on top of everything except spinner)
        if self.state.missing_tool_dialog_state.is_visible() {
            use ratatui::widgets::Widget;
            let dialog = MissingToolDialog::new(&self.state.missing_tool_dialog_state);
            dialog.render(size, f.buffer_mut());
        }

        // Draw help dialog (on top of everything)
        if self.state.help_dialog_state.is_visible() {
            HelpDialog::new().render(size, f.buffer_mut(), &mut self.state.help_dialog_state);
        }

        if self.state.input_mode == InputMode::SettingsMenu
            || self.state.settings_menu_state.is_visible()
        {
            SettingsMenu::new().render(size, f.buffer_mut(), &self.state.settings_menu_state);
        }

        // Draw command palette (on top of everything)
        if self.state.command_palette_state.is_visible() {
            CommandPalette::new().render(size, f.buffer_mut(), &self.state.command_palette_state);
        }

        if self.state.input_mode == InputMode::WorkspaceDefaults
            || self.state.workspace_defaults_dialog_state.is_visible()
        {
            WorkspaceDefaultsDialog::new().render(
                size,
                f.buffer_mut(),
                &self.state.workspace_defaults_dialog_state,
            );
        }

        // Draw removing project spinner overlay
        if self.state.input_mode == InputMode::RemovingProject {
            use crate::ui::components::Spinner;
            use ratatui::layout::Alignment;
            use ratatui::style::{Color, Style};
            use ratatui::symbols::border;
            use ratatui::text::Line;
            use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

            let dialog_width: u16 = 30;
            let dialog_height: u16 = 3;

            // Center the dialog
            let x = size.width.saturating_sub(dialog_width) / 2;
            let y = size.height.saturating_sub(dialog_height) / 2;

            let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

            // Clear the area first
            Clear.render(dialog_area, f.buffer_mut());

            // Render dialog box with rounded corners
            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(Color::Rgb(130, 170, 255)));

            let inner = block.inner(dialog_area);
            block.render(dialog_area, f.buffer_mut());

            // Render spinner and message
            let spinner = Spinner::dots();
            let line = Line::from(vec![
                spinner.span(Color::Rgb(130, 170, 255)),
                ratatui::text::Span::raw(" Removing project..."),
            ]);

            let para = Paragraph::new(line).alignment(Alignment::Center);
            para.render(inner, f.buffer_mut());
        }
    }

    /// Render file viewer tab content
    fn render_file_viewer_tab(
        &mut self,
        content_area: Rect,
        footer_area: Rect,
        f: &mut ratatui::Frame<'_>,
    ) {
        use crate::ui::components::{
            bg_base, text_muted, text_primary, FileViewerView, FooterContext, GlobalFooter,
        };
        use ratatui::style::Style;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Widget};
        use unicode_width::UnicodeWidthStr;

        let is_command_mode = self.state.input_mode == InputMode::Command;

        // Layout: tab bar, file header, content (+ optional command prompt)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(if is_command_mode {
                vec![
                    Constraint::Length(1), // Tab bar
                    Constraint::Length(1), // File header
                    Constraint::Min(3),    // File content
                    Constraint::Length(3), // Command prompt
                ]
            } else {
                vec![
                    Constraint::Length(1), // Tab bar
                    Constraint::Length(1), // File header (path + line count)
                    Constraint::Min(5),    // File content
                ]
            })
            .split(content_area);

        let tab_bar_chunk = chunks[0];
        let header_chunk = chunks[1];
        let content_chunk = chunks[2];
        let command_chunk = if is_command_mode {
            Some(chunks[3])
        } else {
            None
        };

        // Store areas for mouse hit-testing
        self.state.tab_bar_area = Some(tab_bar_chunk);
        self.state.chat_area = None;
        self.state.file_viewer_area = Some(content_chunk);
        self.state.raw_events_area = None;
        self.state.input_area = command_chunk;
        self.state.status_bar_area = None;
        self.state.footer_area = Some(footer_area);

        // Render tab bar
        let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
        self.ensure_tab_bar_scroll(tab_bar_chunk.width, tabs_focused);
        let tab_bar = self.build_tab_bar(tabs_focused);
        tab_bar.render(tab_bar_chunk, f.buffer_mut());

        // Render file header and content
        if let Some(file_session) = self.state.tab_manager.active_file_viewer_mut() {
            let markdown_width = content_chunk.width.saturating_sub(1) as usize;
            file_session.ensure_render_cache(markdown_width);

            // Render file header with path and line count
            let path_str = file_session.file_path.display().to_string();
            let mode_label = format!(
                "{} • {}",
                file_session.file_kind_label(),
                file_session.view_mode_label()
            );
            let line_info = format!(
                " ({} lines • {})",
                file_session.effective_total_lines(),
                mode_label
            );

            // Truncate path if it doesn't fit in the header width (UTF-8 safe, width-aware)
            let available_width = header_chunk.width.saturating_sub(2) as usize; // 1 leading space + 1 safety
            let line_info_width = UnicodeWidthStr::width(line_info.as_str());
            let max_path_width = available_width.saturating_sub(line_info_width);

            let truncated_path = if UnicodeWidthStr::width(path_str.as_str()) > max_path_width {
                if max_path_width <= 3 {
                    // Not enough room for "..." + content
                    "...".chars().take(max_path_width).collect::<String>()
                } else {
                    // Build tail from right, respecting character boundaries and widths
                    let mut tail = String::new();
                    let mut width = 0usize;
                    let target = max_path_width.saturating_sub(3); // reserve for "..."
                    for ch in path_str.chars().rev() {
                        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                        if width + w > target {
                            break;
                        }
                        width += w;
                        tail.insert(0, ch);
                    }
                    format!("...{}", tail)
                }
            } else {
                path_str
            };

            let header_line = Line::from(vec![
                Span::styled(" ", Style::default().bg(bg_base())),
                Span::styled(
                    truncated_path,
                    Style::default().fg(text_primary()).bg(bg_base()),
                ),
                Span::styled(line_info, Style::default().fg(text_muted()).bg(bg_base())),
            ]);

            let header_para = Paragraph::new(header_line).style(Style::default().bg(bg_base()));
            header_para.render(header_chunk, f.buffer_mut());

            // Render file content with line numbers and scrollbar
            FileViewerView::new(file_session).render(content_chunk, f.buffer_mut());
        }

        // Render command prompt if in command mode
        if let Some(cmd_area) = command_chunk {
            self.render_command_prompt(cmd_area, f.buffer_mut());
            // Set cursor position for command input
            let prompt = format!("  cmd › {}", self.state.command_buffer);
            let prompt_width = UnicodeWidthStr::width(prompt.as_str()) as u16;
            let max_x = cmd_area.x + cmd_area.width.saturating_sub(1);
            let cx = (cmd_area.x + prompt_width).min(max_x);
            let cy = cmd_area.y + 1;
            f.set_cursor_position((cx, cy));
        }

        // Render footer (sidebar-aware)
        let footer_context = if self.state.input_mode == InputMode::SidebarNavigation {
            FooterContext::Sidebar
        } else {
            FooterContext::FileViewer
        };
        let footer = GlobalFooter::for_context(footer_context)
            .with_spinner(self.state.footer_spinner.as_ref())
            .with_message(self.state.footer_message.as_deref());
        footer.render(footer_area, f.buffer_mut());
    }

    fn render_theme_picker(&mut self, size: Rect, buf: &mut ratatui::buffer::Buffer) {
        if !self.state.theme_picker_state.is_visible() {
            return;
        }
        use ratatui::widgets::Widget;
        self.state.theme_picker_state.update_viewport(size);
        let picker = ThemePicker::new(&self.state.theme_picker_state);
        picker.render(size, buf);
    }

    /// Render command mode prompt
    fn render_command_prompt(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        use ratatui::style::Style;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Clear, Paragraph, Widget};
        use unicode_width::UnicodeWidthStr;

        Clear.render(area, buf);
        buf.set_style(area, Style::default().bg(crate::ui::components::input_bg()));

        if area.height < 3 || area.width == 0 {
            return;
        }

        let padding_top: u16 = 1;
        let padding_bottom: u16 = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return;
        }

        let prefix = "  cmd › ";
        let prefix_width = UnicodeWidthStr::width(prefix) as u16;
        let buffer_width = UnicodeWidthStr::width(self.state.command_buffer.as_str()) as u16;
        let total_width = prefix_width + buffer_width;
        let content_width = area.width;

        let line = if total_width > content_width {
            // Truncate from the left, showing most recent input
            let mut truncated = String::new();
            let mut width = 0usize;
            for ch in self.state.command_buffer.chars().rev() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if width + w > content_width.saturating_sub(prefix_width + 1) as usize {
                    break;
                }
                width += w;
                truncated.insert(0, ch);
            }
            Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(crate::ui::components::text_muted()),
                ),
                Span::raw("…"),
                Span::styled(
                    truncated,
                    Style::default().fg(crate::ui::components::text_primary()),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(crate::ui::components::text_muted()),
                ),
                Span::styled(
                    &self.state.command_buffer,
                    Style::default().fg(crate::ui::components::text_primary()),
                ),
            ])
        };

        let para =
            Paragraph::new(line).style(Style::default().bg(crate::ui::components::input_bg()));
        para.render(
            Rect {
                x: area.x,
                y: area.y + padding_top,
                width: content_width,
                height: content_height,
            },
            buf,
        );
    }

    fn render_slash_menu(
        &mut self,
        chat_area: Rect,
        input_area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        if !self.state.slash_menu_state.is_visible() {
            return;
        }

        let available_height = input_area.y.saturating_sub(chat_area.y);
        let list_height_max = available_height.saturating_sub(4);
        if list_height_max == 0 {
            return;
        }

        let list_len = self.state.slash_menu_state.filtered_len().max(1);
        let list_height = list_len.min(list_height_max as usize).max(1) as u16;
        self.state
            .slash_menu_state
            .set_max_visible(list_height as usize);

        let menu_height = list_height.saturating_add(4);
        let menu_y = input_area.y.saturating_sub(menu_height);
        let menu_area = Rect {
            x: input_area.x,
            y: menu_y,
            width: input_area.width,
            height: menu_height,
        };

        SlashMenu::new().render(menu_area, buf, &self.state.slash_menu_state);
    }

    fn find_latest_plan_file(session: &AgentSession) -> Option<std::path::PathBuf> {
        let mut candidates = Vec::new();
        if let Some(home_dir) = dirs::home_dir() {
            candidates.push(home_dir.join(".claude").join("plans"));
        }
        if let Some(ref working_dir) = session.working_dir {
            candidates.push(working_dir.join(".claude").join("plans"));
        }

        let mut newest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
        for plans_dir in candidates {
            if !plans_dir.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&plans_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "md") {
                        if let Ok(metadata) = path.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                if newest.as_ref().is_none_or(|(_, t)| modified > *t) {
                                    newest = Some((path, modified));
                                }
                            }
                        }
                    }
                }
            }
        }
        newest.map(|(path, _)| path)
    }

    /// Find the most recent plan file path for the session (for ExitPlanMode display)
    fn read_plan_file_path_for_session(session: &AgentSession) -> Option<String> {
        Self::find_latest_plan_file(session).map(|path| path.display().to_string())
    }

    /// Read the plan file for the current session (for ExitPlanMode display)
    fn read_plan_file_for_session(session: &AgentSession) -> (String, String) {
        if let Some(path) = Self::find_latest_plan_file(session) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return (content, path.display().to_string());
            }
        }
        // Fallback if no plan file found
        (
            "(Plan content not found)".to_string(),
            ".claude/plans/plan.md".to_string(),
        )
    }

    /// Extract a filename from tool result text
    fn extract_filename(text: &str) -> Option<String> {
        // Look for common file path patterns
        for line in text.lines() {
            let line = line.trim();
            // Look for paths like /path/to/file.rs or file.rs
            if line.contains('/') || line.contains('.') {
                // Try to find a file path
                for word in line.split_whitespace() {
                    let word = word.trim_matches(|c: char| {
                        !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
                    });
                    if word.contains('.') && !word.starts_with('.') {
                        // Looks like a filename
                        return Some(word.to_string());
                    }
                }
            }
        }
        None
    }

    /// Dump complete app state to a JSON file for debugging.
    fn dump_debug_state(&self) -> Result<String, String> {
        use chrono::Local;
        use serde_json::json;

        let timestamp = Local::now().format("%Y%m%d_%H%M%S");

        // Save to ~/.conduit/debug/ directory
        let debug_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".conduit")
            .join("debug");

        // Create directory if it doesn't exist
        std::fs::create_dir_all(&debug_dir)
            .map_err(|e| format!("Could not create debug directory: {}", e))?;

        let filepath = debug_dir.join(format!("conduit_debug_{}.json", timestamp));

        let mut sessions_data = Vec::new();

        for (idx, session) in self.state.tab_manager.sessions().iter().enumerate() {
            // Collect chat messages
            let messages: Vec<_> = session
                .chat_view
                .messages()
                .iter()
                .map(|msg| {
                    let summary_data = msg.summary.as_ref().map(|s| {
                        json!({
                            "duration_secs": s.duration_secs,
                            "input_tokens": s.input_tokens,
                            "output_tokens": s.output_tokens,
                            "files_changed": s.files_changed.iter().map(|f| json!({
                                "filename": f.filename,
                                "additions": f.additions,
                                "deletions": f.deletions,
                            })).collect::<Vec<_>>(),
                        })
                    });

                    json!({
                        "role": format!("{:?}", msg.role),
                        "content": msg.content,
                        "content_length": msg.content.len(),
                        "tool_name": msg.tool_name,
                        "tool_args": msg.tool_args,
                        "is_streaming": msg.is_streaming,
                        "has_summary": msg.summary.is_some(),
                        "summary": summary_data,
                    })
                })
                .collect();

            // Collect raw events
            let raw_events: Vec<_> = session
                .raw_events_view
                .events()
                .iter()
                .map(|evt| {
                    let elapsed = evt.timestamp.duration_since(evt.session_start);
                    json!({
                        "timestamp_ms": elapsed.as_millis(),
                        "direction": format!("{:?}", evt.direction),
                        "event_type": evt.event_type,
                        "raw_json": evt.raw_json,
                    })
                })
                .collect();

            // Current turn summary
            let turn_summary = json!({
                "duration_secs": session.current_turn_summary.duration_secs,
                "input_tokens": session.current_turn_summary.input_tokens,
                "output_tokens": session.current_turn_summary.output_tokens,
                "files_changed": session.current_turn_summary.files_changed.iter().map(|f| json!({
                    "filename": f.filename,
                    "additions": f.additions,
                    "deletions": f.deletions,
                })).collect::<Vec<_>>(),
            });

            sessions_data.push(json!({
                "index": idx,
                "id": session.id.to_string(),
                "agent_type": format!("{:?}", session.agent_type),
                "agent_session_id": session.agent_session_id.as_ref().map(|s| s.as_str().to_string()),
                "is_processing": session.is_processing,
                "turn_count": session.turn_count,
                "total_usage": {
                    "input_tokens": session.total_usage.input_tokens,
                    "output_tokens": session.total_usage.output_tokens,
                    "cached_tokens": session.total_usage.cached_tokens,
                    "total_tokens": session.total_usage.total_tokens,
                },
                "current_turn_summary": turn_summary,
                "chat_messages": messages,
                "chat_message_count": session.chat_view.len(),
                "streaming_buffer": session.chat_view.streaming_buffer(),
                "streaming_reasoning": session.chat_view.streaming_message_for(MessageRole::Reasoning),
                "raw_events": raw_events,
                "raw_event_count": session.raw_events_view.len(),
                "input_box_content": session.input_box.input(),
            }));
        }

        let dump = json!({
            "timestamp": Local::now().to_rfc3339(),
            "view_mode": format!("{:?}", self.state.view_mode),
            "input_mode": format!("{:?}", self.state.input_mode),
            "active_tab_index": self.state.tab_manager.active_index(),
            "tab_count": self.state.tab_manager.len(),
            "sessions": sessions_data,
        });

        let full_path = filepath.display().to_string();
        let mut file =
            File::create(&filepath).map_err(|e| format!("Could not create file: {}", e))?;
        let json_str = serde_json::to_string_pretty(&dump)
            .map_err(|e| format!("Could not serialize debug data: {}", e))?;
        file.write_all(json_str.as_bytes())
            .map_err(|e| format!("Could not write to file: {}", e))?;

        Ok(full_path)
    }
}

struct SessionStateSnapshot {
    tabs: Vec<SessionTab>,
    active_tab_index: usize,
    sidebar_visible: bool,
    tree_selected_index: usize,
    collapsed_repo_ids: Vec<uuid::Uuid>,
}

#[derive(Debug, Default)]
struct SessionPersistenceReport {
    errors: Vec<String>,
}

impl SessionPersistenceReport {
    fn push(&mut self, message: String) {
        self.errors.push(message);
    }

    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    fn error_count(&self) -> usize {
        self.errors.len()
    }

    fn first_error_or_unknown(&self) -> &str {
        self.errors
            .first()
            .map(std::string::String::as_str)
            .unwrap_or("unknown error")
    }
}

/// Async helper for generating title and branch name
async fn generate_title_and_branch_impl(
    tools: ToolAvailability,
    user_message: String,
    working_dir: PathBuf,
    workspace_id: Option<uuid::Uuid>,
    current_branch: String,
    worktree_manager: WorkspaceRepoManager,
    workspace_dao: Option<WorkspaceStore>,
) -> Result<TitleGeneratedResult, String> {
    use crate::util::{generate_title_and_branch, get_git_username, sanitize_branch_suffix};

    // Call AI for title generation
    let metadata = generate_title_and_branch(&tools, &user_message, &working_dir)
        .await
        .map_err(|e| e.to_string())?;

    // Try to rename branch if workspace exists
    let new_branch = if workspace_id.is_some() {
        // Always fetch fresh branch from git - the passed-in current_branch may be stale
        // Only fall back to passed-in value if git lookup fails or returns empty
        let resolved_branch = {
            let wd = working_dir.clone();
            let wm = worktree_manager.clone();
            let wd_for_log = wd.clone();
            let fresh_branch = match tokio::task::spawn_blocking(move || {
                wm.get_current_branch(&wd).map_err(|e| e.to_string())
            })
            .await
            {
                Ok(Ok(branch)) => branch,
                Ok(Err(err)) => {
                    tracing::warn!(
                        error = %err,
                        working_dir = %wd_for_log.display(),
                        "Failed to fetch current branch from worktree"
                    );
                    String::new()
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "spawn_blocking failed while fetching current branch"
                    );
                    String::new()
                }
            };
            if fresh_branch.is_empty() {
                current_branch.clone()
            } else {
                fresh_branch
            }
        };

        if resolved_branch.is_empty() {
            tracing::debug!("Skipping branch rename: could not determine current branch");
            None
        } else {
            let raw_username = get_git_username();
            // Sanitize username to ensure valid git ref (spaces, special chars become hyphens)
            // Note: sanitize_branch_suffix returns "task" for empty input, so we only check for "task"
            let username = sanitize_branch_suffix(&raw_username);
            let suffix = sanitize_branch_suffix(&metadata.branch_suffix);

            // Skip branch rename if suffix is just the fallback "task"
            // (this can happen with non-ASCII only input or empty AI response)
            if suffix == "task" {
                tracing::debug!(
                    suffix = %suffix,
                    "Skipping branch rename: sanitized suffix is generic fallback"
                );
                None
            } else {
                // If username sanitizes to fallback, drop the prefix and use the suffix alone.
                // (Suffix is already sanitized to ASCII kebab-case with no slashes.)
                let new_branch_name = if username == "task" {
                    tracing::debug!(
                        raw_username = %raw_username,
                        sanitized = %username,
                        "Username unusable; generating branch without username prefix"
                    );
                    suffix.clone()
                } else {
                    format!("{}/{}", username, suffix)
                };

                // Only rename if the new name differs from current
                if new_branch_name != resolved_branch {
                    let wd = working_dir.clone();
                    let old = resolved_branch.clone();
                    let new_name = new_branch_name.clone();
                    let wm = worktree_manager.clone();

                    // Capture full error result instead of just is_ok()
                    // Branch rename is best-effort: join errors shouldn't prevent applying the title
                    let rename_join_result = tokio::task::spawn_blocking(move || {
                        wm.rename_branch(&wd, &old, &new_name)
                            .map_err(|e| e.to_string())
                    })
                    .await;

                    match rename_join_result {
                        Ok(Ok(())) => {
                            // Update database if rename succeeded
                            if let (Some(ws_id), Some(ref dao)) = (workspace_id, &workspace_dao) {
                                let db_update_result = tokio::task::spawn_blocking({
                                    let dao = dao.clone();
                                    let new_branch = new_branch_name.clone();
                                    move || {
                                        if let Ok(Some(mut ws)) = dao.get_by_id(ws_id) {
                                            ws.branch = new_branch.clone();
                                            dao.update(&ws).map_err(|e| {
                                                format!(
                                                    "Failed to update workspace branch to {}: {}",
                                                    new_branch, e
                                                )
                                            })
                                        } else {
                                            Err(format!(
                                                "Workspace {} not found for branch update",
                                                ws_id
                                            ))
                                        }
                                    }
                                })
                                .await;

                                // Log any errors from the DB update (don't fail the whole operation)
                                match db_update_result {
                                    Ok(Ok(())) => {}
                                    Ok(Err(e)) => {
                                        tracing::warn!(
                                            error = %e,
                                            workspace_id = %ws_id,
                                            "Failed to persist branch rename to database"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            error = %e,
                                            workspace_id = %ws_id,
                                            "spawn_blocking failed for database update"
                                        );
                                    }
                                }
                            }
                            Some(new_branch_name)
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(
                                error = %e,
                                old_branch = %resolved_branch,
                                new_branch = %new_branch_name,
                                "Failed to rename git branch"
                            );
                            None
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                old_branch = %resolved_branch,
                                new_branch = %new_branch_name,
                                "spawn_blocking join failed during branch rename"
                            );
                            None
                        }
                    }
                } else {
                    None
                }
            }
        }
    } else {
        None
    };

    Ok(TitleGeneratedResult {
        title: app_prompt::sanitize_title(&metadata.title),
        new_branch,
        workspace_id,
        tool_used: metadata.tool_used.clone(),
        used_fallback: metadata.used_fallback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::{AssistantMessageEvent, ReasoningEvent, TurnCompletedEvent};
    use crate::agent::{AgentType, ModelRegistry, ReasoningEffort, SessionId, TokenUsage};
    use crate::config::Config;
    use crate::data::{QueuedMessage, QueuedMessageMode};
    use crate::ui::components::MessageRole;
    use crate::ui::session::AgentSession;
    use crate::util::{Tool, ToolAvailability};
    use chrono::Utc;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn init_test_data_dir() -> PathBuf {
        static TEST_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
        TEST_DATA_DIR
            .get_or_init(|| {
                let dir = tempfile::Builder::new()
                    .prefix("conduit-test-data-")
                    .tempdir()
                    .expect("Failed to create test data dir");
                let path = dir.path().to_path_buf();
                // Keep temp dir alive for test process lifetime.
                std::mem::forget(dir);
                crate::util::init_data_dir(Some(path.clone()));
                path
            })
            .clone()
    }

    fn build_test_app_with_sessions(session_ids: &[Uuid]) -> App {
        init_test_data_dir();
        let config = Config::default();
        let tools = ToolAvailability::default();
        let core = crate::core::ConduitCore::new(config, tools);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new(10);

        for session_id in session_ids {
            let mut session = AgentSession::new(AgentType::Codex);
            session.id = *session_id;
            state.tab_manager.add_session(session);
        }

        App {
            core,
            state,
            event_tx,
            event_rx,
            git_tracker: None,
        }
    }

    fn create_test_file(contents: &str) -> PathBuf {
        let path = init_test_data_dir().join(format!("file-viewer-{}.txt", Uuid::new_v4()));
        std::fs::write(&path, contents).expect("failed to write test file");
        path
    }

    fn create_test_markdown_file(contents: &str) -> PathBuf {
        let path = init_test_data_dir().join(format!("file-viewer-{}.md", Uuid::new_v4()));
        std::fs::write(&path, contents).expect("failed to write markdown test file");
        path
    }

    fn status_bar_model_click_position(app: &App, status_bar_area: Rect) -> (u16, u16) {
        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");

        let show_mode = session.capabilities.supports_plan_mode;
        let mode_width = if show_mode {
            session.agent_mode.display_name().len()
        } else {
            0
        };
        let model_id = session
            .model
            .clone()
            .unwrap_or_else(|| ModelRegistry::default_model(session.agent_type));
        let model_display = ModelRegistry::find_model(session.agent_type, &model_id)
            .map(|m| m.display_name.to_string())
            .unwrap_or(model_id);
        let model_width = model_display.len();
        let agent_width = session.agent_type.display_name().len();

        let leading: usize = 2;
        let relative_x = if show_mode {
            let model_start = leading + mode_width + 2 - 1;
            let model_end = leading + mode_width + 2 + model_width + 1 + agent_width + 1;
            (model_start + model_end) / 2
        } else {
            let model_start = leading.saturating_sub(1);
            let model_end = leading + model_width + 1 + agent_width + 1;
            (model_start + model_end) / 2
        };

        (status_bar_area.x + relative_x as u16, status_bar_area.y)
    }

    #[test]
    fn test_apply_session_persistence_report_sets_footer_warning() {
        let mut app = build_test_app_with_sessions(&[]);
        let mut report = SessionPersistenceReport::default();
        report.push("failed to save state".to_string());

        app.apply_session_persistence_report(report);

        assert_eq!(
            app.state.footer_message.as_deref(),
            Some("Warning: some session state could not be saved. Check logs.")
        );
        assert!(app.state.footer_message_expires_at.is_some());
    }

    #[test]
    fn test_handle_open_file_sets_file_viewer_mode() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let path = create_test_file("line-1\nline-2\n");
        let mut effects = Vec::new();

        app.handle_open_file(path.clone(), &mut effects);

        assert!(app.state.tab_manager.active_is_file());
        assert_eq!(app.state.input_mode, InputMode::FileViewer);
        assert_eq!(
            app.state
                .tab_manager
                .active_file_viewer()
                .expect("file viewer missing")
                .file_path,
            path
        );
        assert!(matches!(effects.as_slice(), [Effect::SaveSessionState]));
    }

    #[test]
    fn test_markdown_file_defaults_to_rendered_mode() {
        let mut app = build_test_app_with_sessions(&[]);
        let path = create_test_markdown_file(
            "# Title\nThis markdown paragraph is long enough to wrap in a narrow viewport.\n",
        );
        app.state
            .tab_manager
            .open_file(path)
            .expect("open markdown file");
        app.sync_input_mode_for_active_tab();

        let viewer = app
            .state
            .tab_manager
            .active_file_viewer_mut()
            .expect("file viewer missing");
        assert_eq!(
            viewer.active_view_mode(),
            crate::ui::file_viewer::FileViewMode::Rendered
        );

        viewer.ensure_render_cache(20);
        assert!(viewer.effective_total_lines() >= viewer.total_lines);
    }

    #[test]
    fn test_sync_input_mode_for_active_tab_tracks_file_tab_transitions() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let path = create_test_file("line-1\nline-2\nline-3\n");
        app.state.tab_manager.open_file(path).expect("open file");

        app.sync_input_mode_for_active_tab();
        assert_eq!(app.state.input_mode, InputMode::FileViewer);

        app.state.tab_manager.switch_to(0);
        app.sync_input_mode_for_active_tab();
        assert_eq!(app.state.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_lookup_footer_action_uses_file_viewer_context() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let path = create_test_file("line-1\nline-2\nline-3\n");
        app.state.tab_manager.open_file(path).expect("open file");
        app.sync_input_mode_for_active_tab();

        assert_eq!(app.lookup_footer_action("tab"), Some(Action::NextTab));
        assert_eq!(app.lookup_footer_action("j"), Some(Action::ScrollDown(1)));
        assert_eq!(app.lookup_footer_action("q"), Some(Action::CloseTab));
        assert_eq!(app.lookup_footer_action("esc"), Some(Action::CloseTab));
    }

    #[test]
    fn test_flush_scroll_deltas_scrolls_file_viewer() {
        let mut app = build_test_app_with_sessions(&[]);
        let content = (0..50)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = create_test_file(&content);
        app.state.tab_manager.open_file(path).expect("open file");
        app.sync_input_mode_for_active_tab();

        let mut pending_up = 0usize;
        let mut pending_down = 4usize;
        app.flush_scroll_deltas(&mut pending_up, &mut pending_down);

        assert_eq!(pending_up, 0);
        assert_eq!(pending_down, 0);
        assert_eq!(
            app.state
                .tab_manager
                .active_file_viewer()
                .expect("file viewer missing")
                .scroll_offset,
            4
        );
    }

    #[test]
    fn test_should_handle_as_text_input_false_for_file_viewer_context() {
        let mut app = build_test_app_with_sessions(&[]);
        let path = create_test_file("line-1\nline-2\n");
        app.state.tab_manager.open_file(path).expect("open file");
        app.sync_input_mode_for_active_tab();

        let key = crossterm::event::KeyEvent {
            code: crossterm::event::KeyCode::Char('j'),
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        };
        let context = app.key_context_for_active_tab();

        assert_eq!(context, KeyContext::FileViewer);
        assert!(!app.should_handle_as_text_input(&key, context));
    }

    #[test]
    fn test_colon_triggers_command_mode_on_empty_input() {
        // Typing ":" on empty input SHOULD trigger command mode
        let result = App::should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::Normal,
            true, // input_is_empty
            false,
            false,
        );
        assert!(result, "Colon should trigger command mode on empty input");
    }

    #[test]
    fn test_colon_with_modifiers_does_not_trigger_command_mode() {
        // Typing "Shift+:" should NOT trigger command mode
        let result = App::should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::SHIFT,
            InputMode::Normal,
            true,
            false,
            false,
        );
        assert!(
            !result,
            "Colon with modifiers should not trigger command mode"
        );
    }

    /// Test that ":" does NOT trigger command mode when input box has content.
    /// This verifies the fix for the bug where pasting "hello:world" would
    /// incorrectly trigger command mode when the ":" character was encountered.
    #[test]
    fn test_colon_does_not_trigger_command_mode_with_existing_input() {
        // Simulate: user has typed "hello" and now types ":"
        // ":" should be inserted as a regular character, not trigger command mode
        let result = App::should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::Normal,
            false, // input already has content
            false,
            false,
        );

        assert!(
            !result,
            "Colon should NOT trigger command mode when input has existing content"
        );
    }

    /// Test case: pasting "url:port" pattern should not trigger command mode
    #[test]
    fn test_paste_url_with_port_does_not_trigger_command_mode() {
        // Simulate: user pastes "localhost:8080"
        // After pasting "localhost", the ":" should be inserted, not trigger command mode
        let result = App::should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::Normal,
            false, // input has content from paste
            false,
            false,
        );

        assert!(
            !result,
            "Pasting 'localhost:8080' should not trigger command mode at ':'"
        );
    }

    #[test]
    fn test_colon_does_not_trigger_in_selecting_model() {
        let result = App::should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::SelectingModel,
            true,
            false,
            false,
        );

        assert!(
            !result,
            "Colon should NOT trigger command mode while selecting a model"
        );
    }

    #[test]
    fn test_slash_triggers_menu_on_empty_input() {
        let result = App::should_trigger_slash_menu(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
            InputMode::Normal,
            true,
            false,
            false,
            true,
        );
        assert!(result, "Slash should trigger menu on empty input");
    }

    #[test]
    fn test_dollar_triggers_menu_on_empty_input() {
        let result = App::should_trigger_slash_menu(
            KeyCode::Char('$'),
            KeyModifiers::NONE,
            InputMode::Normal,
            true,
            false,
            false,
            true,
        );
        assert!(result, "Dollar should trigger menu on empty input");
    }

    #[test]
    fn test_slash_does_not_trigger_with_existing_input() {
        let result = App::should_trigger_slash_menu(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
            InputMode::Normal,
            false,
            false,
            false,
            true,
        );
        assert!(
            !result,
            "Slash should not trigger menu when input has content"
        );
    }

    #[test]
    fn test_slash_does_not_trigger_without_session() {
        let result = App::should_trigger_slash_menu(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
            InputMode::Normal,
            true,
            false,
            false,
            false,
        );
        assert!(
            !result,
            "Slash should not trigger menu without an active session"
        );
    }

    #[test]
    fn test_slash_command_action_maps_fork_to_fork_session() {
        assert_eq!(
            App::slash_command_action(ConduitCommand::Fork),
            Some(Action::ForkSession)
        );
    }

    #[test]
    fn test_slash_command_action_maps_handoff_when_present() {
        let mut slash_state = crate::ui::components::SlashMenuState::new();
        let entries = CommandResolver::menu_entries(std::path::Path::new("."));
        slash_state.show_with_entries('/', entries);

        let entry = slash_state
            .commands
            .iter()
            .find(|entry| entry.label == "/handoff")
            .expect("Expected /handoff command to be present");
        assert_eq!(
            App::slash_command_action(match entry.kind {
                MenuEntryKind::ConduitCommand(command) => command,
                _ => panic!("expected conduit command"),
            }),
            Some(Action::HandoffSession)
        );
    }

    #[test]
    fn test_first_time_splash_shortcuts_only_active_in_normal_without_overlay() {
        assert!(App::should_handle_first_time_splash_shortcuts(
            true,
            InputMode::Normal,
            false
        ));
        assert!(App::should_handle_first_time_splash_shortcuts(
            true,
            InputMode::Scrolling,
            false
        ));
        assert!(!App::should_handle_first_time_splash_shortcuts(
            true,
            InputMode::SelectingModel,
            false
        ));
        assert!(!App::should_handle_first_time_splash_shortcuts(
            true,
            InputMode::Normal,
            true
        ));
        assert!(!App::should_handle_first_time_splash_shortcuts(
            false,
            InputMode::Normal,
            false
        ));
    }

    #[test]
    fn test_build_fork_seed_prompt_includes_roles() {
        use crate::ui::components::ChatMessage;

        let mut summary = crate::ui::components::TurnSummary::new();
        summary.duration_secs = 12;
        summary.input_tokens = 100;
        summary.output_tokens = 200;

        let messages = vec![
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there"),
            ChatMessage::tool_with_exit("Bash", "ls -la", "file.txt", Some(0)),
            ChatMessage::turn_summary(summary),
        ];

        let prompt = app_prompt::build_fork_seed_prompt(&messages);

        // Check header and structure
        assert!(prompt.contains("[CONDUIT_FORK_SEED]"));
        assert!(prompt.contains("<previous-session-transcript>"));
        assert!(prompt.contains("</previous-session-transcript>"));
        assert!(prompt.contains("[END OF CONTEXT]"));
        assert!(prompt.contains("reply with ONLY"));
        assert!(prompt.contains("Ready"));

        // Check message content
        assert!(prompt.contains("[role=user]"));
        assert!(prompt.contains("[role=assistant]"));
        assert!(prompt.contains("name=\"Bash\""));
        assert!(prompt.contains("args=\"ls -la\""));
        assert!(prompt.contains("exit=0"));
        assert!(prompt.contains("[role=summary]"));
        assert!(prompt.contains("tokens_in=100"));
        assert!(prompt.contains("tokens_out=200"));
    }

    #[test]
    fn test_build_fork_seed_prompt_truncates_large_transcript() {
        use crate::ui::components::ChatMessage;

        let oversized = "a".repeat(app_prompt::MAX_SEED_PROMPT_SIZE + 10_000);
        let messages = vec![ChatMessage::user(oversized)];

        let prompt = app_prompt::build_fork_seed_prompt(&messages);

        assert!(
            prompt.contains("[TRUNCATED: transcript exceeded size limit]"),
            "Expected truncation marker"
        );
        assert!(prompt.contains("[END OF CONTEXT]"));
        assert!(prompt.ends_with("Ready"));
    }

    #[test]
    fn test_strip_image_placeholders_removes_placeholders() {
        let prompt = "Hello [img] world".to_string();
        let placeholders = vec!["[img]".to_string()];

        let cleaned = App::strip_image_placeholders(prompt, &placeholders);

        assert_eq!(cleaned, "Hello  world");
    }

    #[test]
    fn test_build_user_prompt_jsonl_with_no_images() {
        let result = App::build_user_prompt_jsonl("Test prompt", &[]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();

        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");

        let content = parsed["message"]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Test prompt");
    }

    #[test]
    fn test_build_user_prompt_jsonl_with_missing_images_fallback() {
        // Test with non-existent image paths - should add fallback text blocks
        let images = vec![PathBuf::from("/nonexistent/image.png")];
        let result = App::build_user_prompt_jsonl("Test prompt", &images).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();

        let content = parsed["message"]["content"].as_array().unwrap();
        // Should have fallback text for failed image + the prompt text
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0]["text"]
            .as_str()
            .unwrap()
            .contains("Failed to load image"));
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "Test prompt");
    }

    #[test]
    fn test_build_user_prompt_jsonl_with_real_image() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a minimal valid PNG file (1x1 red pixel)
        let png_data: [u8; 70] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 dimensions
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, // 8-bit RGB
            0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
            0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x18,
            0xDD, 0x8D, 0xB5, // compressed image data
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
            0x82, // IEND chunk
        ];

        let mut temp_file = NamedTempFile::with_suffix(".png").unwrap();
        temp_file.write_all(&png_data).unwrap();
        let temp_path = temp_file.path().to_path_buf();

        let result =
            App::build_user_prompt_jsonl("What is in this image?", &[temp_path.clone()]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(result.trim()).unwrap();

        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");

        let content = parsed["message"]["content"].as_array().unwrap();
        // Should have image block + text block
        assert_eq!(content.len(), 2, "Expected 2 content blocks (image + text)");

        // First block should be an image
        assert_eq!(content[0]["type"], "image", "First block should be image");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[0]["source"]["media_type"], "image/png");
        // Verify base64 data is non-empty
        let base64_data = content[0]["source"]["data"].as_str().unwrap();
        assert!(!base64_data.is_empty(), "base64 data should not be empty");

        // Second block should be text
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "What is in this image?");
    }

    #[test]
    fn test_truncate_queue_line_handles_small_widths() {
        assert_eq!(app_queue::truncate_queue_line("abcdef", 4), "a...");
        assert_eq!(app_queue::truncate_queue_line("abcdef", 3), "...");
        assert_eq!(app_queue::truncate_queue_line("abcdef", 2), "..");
        assert_eq!(app_queue::truncate_queue_line("abcdef", 0), "");
    }

    #[test]
    fn test_build_queued_submission_concat_vs_separate() {
        let msg_a = QueuedMessage {
            id: Uuid::new_v4(),
            mode: QueuedMessageMode::FollowUp,
            text: "First".to_string(),
            images: Vec::new(),
            created_at: Utc::now(),
        };
        let msg_b = QueuedMessage {
            id: Uuid::new_v4(),
            mode: QueuedMessageMode::Steer,
            text: "Second".to_string(),
            images: Vec::new(),
            created_at: Utc::now(),
        };

        let (concat, _, _) = app_queue::build_queued_submission(
            &[msg_a.clone(), msg_b.clone()],
            crate::config::QueueDelivery::Concat,
        );
        let (separate, _, _) = app_queue::build_queued_submission(
            &[msg_a.clone(), msg_b.clone()],
            crate::config::QueueDelivery::Separate,
        );

        assert_eq!(concat, "First\n\nSecond");
        assert!(separate.contains("[Queued 1 of 2]"));
        assert!(separate.contains("[Queued 2 of 2]"));
    }

    #[test]
    fn test_sanitize_title_collapses_whitespace_and_bounds_length() {
        let title = "  Hello\n\tworld  ".to_string();
        let cleaned = app_prompt::sanitize_title(&title);
        assert_eq!(cleaned, "Hello world");

        let long = "a".repeat(250);
        let bounded = app_prompt::sanitize_title(&long);
        assert!(bounded.chars().count() <= 200);

        let empty = "\n\t\r".to_string();
        let fallback = app_prompt::sanitize_title(&empty);
        assert_eq!(fallback, "Untitled task");
    }

    #[tokio::test]
    async fn test_agent_event_routes_streaming_by_session_id_after_tab_close() {
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();
        let session_c = Uuid::new_v4();

        let mut app = build_test_app_with_sessions(&[session_a, session_b, session_c]);

        // Close the first tab so indices shift: B -> 0, C -> 1
        assert!(app.state.tab_manager.close_tab(0));
        assert_eq!(
            app.state.tab_manager.session_index_by_id(session_b),
            Some(0)
        );
        assert_eq!(
            app.state.tab_manager.session_index_by_id(session_c),
            Some(1)
        );

        let event = AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "message for B".to_string(),
            is_final: false,
        });

        app.handle_agent_event(session_b, event).await.unwrap();

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_b)
                .expect("session B missing");
            assert_eq!(session.chat_view.streaming_buffer(), Some("message for B"));
            assert!(session.chat_view.messages().is_empty());
        }

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_c)
                .expect("session C missing");
            assert!(session.chat_view.streaming_buffer().is_none());
            assert!(session.chat_view.messages().is_empty());
        }
    }

    #[tokio::test]
    async fn test_agent_event_routes_reasoning_by_session_id_after_tab_close() {
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();
        let session_c = Uuid::new_v4();

        let mut app = build_test_app_with_sessions(&[session_a, session_b, session_c]);

        // Close the first tab so indices shift: B -> 0, C -> 1
        assert!(app.state.tab_manager.close_tab(0));
        assert_eq!(
            app.state.tab_manager.session_index_by_id(session_b),
            Some(0)
        );
        assert_eq!(
            app.state.tab_manager.session_index_by_id(session_c),
            Some(1)
        );

        let event = AgentEvent::AssistantReasoning(ReasoningEvent {
            text: "thinking...".to_string(),
        });

        app.handle_agent_event(session_b, event).await.unwrap();

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_b)
                .expect("session B missing");
            assert_eq!(
                session
                    .chat_view
                    .streaming_message_for(MessageRole::Reasoning),
                Some("thinking...")
            );
        }

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_c)
                .expect("session C missing");
            assert!(session
                .chat_view
                .streaming_message_for(MessageRole::Reasoning)
                .is_none());
        }
    }

    #[tokio::test]
    async fn test_agent_event_routes_final_by_session_id_after_tab_close() {
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();
        let session_c = Uuid::new_v4();

        let mut app = build_test_app_with_sessions(&[session_a, session_b, session_c]);

        // Close the first tab so indices shift: B -> 0, C -> 1
        assert!(app.state.tab_manager.close_tab(0));
        assert_eq!(
            app.state.tab_manager.session_index_by_id(session_b),
            Some(0)
        );
        assert_eq!(
            app.state.tab_manager.session_index_by_id(session_c),
            Some(1)
        );

        let event = AgentEvent::AssistantMessage(AssistantMessageEvent {
            text: "message for B".to_string(),
            is_final: true,
        });

        app.handle_agent_event(session_b, event).await.unwrap();

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_b)
                .expect("session B missing");
            assert!(session.chat_view.streaming_buffer().is_none());
            let messages = session.chat_view.messages();
            let last = messages.last().expect("missing assistant message");
            assert_eq!(last.role, MessageRole::Assistant);
            assert_eq!(last.content, "message for B");
        }

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_c)
                .expect("session C missing");
            assert!(session.chat_view.streaming_buffer().is_none());
            assert!(session.chat_view.messages().is_empty());
        }
    }

    #[tokio::test]
    async fn test_turn_completed_clears_codex_input_channel() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);

        let (tx, _rx) = mpsc::channel(1);
        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_id)
                .expect("session missing");
            session.agent_type = AgentType::Codex;
            session.agent_input_tx = Some(tx);
            session.start_processing();
        }

        app.handle_agent_event(
            session_id,
            AgentEvent::TurnCompleted(TurnCompletedEvent {
                usage: TokenUsage::default(),
            }),
        )
        .await
        .unwrap();

        let session = app
            .state
            .tab_manager
            .session_by_id_mut(session_id)
            .expect("session missing");
        assert!(session.agent_input_tx.is_none());
        assert!(!session.is_processing);
    }

    #[test]
    fn test_submit_prompt_for_tab_does_not_resume_stale_codex_session_without_live_channel() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let cwd = std::env::current_dir().expect("cwd");
        let saved_session = SessionId::from_string("codex-thread-123");
        let default_model = app.config().default_model_for(AgentType::Codex);

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_id)
                .expect("session missing");
            session.agent_type = AgentType::Codex;
            session.model = Some(default_model);
            session.model_invalid = false;
            session.working_dir = Some(cwd.clone());
            session.resume_session_id = Some(saved_session);
            session.agent_input_tx = None;
        }

        let effects = app
            .submit_prompt_for_tab(0, "hi".to_string(), vec![], vec![], false, None)
            .expect("submit should succeed");

        let (agent_type, config) = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::StartAgent {
                    agent_type, config, ..
                } => Some((agent_type, config)),
                _ => None,
            })
            .expect("expected StartAgent effect");

        assert_eq!(*agent_type, AgentType::Codex);
        assert!(config.resume_session.is_none());
        assert_eq!(config.prompt, "hi");
    }

    #[test]
    fn test_submit_prompt_for_tab_resumes_live_codex_session_after_turn_completion() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let cwd = std::env::current_dir().expect("cwd");
        let live_session = SessionId::from_string("codex-thread-live");
        let default_model = app.config().default_model_for(AgentType::Codex);

        {
            let session = app
                .state
                .tab_manager
                .session_by_id_mut(session_id)
                .expect("session missing");
            session.agent_type = AgentType::Codex;
            session.model = Some(default_model);
            session.model_invalid = false;
            session.working_dir = Some(cwd.clone());
            session.agent_session_id = Some(live_session.clone());
            session.resume_session_id = Some(SessionId::from_string("historic-session"));
            session.agent_input_tx = None;
        }

        let effects = app
            .submit_prompt_for_tab(0, "hi again".to_string(), vec![], vec![], false, None)
            .expect("submit should succeed");

        let (agent_type, config) = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::StartAgent {
                    agent_type, config, ..
                } => Some((agent_type, config)),
                _ => None,
            })
            .expect("expected StartAgent effect");

        assert_eq!(*agent_type, AgentType::Codex);
        assert_eq!(config.resume_session.as_ref(), Some(&live_session));
        assert_eq!(config.prompt, "hi again");
    }

    #[test]
    fn test_handle_list_action_select_next_for_project_picker() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::PickingProject;
        app.state
            .project_picker_state
            .list
            .set_filtered(vec![0, 1, 2]);
        app.state.project_picker_state.list.selected = 0;

        app.handle_list_action(Action::SelectNext);

        assert_eq!(app.state.project_picker_state.list.selected, 1);
    }

    #[test]
    fn test_handle_list_action_page_down_up_for_session_import() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::ImportingSession;
        let filtered: Vec<usize> = (0..15).collect();
        app.state.session_import_state.list.set_filtered(filtered);
        app.state.session_import_state.list.selected = 0;

        app.handle_list_action(Action::SelectPageDown);
        assert_eq!(app.state.session_import_state.list.selected, 10);

        app.handle_list_action(Action::SelectPageUp);
        assert_eq!(app.state.session_import_state.list.selected, 0);
    }

    #[test]
    fn test_handle_raw_events_toggle_expand_and_collapse() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.raw_events_view.push_event(
                EventDirection::Received,
                "test_event",
                json!({ "ok": true }),
            );
            assert!(!session.raw_events_view.is_expanded());
        }

        let mut effects = Vec::new();
        app.handle_raw_events_action(Action::RawEventsToggleExpand, &mut effects);
        assert!(app
            .state
            .tab_manager
            .active_session()
            .expect("session missing")
            .raw_events_view
            .is_expanded());

        app.handle_raw_events_action(Action::RawEventsCollapse, &mut effects);
        assert!(!app
            .state
            .tab_manager
            .active_session()
            .expect("session missing")
            .raw_events_view
            .is_expanded());
    }

    #[test]
    fn test_handle_raw_events_copy_selected_json() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.raw_events_view.push_event(
                EventDirection::Received,
                "test_event",
                json!({ "foo": "bar" }),
            );
        }

        let mut effects = Vec::new();
        app.handle_raw_events_action(Action::EventDetailCopy, &mut effects);

        let expected = serde_json::to_string_pretty(&json!({ "foo": "bar" })).unwrap();
        assert!(matches!(
            effects.as_slice(),
            [Effect::CopyToClipboard(content)] if content == &expected
        ));
    }

    #[test]
    fn test_handle_confirmation_action_archive_workspace_remote_delete() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state.confirmation_dialog_state.visible = true;
        app.state.confirmation_dialog_state.context =
            Some(ConfirmationContext::ArchiveWorkspaceRemoteDelete { workspace_id });

        let mut effects = Vec::new();
        app.handle_confirmation_action(Action::ConfirmYes, &mut effects)
            .unwrap();

        assert!(matches!(
            effects.as_slice(),
            [Effect::ArchiveWorkspace { workspace_id: id, delete_remote: true }] if *id == workspace_id
        ));
        assert_eq!(app.state.input_mode, InputMode::Confirming);
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::ArchiveWorkspaceInProgress {
                workspace_id: id
            }) if id == workspace_id
        ));
        assert!(app.state.busy_workspaces.contains(&workspace_id));
    }

    #[test]
    fn test_handle_confirmation_action_cancel_archive_workspace() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state.confirmation_dialog_state.visible = true;
        app.state.confirmation_dialog_state.context =
            Some(ConfirmationContext::ArchiveWorkspace(workspace_id));

        let mut effects = Vec::new();
        app.handle_confirmation_action(Action::ConfirmNo, &mut effects)
            .unwrap();

        assert!(effects.is_empty());
        assert_eq!(app.state.input_mode, InputMode::SidebarNavigation);
        assert!(!app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.context.is_none());
    }

    #[test]
    fn test_handle_confirmation_action_archive_workspace_starts_async_preflight() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state.confirmation_dialog_state.visible = true;
        app.state.confirmation_dialog_state.context =
            Some(ConfirmationContext::ArchiveWorkspace(workspace_id));

        let mut effects = Vec::new();
        app.handle_confirmation_action(Action::ConfirmYes, &mut effects)
            .unwrap();

        assert!(matches!(
            effects.as_slice(),
            [Effect::ArchiveWorkspacePreflight { workspace_id: id }] if *id == workspace_id
        ));
        assert_eq!(app.state.input_mode, InputMode::Confirming);
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::ArchiveWorkspaceInProgress {
                workspace_id: id
            }) if id == workspace_id
        ));
    }

    #[test]
    fn test_handle_confirmation_action_ignores_archive_confirm_while_loading() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Archive Workspace",
                "Archiving workspace...",
                Some(ConfirmationContext::ArchiveWorkspaceInProgress { workspace_id }),
            );

        let mut effects = Vec::new();
        app.handle_confirmation_action(Action::ConfirmYes, &mut effects)
            .unwrap();

        assert!(effects.is_empty());
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert_eq!(app.state.input_mode, InputMode::Confirming);
    }

    #[test]
    fn test_handle_dialog_cancel_keeps_archive_loading_visible() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Archive Workspace",
                "Archiving workspace...",
                Some(ConfirmationContext::ArchiveWorkspaceInProgress { workspace_id }),
            );

        app.handle_dialog_action(Action::Cancel);

        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert_eq!(app.state.input_mode, InputMode::Confirming);
    }

    #[test]
    fn test_handle_dialog_cancel_keeps_remove_project_preflight_loading_visible() {
        let mut app = build_test_app_with_sessions(&[]);
        let repo_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Remove Project",
                "Analyzing project workspaces...",
                Some(ConfirmationContext::RemoveProjectPreflightInProgress { repo_id }),
            );

        app.handle_dialog_action(Action::Cancel);

        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::RemoveProjectPreflightInProgress {
                repo_id: id
            }) if id == repo_id
        ));
        assert_eq!(app.state.input_mode, InputMode::Confirming);
    }

    #[test]
    fn test_handle_dialog_cancel_keeps_fork_preflight_loading_visible() {
        let mut app = build_test_app_with_sessions(&[]);
        let parent_workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Fork Session",
                "Analyzing workspace state...",
                Some(ConfirmationContext::ForkSessionPreflightInProgress {
                    parent_workspace_id,
                }),
            );

        app.handle_dialog_action(Action::Cancel);

        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::ForkSessionPreflightInProgress {
                parent_workspace_id: id
            }) if id == parent_workspace_id
        ));
        assert_eq!(app.state.input_mode, InputMode::Confirming);
    }

    #[tokio::test]
    async fn test_archive_workspace_dialog_preflight_completed_shows_confirmation() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Archive Workspace",
                "Analyzing workspace...",
                Some(ConfirmationContext::ArchiveWorkspacePreflightInProgress { workspace_id }),
            );

        let event = AppEvent::ArchiveWorkspaceDialogPreflightCompleted {
            workspace_id,
            result: Ok(ArchiveWorkspaceDialogPreflightResult {
                workspace_name: "free-rain".to_string(),
                message: "This will remove the worktree.".to_string(),
                warnings: vec!["Uncommitted changes".to_string()],
                has_dirty: true,
                has_unmerged: false,
            }),
        };

        let effects = app.handle_app_event(event).await.unwrap();
        assert!(effects.is_empty());
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(!app.state.confirmation_dialog_state.loading);
        assert_eq!(app.state.input_mode, InputMode::Confirming);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::ArchiveWorkspace(id)) if id == workspace_id
        ));
    }

    #[tokio::test]
    async fn test_remove_project_dialog_preflight_completed_shows_confirmation() {
        let mut app = build_test_app_with_sessions(&[]);
        let repo_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Remove Project",
                "Analyzing project workspaces...",
                Some(ConfirmationContext::RemoveProjectPreflightInProgress { repo_id }),
            );

        let event = AppEvent::RemoveProjectDialogPreflightCompleted {
            repo_id,
            result: Ok(RemoveProjectDialogPreflightResult {
                repo_name: "demo-repo".to_string(),
                warnings: vec!["2 workspaces will be archived".to_string()],
                has_dirty: false,
                has_unmerged: false,
                workspace_count: 2,
            }),
        };

        let effects = app.handle_app_event(event).await.unwrap();
        assert!(effects.is_empty());
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(!app.state.confirmation_dialog_state.loading);
        assert_eq!(app.state.input_mode, InputMode::Confirming);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::RemoveProject(id)) if id == repo_id
        ));
    }

    #[tokio::test]
    async fn test_fork_session_dialog_preflight_completed_shows_confirmation() {
        let mut app = build_test_app_with_sessions(&[]);
        let parent_workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Fork Session",
                "Analyzing workspace state...",
                Some(ConfirmationContext::ForkSessionPreflightInProgress {
                    parent_workspace_id,
                }),
            );
        app.state.pending_fork_request = Some(PendingForkRequest {
            agent_type: AgentType::Codex,
            agent_mode: crate::agent::AgentMode::Build,
            model: Some("o3".to_string()),
            reasoning_effort: None,
            parent_session_id: None,
            parent_workspace_id,
            seed_prompt: std::sync::Arc::from("seed prompt"),
            token_estimate: 1600,
            context_window: 2000,
            fork_seed_id: None,
        });

        let event = AppEvent::ForkSessionDialogPreflightCompleted {
            parent_workspace_id,
            result: Ok(ForkSessionDialogPreflightResult {
                base_branch: "feature/branch".to_string(),
                dirty_warning: Some("Uncommitted changes detected".to_string()),
            }),
        };

        let effects = app.handle_app_event(event).await.unwrap();
        assert!(effects.is_empty());
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(!app.state.confirmation_dialog_state.loading);
        assert_eq!(app.state.input_mode, InputMode::Confirming);
        assert!(matches!(
            app.state.confirmation_dialog_state.context,
            Some(ConfirmationContext::ForkSession {
                parent_workspace_id: id,
                base_branch
            }) if id == parent_workspace_id && base_branch == "feature/branch"
        ));
    }

    #[tokio::test]
    async fn test_projects_discovered_populates_picker() {
        let mut app = build_test_app_with_sessions(&[]);
        let base_dir = PathBuf::from("/tmp/projects");
        app.state
            .project_picker_state
            .show_loading(base_dir.clone());
        app.state.input_mode = InputMode::PickingProject;

        let event = AppEvent::ProjectsDiscovered {
            base_dir: base_dir.clone(),
            result: Ok(vec![
                ProjectDiscoveryEntry {
                    name: "alpha".to_string(),
                    path: base_dir.join("alpha"),
                },
                ProjectDiscoveryEntry {
                    name: "beta".to_string(),
                    path: base_dir.join("beta"),
                },
            ]),
        };

        let effects = app.handle_app_event(event).await.unwrap();
        assert!(effects.is_empty());
        assert!(app.state.project_picker_state.visible);
        assert!(!app.state.project_picker_state.loading);
        assert_eq!(app.state.project_picker_state.projects.len(), 2);
        assert_eq!(app.state.project_picker_state.projects[0].name, "alpha");
        assert_eq!(app.state.project_picker_state.projects[1].name, "beta");
    }

    #[test]
    fn test_handle_input_edit_backspace_exits_command_mode_when_empty() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::Command;
        app.state.command_buffer.clear();

        app.handle_input_edit_action(Action::Backspace);

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(app.state.command_buffer.is_empty());
    }

    #[test]
    fn test_handle_input_edit_move_cursor_up_dequeues_queue() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let queued = QueuedMessage {
            id: Uuid::new_v4(),
            mode: QueuedMessageMode::FollowUp,
            text: "queued message".to_string(),
            images: Vec::new(),
            created_at: Utc::now(),
        };

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.queue_message(queued);
            assert!(session.input_box.input().is_empty());
        }

        app.handle_input_edit_action(Action::MoveCursorUp);

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        assert_eq!(session.input_box.input(), "queued message");
        assert!(session.queued_messages.is_empty());
    }

    #[test]
    fn test_handle_overlay_show_help() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::Normal;
        let mut effects = Vec::new();

        app.handle_overlay_action(Action::ShowHelp, &mut effects)
            .unwrap();

        assert_eq!(app.state.input_mode, InputMode::ShowingHelp);
        assert!(app.state.help_dialog_state.is_visible());
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_overlay_open_command_palette() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::Normal;
        let mut effects = Vec::new();

        app.handle_overlay_action(Action::OpenCommandPalette, &mut effects)
            .unwrap();

        assert_eq!(app.state.input_mode, InputMode::CommandPalette);
        assert!(app.state.command_palette_state.is_visible());
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_overlay_toggle_details() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::ShowingError;
        let mut effects = Vec::new();
        app.state
            .error_dialog_state
            .show_with_details("Oops", "Something broke", "trace");
        assert!(!app.state.error_dialog_state.details_expanded);

        app.handle_overlay_action(Action::ToggleDetails, &mut effects)
            .unwrap();

        assert!(app.state.error_dialog_state.details_expanded);
        assert!(effects.is_empty());
    }

    #[tokio::test]
    async fn test_project_removed_errors_keep_error_dialog_focused() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::SidebarNavigation;

        let event = AppEvent::ProjectRemoved {
            result: RemoveProjectResult {
                repo_id: Uuid::new_v4(),
                workspace_ids: Vec::new(),
                errors: vec!["Failed to canonicalize workspaces dir: not found".to_string()],
            },
        };

        let effects = app.handle_app_event(event).await.unwrap();
        assert!(effects.is_empty());
        assert!(app.state.error_dialog_state.is_visible());
        assert!(app.state.error_dialog_state.has_details());
        assert_eq!(app.state.input_mode, InputMode::ShowingError);

        let mut effects = Vec::new();
        app.handle_overlay_action(Action::ToggleDetails, &mut effects)
            .unwrap();
        assert!(app.state.error_dialog_state.details_expanded);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_overlay_select_agent_creates_tab() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::SelectingAgent;
        app.state.agent_selector_state.show();
        let mut effects = Vec::new();

        app.handle_overlay_action(Action::SelectAgent, &mut effects)
            .unwrap();

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(!app.state.agent_selector_state.is_visible());
        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        assert_eq!(session.agent_type, AgentType::Codex);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_submit_related_action_with_no_session() {
        let mut app = build_test_app_with_sessions(&[]);
        let mut effects = Vec::new();

        app.handle_submit_related_action(Action::Submit, &mut effects)
            .unwrap();

        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_global_quit_sets_flag_and_effect() {
        let mut app = build_test_app_with_sessions(&[]);
        let mut effects = Vec::new();

        app.handle_global_action(Action::Quit, &mut effects);

        assert!(app.state.should_quit);
        assert!(matches!(effects.as_slice(), [Effect::SaveSessionState]));
    }

    #[test]
    fn test_handle_global_toggle_view_mode() {
        let mut app = build_test_app_with_sessions(&[]);
        let mut effects = Vec::new();

        app.state.view_mode = ViewMode::Chat;
        app.handle_global_action(Action::ToggleViewMode, &mut effects);

        assert_eq!(app.state.view_mode, ViewMode::RawEvents);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_global_handoff_session_opens_model_selector() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Codex, executable));
        let mut effects = Vec::new();

        app.handle_global_action(Action::HandoffSession, &mut effects);

        assert!(effects.is_empty());
        assert_eq!(app.state.input_mode, InputMode::SelectingModel);
        assert!(app.state.model_selector_state.is_visible());
        assert_eq!(
            app.state.model_picker_context,
            ModelPickerContext::HandoffSelection
        );
        assert!(app.state.pending_handoff_request.is_some());
    }

    #[tokio::test]
    async fn test_handle_global_fork_session_uses_model_window_and_observed_tokens() {
        let session_id = Uuid::new_v4();
        let parent_workspace_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let observed_key = format!(
            "model_context_window::{}::{}",
            AgentType::Codex.as_str(),
            "gpt-5.3-codex-spark"
        );
        if let Some(store) = app.core.app_state_store() {
            let _ = store.delete(&observed_key);
        }

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.workspace_id = Some(parent_workspace_id);
            session.model = Some("gpt-5.3-codex-spark".to_string());
            session.context_state.current_tokens = 111_000;
        }

        let mut effects = Vec::new();
        app.handle_global_action(Action::ForkSession, &mut effects);

        assert!(effects.is_empty());
        let pending = app
            .state
            .pending_fork_request
            .as_ref()
            .expect("expected pending fork request");
        assert_eq!(
            pending.context_window,
            ModelRegistry::CODEX_GPT53_SPARK_CONTEXT_WINDOW
        );
        assert_eq!(pending.token_estimate, 111_000);
        assert_eq!(pending.model.as_deref(), Some("gpt-5.3-codex-spark"));
    }

    #[test]
    fn test_handle_global_copy_workspace_path() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let mut effects = Vec::new();

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.working_dir = Some(PathBuf::from("workspace"));
        }

        app.handle_global_action(Action::CopyWorkspacePath, &mut effects);

        assert!(matches!(
            effects.as_slice(),
            [Effect::CopyToClipboard(content)] if content == "workspace"
        ));
    }

    #[test]
    fn test_handle_dialog_cancel_clears_command_buffer() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::Command;
        app.state.command_buffer = "cmd".to_string();

        app.handle_dialog_action(Action::Cancel);

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(app.state.command_buffer.is_empty());
    }

    #[test]
    fn test_handle_dialog_cancel_selecting_model_clears_pending_handoff() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::SelectingModel;
        app.state
            .model_selector_state
            .show(None, app.model_selector_defaults());
        app.state.model_picker_context = ModelPickerContext::HandoffSelection;
        app.state.pending_handoff_request = Some(PendingHandoffRequest {
            source_agent_type: AgentType::Codex,
            agent_mode: AgentMode::Build,
            reasoning_effort: None,
            workspace_id: None,
            working_dir: None,
            project_name: None,
            workspace_name: None,
            pr_number: None,
            handoff_prompt: Arc::from("handoff"),
        });

        app.handle_dialog_action(Action::Cancel);

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(!app.state.model_selector_state.is_visible());
        assert_eq!(
            app.state.model_picker_context,
            ModelPickerContext::SessionSelection
        );
        assert!(app.state.pending_handoff_request.is_none());
    }

    #[test]
    fn test_handle_dialog_add_repository_from_sidebar() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::SidebarNavigation;

        app.handle_dialog_action(Action::AddRepository);

        assert_eq!(app.state.input_mode, InputMode::AddingRepository);
        assert!(app.state.add_repo_dialog_state.path.is_visible());
    }

    #[test]
    fn test_handle_confirm_action_hides_error_dialog() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::ShowingError;
        app.state.error_dialog_state.show("Error", "Boom");

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(!app.state.error_dialog_state.is_visible());
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_confirm_action_archive_workspace_starts_async_preflight() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state.confirmation_dialog_state.visible = true;
        app.state.confirmation_dialog_state.context =
            Some(ConfirmationContext::ArchiveWorkspace(workspace_id));
        app.state.confirmation_dialog_state.select_confirm();

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert!(matches!(
            effects.as_slice(),
            [Effect::ArchiveWorkspacePreflight { workspace_id: id }] if *id == workspace_id
        ));
        assert_eq!(app.state.input_mode, InputMode::Confirming);
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
    }

    #[test]
    fn test_handle_confirm_action_ignores_archive_progress_dialog() {
        let mut app = build_test_app_with_sessions(&[]);
        let workspace_id = Uuid::new_v4();
        app.state.input_mode = InputMode::Confirming;
        app.state
            .confirmation_dialog_state
            .show_loading_with_context(
                "Archive Workspace",
                "Archiving workspace...",
                Some(ConfirmationContext::ArchiveWorkspaceInProgress { workspace_id }),
            );

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert!(effects.is_empty());
        assert!(app.state.confirmation_dialog_state.visible);
        assert!(app.state.confirmation_dialog_state.loading);
        assert_eq!(app.state.input_mode, InputMode::Confirming);
    }

    #[test]
    fn test_handle_confirm_action_selecting_agent_creates_tab() {
        let mut app = build_test_app_with_sessions(&[]);
        app.state.input_mode = InputMode::SelectingAgent;
        app.state.agent_selector_state.show();

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(!app.state.agent_selector_state.is_visible());
        assert!(app.state.tab_manager.active_session().is_some());
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_confirm_action_selecting_model_executes_pending_handoff() {
        let source_session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[source_session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Codex, executable));
        let workspace_id = Uuid::new_v4();
        let working_dir = std::env::current_dir().expect("cwd");

        app.state.pending_handoff_request = Some(PendingHandoffRequest {
            source_agent_type: AgentType::Codex,
            agent_mode: AgentMode::Build,
            reasoning_effort: None,
            workspace_id: Some(workspace_id),
            working_dir: Some(working_dir.clone()),
            project_name: Some("project-a".to_string()),
            workspace_name: Some("workspace-a".to_string()),
            pr_number: Some(42),
            handoff_prompt: Arc::from("[CONDUIT_HANDOFF]\nReady"),
        });
        app.state.input_mode = InputMode::SelectingModel;
        app.state.model_picker_context = ModelPickerContext::HandoffSelection;
        app.state
            .model_selector_state
            .set_allowed_providers(Some(vec![AgentType::Codex]));
        app.state.model_selector_state.show(
            Some(app.config().default_model_for(AgentType::Codex)),
            app.model_selector_defaults(),
        );

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert!(app.state.pending_handoff_request.is_none());
        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert_eq!(app.state.tab_manager.len(), 2);
        assert!(matches!(
            effects.as_slice(),
            [Effect::StartAgent {
                agent_type: AgentType::Codex,
                ..
            }]
        ));

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        let expected_model = app.config().default_model_for(AgentType::Codex);
        assert_eq!(session.agent_type, AgentType::Codex);
        assert_eq!(session.workspace_id, Some(workspace_id));
        assert_eq!(session.working_dir.as_ref(), Some(&working_dir));
        assert_eq!(session.project_name.as_deref(), Some("project-a"));
        assert_eq!(session.workspace_name.as_deref(), Some("workspace-a"));
        assert_eq!(session.pr_number, Some(42));
        assert_eq!(session.model.as_deref(), Some(expected_model.as_str()));
        assert!(session.suppress_next_assistant_reply);
        assert!(session.suppress_next_turn_summary);
        assert!(session
            .chat_view
            .messages()
            .iter()
            .any(|message| message.role == MessageRole::System
                && message.content.contains("Handoff context was injected")));
    }

    #[test]
    fn test_execute_handoff_session_fallback_provider_uses_default_model() {
        let source_session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[source_session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Codex, executable.clone()));
        assert!(app.tools_mut().update_tool(Tool::Claude, executable));
        app.config_mut()
            .set_enabled_providers(vec![AgentType::Codex]);

        app.state.pending_handoff_request = Some(PendingHandoffRequest {
            source_agent_type: AgentType::Codex,
            agent_mode: AgentMode::Build,
            reasoning_effort: None,
            workspace_id: None,
            working_dir: None,
            project_name: None,
            workspace_name: None,
            pr_number: None,
            handoff_prompt: Arc::from("[CONDUIT_HANDOFF]\nReady"),
        });

        let effects = app
            .execute_handoff_session(AgentType::Claude, "opus".to_string())
            .expect("handoff should succeed");

        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::StartAgent {
                agent_type: AgentType::Codex,
                ..
            }
        )));

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        let expected_model = app.config().default_model_for(AgentType::Codex);
        assert_eq!(session.agent_type, AgentType::Codex);
        assert_eq!(session.model.as_deref(), Some(expected_model.as_str()));
    }

    #[test]
    fn test_handle_confirm_action_selecting_reasoning_sets_effort() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.agent_type = AgentType::Codex;
        }
        app.state
            .reasoning_selector_state
            .show(AgentType::Codex, None);
        app.state.reasoning_selector_state.insert_str("xhigh");
        app.state.input_mode = InputMode::SelectingReasoning;

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(!app.state.reasoning_selector_state.is_visible());
        assert!(effects.is_empty());

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        assert_eq!(session.reasoning_effort, Some(ReasoningEffort::XHigh));
    }

    #[test]
    fn test_handle_confirm_action_model_selector_wins_over_stale_mode() {
        let mut app = build_test_app_with_sessions(&[]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Claude, executable));
        app.config_mut()
            .set_enabled_providers(vec![AgentType::Claude]);

        app.state.pending_new_project_target = Some(NewProjectTarget::BaseDirDialog);
        app.state.model_picker_context = ModelPickerContext::OnboardingDefaultSelection;
        app.state
            .model_selector_state
            .set_allowed_providers(Some(vec![AgentType::Claude]));
        app.state.model_selector_state.show_with_title(
            None,
            DefaultModelSelection::default(),
            "Pick your default model".to_string(),
        );
        // Simulate stale mode mismatch observed in TUI.
        app.state.input_mode = InputMode::SelectingProviders;

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert!(effects.is_empty());
        assert_eq!(app.config().default_agent, AgentType::Claude);
        assert_eq!(app.config().default_model.as_deref(), Some("opus"));
        assert_eq!(app.state.input_mode, InputMode::SettingBaseDir);
        assert!(!app.state.model_selector_state.is_visible());
        assert!(app.state.pending_new_project_target.is_none());
    }

    #[test]
    fn test_handle_confirm_action_selecting_reasoning_blocked_after_session_started() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.turn_count = 1;
        }
        app.state
            .reasoning_selector_state
            .show(AgentType::Codex, None);
        app.state.reasoning_selector_state.insert_str("low");
        app.state.input_mode = InputMode::SelectingReasoning;

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert_eq!(app.state.input_mode, InputMode::SelectingReasoning);
        assert!(app.state.reasoning_selector_state.is_visible());
        assert!(effects.is_empty());

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        assert_eq!(session.reasoning_effort, None);
        let last = session
            .chat_view
            .messages()
            .last()
            .expect("expected error message");
        assert_eq!(last.role, MessageRole::Error);
        assert!(last
            .content
            .contains("Changing reasoning effort after a session has started"));
    }

    #[test]
    fn test_handle_confirm_action_blocks_cross_agent_switch_after_session_started() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Claude, executable));

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.turn_count = 1;
        }

        app.state
            .model_selector_state
            .show(None, DefaultModelSelection::default());
        app.state.model_selector_state.insert_str("Opus 4.6");
        app.state.input_mode = InputMode::SelectingModel;

        let mut effects = Vec::new();
        app.handle_confirm_action(&mut effects).unwrap();

        assert_eq!(app.state.input_mode, InputMode::SelectingModel);
        assert!(app.state.model_selector_state.is_visible());
        assert!(effects.is_empty());

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        assert_eq!(session.agent_type, AgentType::Codex);
        let last = session
            .chat_view
            .messages()
            .last()
            .expect("expected error message");
        assert_eq!(last.role, MessageRole::Error);
        assert!(last
            .content
            .contains("Switching agent type after a session has started"));
    }

    #[test]
    fn test_handle_status_bar_click_model_picker_respects_enabled_providers() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Codex, executable.clone()));
        assert!(app.tools_mut().update_tool(Tool::Claude, executable));
        app.config_mut()
            .set_enabled_providers(vec![AgentType::Codex]);

        let status_bar_area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 1,
        };
        let (x, y) = status_bar_model_click_position(&app, status_bar_area);

        let _ = app.handle_status_bar_click(x, y, status_bar_area);

        assert_eq!(app.state.input_mode, InputMode::SelectingModel);
        assert!(app.state.model_selector_state.is_visible());

        app.state.model_selector_state.insert_str("Opus 4.6");
        assert!(
            app.state.model_selector_state.selected_model().is_none(),
            "Claude model should be filtered out when only Codex is enabled"
        );
    }

    #[test]
    fn test_handle_model_selector_click_executes_pending_handoff() {
        let source_session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[source_session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Codex, executable.clone()));
        assert!(app.tools_mut().update_tool(Tool::Claude, executable));
        app.config_mut()
            .set_enabled_providers(vec![AgentType::Codex, AgentType::Claude]);

        app.state.pending_handoff_request = Some(PendingHandoffRequest {
            source_agent_type: AgentType::Codex,
            agent_mode: AgentMode::Build,
            reasoning_effort: None,
            workspace_id: None,
            working_dir: None,
            project_name: None,
            workspace_name: None,
            pr_number: None,
            handoff_prompt: Arc::from("[CONDUIT_HANDOFF]\nReady"),
        });
        app.state.input_mode = InputMode::SelectingModel;
        app.state.model_picker_context = ModelPickerContext::HandoffSelection;
        app.state
            .model_selector_state
            .set_allowed_providers(Some(vec![AgentType::Codex, AgentType::Claude]));
        app.state
            .model_selector_state
            .show(None, app.model_selector_defaults());
        app.state.model_selector_state.insert_str("Opus 4.6");

        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let dialog_width = 60u16.min(terminal_size.0.saturating_sub(4));
        let dialog_height = 18u16.min(terminal_size.1.saturating_sub(2));
        let dialog_x = (terminal_size.0.saturating_sub(dialog_width)) / 2;
        let dialog_y = (terminal_size.1.saturating_sub(dialog_height)) / 2;
        let inner = dialog_content_area(Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        });
        let x = inner.x + 1;
        let y = inner.y + 3;

        let effects = app.handle_model_selector_click(x, y);

        assert_eq!(app.state.input_mode, InputMode::Normal);
        assert!(!app.state.model_selector_state.is_visible());
        assert_eq!(
            app.state.model_picker_context,
            ModelPickerContext::SessionSelection
        );
        assert!(app.state.pending_handoff_request.is_none());
        assert_eq!(app.state.tab_manager.len(), 2);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::StartAgent {
                agent_type: AgentType::Claude,
                ..
            }
        )));

        let source = app
            .state
            .tab_manager
            .session(0)
            .expect("source session missing");
        assert_eq!(source.agent_type, AgentType::Codex);

        let target = app
            .state
            .tab_manager
            .active_session()
            .expect("target session missing");
        assert_eq!(target.agent_type, AgentType::Claude);
    }

    #[test]
    fn test_handle_model_selector_click_blocks_cross_agent_switch_after_session_started() {
        let session_id = Uuid::new_v4();
        let mut app = build_test_app_with_sessions(&[session_id]);
        let executable = std::env::current_exe().expect("test executable path");
        assert!(app.tools_mut().update_tool(Tool::Claude, executable));

        {
            let session = app
                .state
                .tab_manager
                .active_session_mut()
                .expect("session missing");
            session.turn_count = 1;
        }

        app.state
            .model_selector_state
            .show(None, DefaultModelSelection::default());
        app.state.model_selector_state.insert_str("Opus 4.6");
        app.state.input_mode = InputMode::SelectingModel;

        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let dialog_width = 60u16.min(terminal_size.0.saturating_sub(4));
        let dialog_height = 18u16.min(terminal_size.1.saturating_sub(2));
        let dialog_x = (terminal_size.0.saturating_sub(dialog_width)) / 2;
        let dialog_y = (terminal_size.1.saturating_sub(dialog_height)) / 2;
        let inner = dialog_content_area(Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        });
        let x = inner.x + 1;
        let y = inner.y + 3;

        assert!(app.handle_model_selector_click(x, y).is_empty());

        assert_eq!(app.state.input_mode, InputMode::SelectingModel);
        assert!(app.state.model_selector_state.is_visible());

        let session = app
            .state
            .tab_manager
            .active_session()
            .expect("session missing");
        assert_eq!(session.agent_type, AgentType::Codex);
        let last = session
            .chat_view
            .messages()
            .last()
            .expect("expected error message");
        assert_eq!(last.role, MessageRole::Error);
        assert!(last
            .content
            .contains("Switching agent type after a session has started"));
    }
}
