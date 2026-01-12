use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Component, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use chrono::Utc;
use crossterm::{
    event::{self, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    execute,
    terminal::{enable_raw_mode, EnterAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
    Frame, Terminal,
};
use sha2::{Digest, Sha256};
use tempfile::Builder;
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use crate::agent::{
    load_claude_history_with_debug, load_codex_history_with_debug, AgentEvent, AgentMode,
    AgentRunner, AgentStartConfig, AgentType, ClaudeCodeRunner, CodexCliRunner, HistoryDebugEntry,
    MessageDisplay, ModelRegistry, SessionId,
};
use crate::config::{
    parse_action, parse_key_notation, Config, KeyCombo, KeyContext, COMMAND_NAMES,
};
use crate::data::{
    AppStateStore, Database, ForkSeed, ForkSeedStore, QueuedImageAttachment, QueuedMessage,
    QueuedMessageMode, Repository, RepositoryStore, SessionTab, SessionTabStore, WorkspaceStore,
};
use crate::git::{PrManager, PrStatus, WorktreeManager};
use crate::ui::action::Action;
use crate::ui::app_state::{AppState, PendingForkRequest, ScrollDragTarget, SelectionDragTarget};
use crate::ui::clipboard_paste::paste_image_to_temp_png;
use crate::ui::components::{
    bg_highlight, scrollbar_offset_from_point, text_muted, text_primary, AddRepoDialog,
    AgentSelector, BaseDirDialog, ChatMessage, CommandPalette, ConfirmationContext,
    ConfirmationDialog, ConfirmationType, DefaultModelSelection, ErrorDialog, EventDirection,
    GlobalFooter, HelpDialog, MessageRole, MissingToolDialog, ModelSelector, ProcessingState,
    ProjectPicker, RawEventsClick, RawEventsScrollbarMetrics, ScrollbarMetrics, SessionHeader,
    SessionImportPicker, Sidebar, SidebarData, TabBar, TabBarHitTarget, ThemePicker,
    SIDEBAR_HEADER_ROWS,
};
use crate::ui::effect::Effect;
use crate::ui::events::{
    AppEvent, ForkWorkspaceCreated, InputMode, RemoveProjectResult, TitleGeneratedResult, ViewMode,
    WorkspaceArchived, WorkspaceCreated,
};
use crate::ui::session::AgentSession;
use crate::ui::terminal_guard::TerminalGuard;
use crate::util::ToolAvailability;

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
// 20s allows slow CLI agents to shut down on congested machines without UI hangs.
const AGENT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);
// 500ms grace keeps UI responsive while giving SIGTERM a brief chance to exit.
const AGENT_TERMINATION_GRACE: Duration = Duration::from_millis(500);
// 50ms polling keeps wait loops short without a busy spin.
const AGENT_TERMINATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Main application state
pub struct App {
    /// Application configuration
    config: Config,
    /// Tool availability (git, gh, claude, codex)
    tools: ToolAvailability,
    /// In-memory UI state
    state: AppState,
    /// Agent runners
    claude_runner: Arc<ClaudeCodeRunner>,
    codex_runner: Arc<CodexCliRunner>,
    /// Event channel sender
    event_tx: mpsc::UnboundedSender<AppEvent>,
    /// Event channel receiver
    event_rx: mpsc::UnboundedReceiver<AppEvent>,
    /// Repository DAO
    repo_dao: Option<RepositoryStore>,
    /// Workspace DAO
    workspace_dao: Option<WorkspaceStore>,
    /// App state DAO (for persisting app settings)
    app_state_dao: Option<AppStateStore>,
    /// Session tab DAO (for persisting open tabs)
    session_tab_dao: Option<SessionTabStore>,
    /// Fork seed DAO (for persisting fork metadata)
    fork_seed_dao: Option<ForkSeedStore>,
    /// Worktree manager
    worktree_manager: WorktreeManager,
    /// Background git/PR status tracker
    git_tracker: Option<crate::ui::git_tracker::GitTrackerHandle>,
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

        // Initialize database and DAOs
        let (repo_dao, workspace_dao, app_state_dao, session_tab_dao, fork_seed_dao) =
            match Database::open_default() {
                Ok(db) => {
                    let repo_dao = RepositoryStore::new(db.connection());
                    let workspace_dao = WorkspaceStore::new(db.connection());
                    let app_state_dao = AppStateStore::new(db.connection());
                    let session_tab_dao = SessionTabStore::new(db.connection());
                    let fork_seed_dao = ForkSeedStore::new(db.connection());
                    (
                        Some(repo_dao),
                        Some(workspace_dao),
                        Some(app_state_dao),
                        Some(session_tab_dao),
                        Some(fork_seed_dao),
                    )
                }
                Err(e) => {
                    eprintln!("Warning: Failed to open database: {}", e);
                    (None, None, None, None, None)
                }
            };

        // Migrate old worktrees folder to workspaces (one-time migration)
        crate::util::migrate_worktrees_to_workspaces();

        // Initialize worktree manager with managed directory (~/.conduit/workspaces)
        let worktree_manager = WorktreeManager::with_managed_dir(crate::util::workspaces_dir());

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

        // Create runners with configured paths if available
        let claude_runner = match tools.get_path(crate::util::Tool::Claude) {
            Some(path) => Arc::new(ClaudeCodeRunner::with_path(path.clone())),
            None => Arc::new(ClaudeCodeRunner::new()),
        };
        let codex_runner = match tools.get_path(crate::util::Tool::Codex) {
            Some(path) => Arc::new(CodexCliRunner::with_path(path.clone())),
            None => Arc::new(CodexCliRunner::new()),
        };

        let mut app = Self {
            config: config.clone(),
            tools,
            state: AppState::new(config.max_tabs),
            claude_runner,
            codex_runner,
            event_tx,
            event_rx,
            repo_dao,
            workspace_dao,
            app_state_dao,
            session_tab_dao,
            fork_seed_dao,
            worktree_manager,
            git_tracker,
        };

        // Update agent selector based on available tools
        app.state
            .agent_selector_state
            .update_available_agents(&app.tools);

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
            .repo_dao
            .as_ref()
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
        let Some(session_tab_dao) = self.session_tab_dao.clone() else {
            tracing::warn!("Session tab DAO unavailable; skipping session restore");
            return;
        };
        let Some(app_state_dao) = self.app_state_dao.clone() else {
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
            if !self.tools.is_available(required_tool) {
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
            session.workspace_id = tab.workspace_id;
            session.model = tab.model;
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
                if let Some(workspace_dao) = &self.workspace_dao {
                    if let Ok(Some(workspace)) = workspace_dao.get_by_id(workspace_id) {
                        session.working_dir = Some(workspace.path);
                        session.workspace_name = Some(workspace.name.clone());

                        // Look up repository for project name
                        if let Some(repo_dao) = &self.repo_dao {
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
                session.agent_session_id = Some(session_id.clone());

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

        self.state.sidebar_data = SidebarData::new();

        let Some(repo_dao) = &self.repo_dao else {
            return;
        };
        let Some(workspace_dao) = &self.workspace_dao else {
            return;
        };

        // Load all repositories
        if let Ok(repos) = repo_dao.get_all() {
            for repo in repos {
                // Load workspaces for this repository
                if let Ok(workspaces) = workspace_dao.get_by_repository(repo.id) {
                    let workspace_info: Vec<_> = workspaces
                        .into_iter()
                        .map(|ws| (ws.id, ws.name, ws.branch))
                        .collect();
                    self.state
                        .sidebar_data
                        .add_repository(repo.id, &repo.name, workspace_info);
                }
            }
        }

        // Restore expansion state
        for repo_id in expanded_repos {
            self.state.sidebar_data.expand_repo(repo_id);
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
    ) {
        let Some(session_tab_dao) = session_tab_dao else {
            tracing::warn!("Session tab DAO unavailable; skipping session persistence");
            return;
        };
        let Some(app_state_dao) = app_state_dao else {
            tracing::warn!("App state DAO unavailable; skipping session persistence");
            return;
        };

        tracing::info!(
            tab_count = snapshot.tabs.len(),
            active_tab_index = snapshot.active_tab_index,
            "Persisting session state"
        );

        if let Err(e) = session_tab_dao.clear_all() {
            eprintln!("Warning: Failed to clear session tabs: {}", e);
            tracing::warn!(error = %e, "Failed to clear saved session tabs");
            return;
        }

        for tab in &snapshot.tabs {
            if let Err(e) = session_tab_dao.create(tab) {
                eprintln!("Warning: Failed to save session tab: {}", e);
                tracing::warn!(error = %e, tab_index = tab.tab_index, "Failed to save session tab");
            }
        }

        if let Err(e) =
            app_state_dao.set("active_tab_index", &snapshot.active_tab_index.to_string())
        {
            eprintln!("Warning: Failed to save active tab index: {}", e);
        }

        if let Err(e) = app_state_dao.set(
            "sidebar_visible",
            if snapshot.sidebar_visible {
                "true"
            } else {
                "false"
            },
        ) {
            eprintln!("Warning: Failed to save sidebar visibility: {}", e);
        }

        if let Err(e) = app_state_dao.set(
            "tree_selected_index",
            &snapshot.tree_selected_index.to_string(),
        ) {
            eprintln!("Warning: Failed to save tree selection: {}", e);
        }

        let collapsed_ids: Vec<String> = snapshot
            .collapsed_repo_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        if let Err(e) = app_state_dao.set("tree_collapsed_repos", &collapsed_ids.join(",")) {
            eprintln!("Warning: Failed to save collapsed repos: {}", e);
            tracing::warn!(error = %e, "Failed to save collapsed repos");
        }

        tracing::info!("Session state persistence complete");
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
        Self::persist_session_state(
            snapshot,
            self.session_tab_dao.clone(),
            self.app_state_dao.clone(),
        );
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<()> {
        loop {
            let frame_start = Instant::now();

            // Draw UI with timing
            let draw_start = Instant::now();
            terminal.draw(|f| self.draw(f))?;
            let draw_end = Instant::now();
            self.state.metrics.draw_time = draw_end.duration_since(draw_start);
            self.state.metrics.on_draw_end(draw_end);

            // Calculate remaining sleep time to hit ~60 FPS target
            // Account for draw time already spent this frame
            let target_frame = Duration::from_millis(16);
            let elapsed = frame_start.elapsed();
            let sleep_duration = target_frame.saturating_sub(elapsed);

            tokio::select! {
                // Terminal input events + tick
                _ = tokio::time::sleep(sleep_duration) => {
                    // Measure event processing time (after sleep)
                    let event_start = Instant::now();

                    // Handle keyboard and mouse input
                    let mut pending_scroll_up = 0usize;
                    let mut pending_scroll_down = 0usize;

                    while event::poll(Duration::from_millis(0))? {
                        match event::read()? {
                            Event::Key(key) => {
                                self.flush_scroll_deltas(&mut pending_scroll_up, &mut pending_scroll_down);
                                self.dispatch_event(AppEvent::Input(Event::Key(key)), terminal, guard)
                                    .await?;
                            }
                            Event::Mouse(mouse) => {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        if self.handle_tab_bar_wheel(
                                            mouse.column,
                                            mouse.row,
                                            true,
                                        ) {
                                            continue;
                                        }
                                        if self.should_route_scroll_to_chat() {
                                            self.record_scroll(1);
                                        }
                                        pending_scroll_up = pending_scroll_up.saturating_add(1);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        if self.handle_tab_bar_wheel(
                                            mouse.column,
                                            mouse.row,
                                            false,
                                        ) {
                                            continue;
                                        }
                                        if self.should_route_scroll_to_chat() {
                                            self.record_scroll(1);
                                        }
                                        pending_scroll_down = pending_scroll_down.saturating_add(1);
                                    }
                                    _ => {
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
                            _ => {
                                self.flush_scroll_deltas(&mut pending_scroll_up, &mut pending_scroll_down);
                            }
                        }
                    }

                    self.flush_scroll_deltas(&mut pending_scroll_up, &mut pending_scroll_down);

                    self.handle_tick();

                    self.state.metrics.event_time = event_start.elapsed();
                }

                // App events from channel
                Some(event) = self.event_rx.recv() => {
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
            AppEvent::Input(input) => self.handle_input_event(input, terminal, guard).await?,
            AppEvent::Tick => {
                self.handle_tick();
                Vec::new()
            }
            _ => self.handle_app_event(event).await?,
        };

        self.run_effects(effects).await
    }

    async fn handle_input_event(
        &mut self,
        input: Event,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        match input {
            Event::Key(key) => self.handle_key_event(key, terminal, guard).await,
            Event::Mouse(mouse) => self.handle_mouse_event(mouse, terminal, guard).await,
            Event::Paste(text) => {
                self.handle_paste_input(text);
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn handle_tick(&mut self) {
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
                }
            }
        }

        // Clear expired timed footer messages
        self.state.clear_expired_footer_message();

        self.state.theme_picker_state.tick();
        let can_show_picker_error = self.state.theme_picker_state.is_visible()
            || (self.state.footer_message.is_none()
                && self.state.footer_message_expires_at.is_none());
        if can_show_picker_error {
            if let Some(error) = self.state.theme_picker_state.take_error() {
                self.state
                    .set_timed_footer_message(error, Duration::from_secs(5));
            }
        }

        // Tick other animations every 6 frames (~100ms)
        if !self.state.tick_count.is_multiple_of(6) {
            return;
        }

        // Advance spinner frame for PR processing indicator
        self.state.spinner_frame = self.state.spinner_frame.wrapping_add(1);

        // Tick confirmation dialog spinner (for loading state)
        self.state.confirmation_dialog_state.tick();

        // Tick session import spinner (for loading state)
        self.state.session_import_state.tick();

        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.tick();
        }
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

    async fn handle_key_event(
        &mut self,
        key: event::KeyEvent,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        // Special handling for modes that bypass normal key processing
        if self.state.input_mode == InputMode::RemovingProject {
            // Ignore all input while removing project
            return Ok(Vec::new());
        }

        // Handle Ctrl+C with double-press detection (global)
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            tracing::debug!("Ctrl+C detected, calling handle_ctrl_c_press");
            let effects = self.handle_ctrl_c_press();
            return Ok(effects);
        }

        // Handle Esc with double-press detection (only when no dialog active and in normal mode)
        if key.code == KeyCode::Esc
            && !self.has_active_dialog()
            && !self.state.show_first_time_splash
            && matches!(
                self.state.input_mode,
                InputMode::Normal | InputMode::Scrolling
            )
        {
            self.handle_esc_press();
            return Ok(Vec::new());
        }

        // First-time splash screen handling (only when no dialogs are visible)
        if self.state.show_first_time_splash
            && !self.state.command_palette_state.is_visible()
            && !self.state.base_dir_dialog_state.is_visible()
            && !self.state.project_picker_state.is_visible()
            && !self.state.add_repo_dialog_state.is_visible()
            && self.state.input_mode != InputMode::SelectingAgent
            && self.state.input_mode != InputMode::ShowingError
        {
            // Handle Ctrl+P to open command palette
            let is_ctrl_p = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')))
                || matches!(key.code, KeyCode::Char('\x10')); // ASCII 16 = Ctrl+P
            if is_ctrl_p {
                return self
                    .execute_action(Action::OpenCommandPalette, terminal, guard)
                    .await;
            }
            // Handle Ctrl+N to add new project
            let is_ctrl_n = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')))
                || matches!(key.code, KeyCode::Char('\x0e'));
            if is_ctrl_n || (key.modifiers.is_empty() && key.code == KeyCode::Enter) {
                return self
                    .execute_action(Action::NewProject, terminal, guard)
                    .await;
            }
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        return self.execute_action(Action::Quit, terminal, guard).await;
                    }
                    _ => {}
                }
            }
        }

        // Handle Ctrl+N and Ctrl+P when tabs are empty (works from any input mode)
        if self.state.tab_manager.is_empty() && !self.state.command_palette_state.is_visible() {
            let is_ctrl_n = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')))
                || matches!(key.code, KeyCode::Char('\x0e')); // ASCII 14 = Ctrl+N

            if is_ctrl_n {
                return self
                    .execute_action(Action::NewProject, terminal, guard)
                    .await;
            }

            // Handle Ctrl+P for command palette
            let is_ctrl_p = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')))
                || matches!(key.code, KeyCode::Char('\x10')); // ASCII 16 = Ctrl+P

            if is_ctrl_p {
                return self
                    .execute_action(Action::OpenCommandPalette, terminal, guard)
                    .await;
            }
        }

        // Image paste: Ctrl+V (Linux/Windows) or Alt+V (macOS terminals report Cmd as Alt)
        // Match either modifier independently (Cmd often maps to Alt in terminal emulators)
        if self.state.input_mode == InputMode::Normal
            && (key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT))
            && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'v'))
        {
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                match paste_image_to_temp_png() {
                    Ok((path, info)) => {
                        session
                            .input_box
                            .attach_image(path, info.width, info.height);
                    }
                    Err(err) => {
                        let display = MessageDisplay::Error {
                            content: format!("Failed to paste image: {err}"),
                        };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
            }
            return Ok(Vec::new());
        }

        // Global command mode trigger - ':' from most modes enters command mode
        // Only trigger when input box is empty (so pasting "hello:world" doesn't activate command mode)
        if key.code == KeyCode::Char(':')
            && key.modifiers.is_empty()
            && !matches!(
                self.state.input_mode,
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
                    | InputMode::SelectingTheme
            )
        {
            // Only enter command mode if the input box is empty
            let input_is_empty = self
                .state
                .tab_manager
                .active_session()
                .map(|s| s.input_box.input().is_empty())
                .unwrap_or(true);

            if input_is_empty {
                self.state.command_buffer.clear();
                self.state.input_mode = InputMode::Command;
                return Ok(Vec::new());
            }
        }

        // Get the current context from input mode and view mode
        let context = KeyContext::from_input_mode(self.state.input_mode, self.state.view_mode);

        // Text input (typing characters) handled specially
        if self.should_handle_as_text_input(&key, context) {
            self.handle_text_input(key);
            return Ok(Vec::new());
        }

        // Convert key event to KeyCombo for lookup
        let key_combo = KeyCombo::from_key_event(&key);

        // Look up action in config (context-specific first, then global)
        if let Some(action) = self.config.keybindings.get_action(&key_combo, context) {
            return self.execute_action(action.clone(), terminal, guard).await;
        }

        Ok(Vec::new())
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
            Action::Quit => {
                self.state.should_quit = true;
                effects.push(Effect::SaveSessionState);
            }
            Action::ToggleSidebar => {
                self.state.sidebar_state.toggle();
                if self.state.sidebar_state.visible {
                    self.state.sidebar_state.set_focused(true);
                    self.state.input_mode = InputMode::SidebarNavigation;
                    // Focus on the current tab's workspace if it has one
                    if let Some(session) = self.state.tab_manager.active_session() {
                        if let Some(workspace_id) = session.workspace_id {
                            if let Some(index) =
                                self.state.sidebar_data.focus_workspace(workspace_id)
                            {
                                self.state.sidebar_state.tree_state.selected = index;
                            }
                        }
                    }
                } else {
                    self.state.sidebar_state.set_focused(false);
                    self.state.input_mode = InputMode::Normal;
                }
            }
            Action::NewProject => {
                let base_dir = self
                    .app_state_dao
                    .as_ref()
                    .and_then(|dao| dao.get("projects_base_dir").ok().flatten());

                self.state.close_overlays();
                if let Some(base_dir_str) = base_dir {
                    let base_path = if base_dir_str.starts_with('~') {
                        dirs::home_dir()
                            .map(|h| h.join(base_dir_str[1..].trim_start_matches('/')))
                            .unwrap_or_else(|| PathBuf::from(&base_dir_str))
                    } else {
                        PathBuf::from(&base_dir_str)
                    };
                    self.state.project_picker_state.show(base_path);
                    self.state.input_mode = InputMode::PickingProject;
                } else {
                    self.state.base_dir_dialog_state.show();
                    self.state.input_mode = InputMode::SettingBaseDir;
                }
            }
            Action::NewWorkspaceUnderCursor => {
                use crate::ui::components::{ActionType, NodeType};

                let sidebar_focused = self.state.sidebar_state.focused;
                let repo_id_from_sidebar = if sidebar_focused {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    self.state
                        .sidebar_data
                        .get_at(selected)
                        .and_then(|node| match node.node_type {
                            NodeType::Repository => Some(node.id),
                            NodeType::Workspace => node.parent_id,
                            NodeType::Action(ActionType::NewWorkspace) => node.parent_id,
                        })
                } else {
                    None
                };

                let repo_id_from_tab = if sidebar_focused {
                    None
                } else {
                    let session = self.state.tab_manager.active_session();
                    let workspace_id = session.and_then(|s| s.workspace_id);
                    match (workspace_id, self.workspace_dao.as_ref()) {
                        (Some(workspace_id), Some(workspace_dao)) => {
                            match workspace_dao.get_by_id(workspace_id) {
                                Ok(Some(workspace)) => Some(workspace.repository_id),
                                Ok(None) => {
                                    tracing::error!(
                                        workspace_id = %workspace_id,
                                        "Workspace not found for active tab"
                                    );
                                    None
                                }
                                Err(err) => {
                                    tracing::error!(
                                        workspace_id = %workspace_id,
                                        error = %err,
                                        "Failed to load workspace for active tab"
                                    );
                                    None
                                }
                            }
                        }
                        _ => None,
                    }
                };

                let repo_id = if sidebar_focused {
                    repo_id_from_sidebar
                } else {
                    repo_id_from_tab
                };

                if let Some(repo_id) = repo_id {
                    effects.push(self.start_workspace_creation(repo_id));
                } else {
                    self.state.set_timed_footer_message(
                        "No project selected to create a workspace".to_string(),
                        Duration::from_secs(5),
                    );
                }
            }
            Action::OpenPr => {
                if let Some(effect) = self.handle_pr_action() {
                    effects.push(effect);
                }
            }
            Action::ForkSession => {
                self.initiate_fork_session();
            }
            Action::InterruptAgent => {
                self.interrupt_agent();
            }
            Action::ToggleViewMode => {
                self.state.view_mode = match self.state.view_mode {
                    ViewMode::Chat => ViewMode::RawEvents,
                    ViewMode::RawEvents => ViewMode::Chat,
                };
            }
            Action::ShowModelSelector => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    let model = session.model.clone();
                    self.state.close_overlays();
                    let defaults = self.model_selector_defaults();
                    self.state.model_selector_state.show(model, defaults);
                    self.state.input_mode = InputMode::SelectingModel;
                }
            }
            Action::ShowThemePicker => {
                self.state.close_overlays();
                self.state
                    .theme_picker_state
                    .show(self.config.theme_path.as_deref());
                self.state.input_mode = InputMode::SelectingTheme;
            }
            Action::OpenSessionImport => {
                self.state.close_overlays();
                self.state.session_import_state.show();
                self.state.input_mode = InputMode::ImportingSession;
                // Trigger session discovery
                effects.push(Effect::DiscoverSessions);
            }
            Action::ImportSession => {
                if self.state.input_mode == InputMode::ImportingSession {
                    if let Some(session) =
                        self.state.session_import_state.selected_session().cloned()
                    {
                        self.state.session_import_state.hide();
                        self.state.input_mode = InputMode::Normal;
                        effects.push(Effect::ImportSession(session));
                    }
                }
            }
            Action::CycleImportFilter => {
                if self.state.input_mode == InputMode::ImportingSession {
                    self.state.session_import_state.cycle_filter();
                }
            }
            Action::ToggleMetrics => {
                self.state.show_metrics = !self.state.show_metrics;
                // Uncomment to test spinner animation smoothness with Alt+P:
                // if self.state.show_metrics {
                //     self.state
                //         .start_footer_spinner(Some("Testing spinner...".to_string()));
                // } else {
                //     self.state.stop_footer_spinner();
                // }
            }
            Action::ToggleAgentMode => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    // Only toggle when agent supports plan mode
                    if session.capabilities.supports_plan_mode {
                        session.agent_mode = session.agent_mode.toggle();
                        session.update_status();
                    }
                }
            }
            Action::DumpDebugState => {
                effects.push(Effect::DumpDebugState);
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
            Action::CopyWorkspacePath => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    if let Some(working_dir) = &session.working_dir {
                        let path_str = working_dir.display().to_string();
                        effects.push(Effect::CopyToClipboard(path_str.clone()));
                        self.state.set_timed_footer_message(
                            format!("Copied: {}", path_str),
                            Duration::from_secs(10),
                        );
                    }
                }
            }
            Action::CopySelection => {
                let mut copied = false;
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.input_box.has_selection() {
                        if let Some(text) = session.input_box.selected_text() {
                            copied = true;
                            effects.push(Effect::CopyToClipboard(text));
                            if self.config.selection.clear_selection_after_copy {
                                Self::clear_selection_for_target(
                                    session,
                                    SelectionDragTarget::Input,
                                );
                            }
                        }
                    } else if session.chat_view.has_selection() {
                        if let Some(text) = session.chat_view.copy_selection() {
                            copied = true;
                            effects.push(Effect::CopyToClipboard(text));
                            if self.config.selection.clear_selection_after_copy {
                                Self::clear_selection_for_target(
                                    session,
                                    SelectionDragTarget::Chat,
                                );
                            }
                        }
                    }
                }

                if copied {
                    self.state.set_timed_footer_message(
                        "Copied selection".to_string(),
                        Duration::from_secs(5),
                    );
                } else {
                    self.state.set_timed_footer_message(
                        "No selection to copy".to_string(),
                        Duration::from_secs(3),
                    );
                }
            }

            // ========== Tab Management ==========
            Action::CloseTab => {
                let active = self.state.tab_manager.active_index();
                self.stop_agent_for_tab(active);
                self.state.tab_manager.close_tab(active);
                if self.state.tab_manager.is_empty() {
                    self.state.stop_footer_spinner();
                    self.state.sidebar_state.visible = true;
                    self.state.input_mode = InputMode::SidebarNavigation;
                } else {
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
                effects.push(Effect::SaveSessionState);
            }
            Action::NextTab => {
                // Include sidebar in tab cycle when visible
                if self.state.input_mode == InputMode::SidebarNavigation {
                    // From sidebar, go to first tab
                    if !self.state.tab_manager.is_empty() {
                        self.state.tab_manager.switch_to(0);
                        self.state.sidebar_state.set_focused(false);
                        self.state.input_mode = InputMode::Normal;
                        self.sync_sidebar_to_active_tab();
                    }
                } else if self.state.sidebar_state.visible {
                    // Check if on last tab - if so, go to sidebar
                    let current = self.state.tab_manager.active_index();
                    let count = self.state.tab_manager.len();
                    if count > 0 && current == count - 1 {
                        // On last tab, go to sidebar
                        self.state.sidebar_state.set_focused(true);
                        self.state.input_mode = InputMode::SidebarNavigation;
                    } else {
                        self.state.tab_manager.next_tab();
                        self.sync_sidebar_to_active_tab();
                        self.sync_footer_spinner();
                    }
                } else {
                    self.state.tab_manager.next_tab();
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
            }
            Action::PrevTab => {
                // Include sidebar in tab cycle when visible
                if self.state.input_mode == InputMode::SidebarNavigation {
                    // From sidebar, go to last tab
                    let count = self.state.tab_manager.len();
                    if count > 0 {
                        self.state.tab_manager.switch_to(count - 1);
                        self.state.sidebar_state.set_focused(false);
                        self.state.input_mode = InputMode::Normal;
                        self.sync_sidebar_to_active_tab();
                        self.sync_footer_spinner();
                    }
                } else if self.state.sidebar_state.visible {
                    // Check if on first tab - if so, go to sidebar
                    let current = self.state.tab_manager.active_index();
                    if current == 0 {
                        // On first tab, go to sidebar
                        self.state.sidebar_state.set_focused(true);
                        self.state.input_mode = InputMode::SidebarNavigation;
                    } else {
                        self.state.tab_manager.prev_tab();
                        self.sync_sidebar_to_active_tab();
                        self.sync_footer_spinner();
                    }
                } else {
                    self.state.tab_manager.prev_tab();
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
            }
            Action::SwitchToTab(n) => {
                if n > 0 {
                    self.state.tab_manager.switch_to((n - 1) as usize);
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
            }

            // ========== Chat Scrolling ==========
            Action::ScrollUp(n) => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_up(n as usize);
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_up(n as usize);
                    }
                    self.record_scroll(n as usize);
                }
            }
            Action::ScrollDown(n) => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_down(n as usize);
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_down(n as usize);
                    }
                    self.record_scroll(n as usize);
                }
            }
            Action::ScrollPageUp => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.page_up();
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_up(10);
                    }
                    self.record_scroll(10);
                }
            }
            Action::ScrollPageDown => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.page_down();
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_down(10);
                    }
                    self.record_scroll(10);
                }
            }
            Action::ScrollToTop => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_to_top();
                }
            }
            Action::ScrollToBottom => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_to_bottom();
                }
            }
            Action::ScrollPrevUserMessage => {
                if let (Some(session), Some(chat_area)) = (
                    self.state.tab_manager.active_session_mut(),
                    self.state.chat_area,
                ) {
                    if let Some(content) =
                        crate::ui::components::ChatView::content_area_for(chat_area)
                    {
                        let mut extra_len = 0usize;
                        if session.is_processing {
                            extra_len += 1;
                        }
                        if let Some(queue_lines) =
                            Self::build_queue_lines(session, chat_area.width, self.state.input_mode)
                        {
                            extra_len += queue_lines.len();
                        }
                        if extra_len > 0 {
                            extra_len += 1; // spacing line after extras
                        }

                        session.chat_view.scroll_to_prev_user_message(
                            content.width,
                            content.height as usize,
                            extra_len,
                        );
                    }
                }
            }
            Action::ScrollNextUserMessage => {
                if let (Some(session), Some(chat_area)) = (
                    self.state.tab_manager.active_session_mut(),
                    self.state.chat_area,
                ) {
                    if let Some(content) =
                        crate::ui::components::ChatView::content_area_for(chat_area)
                    {
                        let mut extra_len = 0usize;
                        if session.is_processing {
                            extra_len += 1;
                        }
                        if let Some(queue_lines) =
                            Self::build_queue_lines(session, chat_area.width, self.state.input_mode)
                        {
                            extra_len += queue_lines.len();
                        }
                        if extra_len > 0 {
                            extra_len += 1; // spacing line after extras
                        }

                        session.chat_view.scroll_to_next_user_message(
                            content.width,
                            content.height as usize,
                            extra_len,
                        );
                    }
                }
            }

            // ========== Input Box Editing ==========
            Action::InsertNewline => {
                // Don't insert newlines in help dialog, command mode, or sidebar navigation
                if self.state.input_mode != InputMode::ShowingHelp
                    && self.state.input_mode != InputMode::Command
                    && self.state.input_mode != InputMode::SidebarNavigation
                {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.input_box.insert_newline();
                    }
                }
            }
            Action::Backspace => {
                match self.state.input_mode {
                    InputMode::Command => {
                        if self.state.command_buffer.is_empty() {
                            // Exit command mode if buffer is empty
                            self.state.input_mode = InputMode::Normal;
                        } else {
                            self.state.command_buffer.pop();
                        }
                    }
                    InputMode::ShowingHelp => {
                        self.state.help_dialog_state.delete_char();
                    }
                    InputMode::ImportingSession => {
                        self.state.session_import_state.delete_char();
                    }
                    InputMode::PickingProject => {
                        self.state.project_picker_state.delete_char();
                    }
                    InputMode::CommandPalette => {
                        self.state.command_palette_state.delete_char();
                    }
                    InputMode::SettingBaseDir => {
                        self.state.base_dir_dialog_state.delete_char();
                    }
                    InputMode::MissingTool => {
                        self.state.missing_tool_dialog_state.backspace();
                    }
                    InputMode::SelectingTheme => {
                        self.state.theme_picker_state.backspace();
                    }
                    InputMode::SelectingModel => {
                        self.state.model_selector_state.delete_char();
                    }
                    _ => {
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            session.input_box.backspace();
                        }
                    }
                }
            }
            Action::Delete => {
                if self.state.input_mode == InputMode::MissingTool {
                    self.state.missing_tool_dialog_state.delete();
                } else if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.delete();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.delete_forward();
                } else if self.state.input_mode == InputMode::SettingBaseDir {
                    self.state.base_dir_dialog_state.delete_forward();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete();
                }
            }
            Action::DeleteWordBack => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete_word_back();
                }
            }
            Action::DeleteWordForward => {
                // TODO: implement delete_word_forward in InputBox
            }
            Action::DeleteToStart => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete_to_start();
                }
            }
            Action::DeleteToEnd => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete_to_end();
                }
            }
            Action::MoveCursorLeft => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_left();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_left();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_left();
                }
            }
            Action::MoveCursorRight => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_right();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_right();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_right();
                }
            }
            Action::MoveCursorStart => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_to_start();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_start();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_start();
                }
            }
            Action::MoveCursorEnd => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_to_end();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_end();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_end();
                }
            }
            Action::MoveWordLeft => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_word_left();
                }
            }
            Action::MoveWordRight => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_word_right();
                }
            }
            Action::MoveCursorUp => {
                let mut dequeued = None;
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.move_up() && session.input_box.is_cursor_on_first_line() {
                        if session.input_box.is_empty() && !session.queued_messages.is_empty() {
                            dequeued = session.dequeue_last();
                        } else {
                            session.input_box.history_prev();
                        }
                    }
                }
                if let Some(message) = dequeued {
                    self.restore_queued_to_input(message);
                }
            }
            Action::MoveCursorDown => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.move_down() && session.input_box.is_cursor_on_last_line()
                    {
                        session.input_box.history_next();
                    }
                }
            }
            Action::HistoryPrev => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.history_prev();
                }
            }
            Action::HistoryNext => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.history_next();
                }
            }
            Action::Submit => {
                effects
                    .extend(self.handle_submit_action(crate::data::QueuedMessageMode::FollowUp)?);
            }
            Action::SubmitSteer => {
                effects.extend(self.handle_submit_action(crate::data::QueuedMessageMode::Steer)?);
            }
            Action::OpenQueueEditor => {
                self.open_queue_editor();
            }
            Action::CloseQueueEditor => {
                self.close_queue_editor();
            }
            Action::QueueMoveUp => {
                if self.state.input_mode == InputMode::QueueEditing {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.move_queue_up();
                        Self::clamp_queue_selection(session);
                    }
                }
            }
            Action::QueueMoveDown => {
                if self.state.input_mode == InputMode::QueueEditing {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.move_queue_down();
                        Self::clamp_queue_selection(session);
                    }
                }
            }
            Action::QueueEdit => {
                if self.state.input_mode == InputMode::QueueEditing {
                    let mut message = None;
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        message = session.dequeue_selected();
                        Self::clamp_queue_selection(session);
                    }
                    if let Some(msg) = message {
                        self.close_queue_editor();
                        self.restore_queued_to_input(msg);
                    } else {
                        self.close_queue_editor();
                    }
                }
            }
            Action::QueueDelete => {
                if self.state.input_mode == InputMode::QueueEditing {
                    let mut should_close = false;
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if let Some(idx) = session.queue_selection {
                            session.remove_queue_at(idx);
                        }
                        Self::clamp_queue_selection(session);
                        if session.queued_messages.is_empty() {
                            should_close = true;
                        }
                    }
                    if should_close {
                        self.close_queue_editor();
                    }
                }
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
            Action::SelectNext => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    let visible_count = self.state.sidebar_data.visible_nodes().len();
                    self.state
                        .sidebar_state
                        .tree_state
                        .select_next(visible_count);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.select_next();
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.select_next();
                }
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.select_next();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.select_next();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.select_next();
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.select_next();
                }
                InputMode::QueueEditing => {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.select_queue_next();
                    }
                }
                _ => {}
            },
            Action::SelectPrev => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    let visible_count = self.state.sidebar_data.visible_nodes().len();
                    self.state
                        .sidebar_state
                        .tree_state
                        .select_previous(visible_count);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.select_previous();
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.select_prev();
                }
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.select_previous();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.select_prev();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.select_prev();
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.select_prev();
                }
                InputMode::QueueEditing => {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.select_queue_prev();
                    }
                }
                _ => {}
            },
            Action::SelectPageDown => {
                if self.state.input_mode == InputMode::PickingProject {
                    self.state.project_picker_state.page_down();
                } else if self.state.input_mode == InputMode::ImportingSession {
                    self.state.session_import_state.page_down();
                }
            }
            Action::SelectPageUp => {
                if self.state.input_mode == InputMode::PickingProject {
                    self.state.project_picker_state.page_up();
                } else if self.state.input_mode == InputMode::ImportingSession {
                    self.state.session_import_state.page_up();
                }
            }
            Action::Confirm => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    if let Some(node) = self.state.sidebar_data.get_at(selected) {
                        use crate::ui::components::{ActionType, NodeType};
                        match node.node_type {
                            NodeType::Action(ActionType::NewWorkspace) => {
                                if let Some(parent_id) = node.parent_id {
                                    effects.push(self.start_workspace_creation(parent_id));
                                }
                            }
                            NodeType::Workspace => {
                                self.open_workspace(node.id);
                                self.state.input_mode = InputMode::Normal;
                                self.state.sidebar_state.set_focused(false);
                            }
                            NodeType::Repository => {
                                self.state.sidebar_data.toggle_at(selected);
                            }
                        }
                    }
                }
                InputMode::SelectingModel => {
                    if let Some(model) = self.state.model_selector_state.selected_model() {
                        let model_id = model.id.clone();
                        let agent_type = model.agent_type;
                        let display_name = model.display_name.clone();
                        let required_tool = Self::required_tool(agent_type);
                        if !self.tools.is_available(required_tool) {
                            self.show_missing_tool(
                                required_tool,
                                format!(
                                    "{} is required to use this model.",
                                    required_tool.display_name()
                                ),
                            );
                            return Ok(effects);
                        }
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            let agent_changed =
                                session.set_agent_and_model(agent_type, Some(model_id.clone()));
                            let msg = if agent_changed {
                                format!("Switched to {} with model: {}", agent_type, display_name)
                            } else {
                                format!("Model changed to: {}", display_name)
                            };
                            let display = MessageDisplay::System { content: msg };
                            session.chat_view.push(display.to_chat_message());
                        }
                    }
                    self.state.model_selector_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SelectingTheme => {
                    return self.confirm_theme_picker();
                }
                InputMode::SelectingAgent => {
                    let agent_type = self.state.agent_selector_state.selected_agent();
                    self.state.agent_selector_state.hide();
                    self.create_tab_with_agent(agent_type);
                }
                InputMode::PickingProject => {
                    if let Some(project) = self.state.project_picker_state.selected_project() {
                        let repo_id = self.add_project_to_sidebar(project.path.clone());
                        self.state.project_picker_state.hide();
                        if let Some(id) = repo_id {
                            self.state.sidebar_data.expand_repo(id);
                            if let Some(repo_index) = self.state.sidebar_data.find_repo_index(id) {
                                self.state.sidebar_state.tree_state.selected = repo_index + 1;
                            }
                            self.state.sidebar_state.show();
                            self.state.sidebar_state.set_focused(true);
                            self.state.show_first_time_splash = false;
                            self.state.input_mode = InputMode::SidebarNavigation;
                        } else {
                            self.state.input_mode = InputMode::Normal;
                        }
                    }
                }
                InputMode::AddingRepository => {
                    if self.state.add_repo_dialog_state.is_valid() {
                        let repo_id = self.add_repository();
                        self.state.add_repo_dialog_state.hide();
                        if let Some(id) = repo_id {
                            self.state.sidebar_data.expand_repo(id);
                            if let Some(repo_index) = self.state.sidebar_data.find_repo_index(id) {
                                self.state.sidebar_state.tree_state.selected = repo_index + 1;
                            }
                            self.state.sidebar_state.show();
                            self.state.sidebar_state.set_focused(true);
                            self.state.show_first_time_splash = false;
                            self.state.input_mode = InputMode::SidebarNavigation;
                        } else {
                            self.state.input_mode = InputMode::Normal;
                        }
                    }
                }
                InputMode::SettingBaseDir => {
                    if self.state.base_dir_dialog_state.is_valid() {
                        if let Some(dao) = &self.app_state_dao {
                            if let Err(e) = dao.set(
                                "projects_base_dir",
                                self.state.base_dir_dialog_state.input(),
                            ) {
                                self.state.base_dir_dialog_state.hide();
                                self.show_error(
                                    "Failed to Save",
                                    &format!("Could not save projects directory: {}", e),
                                );
                                return Ok(effects);
                            }
                        }
                        let base_path = self.state.base_dir_dialog_state.expanded_path();
                        self.state.base_dir_dialog_state.hide();
                        self.state.close_overlays();
                        self.state.project_picker_state.show(base_path);
                        self.state.input_mode = InputMode::PickingProject;
                    }
                }
                InputMode::Confirming => {
                    if self.state.confirmation_dialog_state.is_confirm_selected() {
                        if let Some(context) = self.state.confirmation_dialog_state.context.clone()
                        {
                            match context {
                                ConfirmationContext::ArchiveWorkspace(id) => {
                                    effects.push(self.execute_archive_workspace(id));
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::SidebarNavigation;
                                    return Ok(effects);
                                }
                                ConfirmationContext::RemoveProject(id) => {
                                    effects.push(self.execute_remove_project(id));
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::SidebarNavigation;
                                    return Ok(effects);
                                }
                                ConfirmationContext::CreatePullRequest {
                                    tab_index,
                                    working_dir,
                                    preflight,
                                } => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::Normal;
                                    effects.extend(self.submit_pr_workflow(
                                        tab_index,
                                        working_dir,
                                        preflight,
                                    )?);
                                    return Ok(effects);
                                }
                                ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::Normal;
                                    effects.push(Effect::OpenPrInBrowser { working_dir });
                                    return Ok(effects);
                                }
                                ConfirmationContext::SteerFallback { message_id } => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::Normal;
                                    effects.extend(self.confirm_steer_fallback(message_id)?);
                                    return Ok(effects);
                                }
                                ConfirmationContext::ForkSession {
                                    parent_workspace_id,
                                    base_branch,
                                } => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::Normal;
                                    if let Some(effect) =
                                        self.execute_fork_session(parent_workspace_id, base_branch)
                                    {
                                        effects.push(effect);
                                    }
                                    return Ok(effects);
                                }
                            }
                        }
                    }
                    // Cancel selected - dismiss the confirmation dialog
                    self.state.input_mode = self.dismiss_confirmation_dialog();
                }
                InputMode::ShowingError => {
                    self.state.error_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::MissingTool => {
                    // Validate and save the path
                    if let Some(result) = self.state.missing_tool_dialog_state.validate() {
                        use crate::ui::components::MissingToolResult;
                        match result {
                            MissingToolResult::PathProvided(path) => {
                                let tool = self.state.missing_tool_dialog_state.tool;
                                // Update ToolAvailability
                                self.tools.update_tool(tool, path.clone());
                                // Save to config
                                if let Err(e) = crate::config::save_tool_path(tool, &path) {
                                    tracing::warn!("Failed to save tool path to config: {}", e);
                                }
                                self.refresh_runners();
                                self.state.missing_tool_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                            }
                            MissingToolResult::Skipped | MissingToolResult::Quit => {
                                self.state.missing_tool_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                            }
                        }
                    }
                    // If validation failed, error is set in state and we stay in dialog
                }
                InputMode::CommandPalette => {
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
                }
                _ => {}
            },
            Action::SetDefaultModel => {
                if self.state.input_mode == InputMode::SelectingModel {
                    if let Some(model) = self.state.model_selector_state.selected_model().cloned() {
                        let model_id = model.id.clone();
                        let agent_type = model.agent_type;
                        self.state
                            .model_selector_state
                            .set_default_model(agent_type, model_id.clone());
                        self.config.set_default_model(agent_type, model_id.clone());

                        if let Err(err) = crate::config::save_default_model(agent_type, &model_id) {
                            tracing::warn!(error = %err, "Failed to save default model");
                            self.state.set_timed_footer_message(
                                format!("Failed to save default model: {err}"),
                                Duration::from_secs(5),
                            );
                        } else {
                            self.state.set_timed_footer_message(
                                format!("Default model set to: {}", model.display_name),
                                Duration::from_secs(5),
                            );
                        }
                    }
                }
            }
            Action::Cancel => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.hide(true); // Cancelled - restore original
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::AddingRepository => {
                    self.state.add_repo_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SettingBaseDir => {
                    self.state.base_dir_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::Confirming => {
                    self.state.input_mode = self.dismiss_confirmation_dialog();
                }
                InputMode::ShowingError => {
                    self.state.error_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::MissingTool => {
                    self.state.missing_tool_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::Scrolling => {
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::Command => {
                    self.state.command_buffer.clear();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::ShowingHelp => {
                    self.state.help_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::QueueEditing => {
                    self.close_queue_editor();
                }
                _ => {}
            },
            Action::ExpandOrSelect => {
                // Same as Confirm for sidebar
                if self.state.input_mode == InputMode::SidebarNavigation {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    if let Some(node) = self.state.sidebar_data.get_at(selected) {
                        use crate::ui::components::{ActionType, NodeType};
                        match node.node_type {
                            NodeType::Action(ActionType::NewWorkspace) => {
                                if let Some(parent_id) = node.parent_id {
                                    effects.push(self.start_workspace_creation(parent_id));
                                }
                            }
                            NodeType::Workspace => {
                                self.open_workspace(node.id);
                                self.state.input_mode = InputMode::Normal;
                                self.state.sidebar_state.set_focused(false);
                            }
                            NodeType::Repository => {
                                self.state.sidebar_data.toggle_at(selected);
                            }
                        }
                    }
                }
            }
            Action::Collapse => {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    if let Some(node) = self.state.sidebar_data.get_at(selected) {
                        if !node.is_leaf() && node.expanded {
                            self.state.sidebar_data.toggle_at(selected);
                        }
                    }
                }
            }
            Action::AddRepository => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    self.state.close_overlays();
                    self.state.add_repo_dialog_state.show();
                    self.state.input_mode = InputMode::AddingRepository;
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.hide();
                    self.state.close_overlays();
                    self.state.add_repo_dialog_state.show();
                    self.state.input_mode = InputMode::AddingRepository;
                }
                _ => {}
            },
            Action::OpenSettings => {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    self.state.close_overlays();
                    if let Some(dao) = &self.app_state_dao {
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
            }
            Action::ArchiveOrRemove => {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    if let Some(node) = self.state.sidebar_data.get_at(selected) {
                        use crate::ui::components::NodeType;
                        match node.node_type {
                            NodeType::Workspace => {
                                self.initiate_archive_workspace(node.id);
                            }
                            NodeType::Repository => {
                                self.initiate_remove_project(node.id);
                            }
                            _ => {}
                        }
                    }
                }
            }

            // ========== Sidebar Navigation ==========
            Action::EnterSidebarMode => {
                self.state.sidebar_state.show();
                self.state.sidebar_state.set_focused(true);
                self.state.input_mode = InputMode::SidebarNavigation;
            }
            Action::ExitSidebarMode => {
                self.state.sidebar_state.set_focused(false);
                self.state.input_mode = InputMode::Normal;
            }

            // ========== Raw Events View ==========
            Action::RawEventsSelectNext => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.select_next();
                }
            }
            Action::RawEventsSelectPrev => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.select_prev();
                }
            }
            Action::RawEventsToggleExpand => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.toggle_expand();
                }
            }
            Action::RawEventsCollapse => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.collapse();
                }
            }
            // ========== Event Detail Panel ==========
            Action::EventDetailToggle => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.toggle_detail();
                }
            }
            Action::EventDetailScrollUp => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.event_detail.scroll_up(1);
                }
            }
            Action::EventDetailScrollDown => {
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let content_height = session.raw_events_view.detail_content_height();
                    session.raw_events_view.event_detail.scroll_down(
                        1,
                        content_height,
                        detail_height,
                    );
                }
            }
            Action::EventDetailPageUp => {
                let list_height = self.raw_events_list_visible_height();
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.raw_events_view.is_detail_visible() {
                        session.raw_events_view.event_detail.page_up(detail_height);
                    } else {
                        let amount = list_height.saturating_sub(2).max(1);
                        session.raw_events_view.scroll_up(amount);
                    }
                }
            }
            Action::EventDetailPageDown => {
                let list_height = self.raw_events_list_visible_height();
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let content_height = session.raw_events_view.detail_content_height();
                    if session.raw_events_view.is_detail_visible() {
                        session
                            .raw_events_view
                            .event_detail
                            .page_down(detail_height, content_height);
                    } else {
                        let amount = list_height.saturating_sub(2).max(1);
                        session.raw_events_view.scroll_down(amount, list_height);
                    }
                }
            }
            Action::EventDetailScrollToTop => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.event_detail.scroll_to_top();
                }
            }
            Action::EventDetailScrollToBottom => {
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let content_height = session.raw_events_view.detail_content_height();
                    session
                        .raw_events_view
                        .event_detail
                        .scroll_to_bottom(content_height, detail_height);
                }
            }
            Action::EventDetailCopy => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    if let Some(json) = session.raw_events_view.get_selected_json() {
                        effects.push(Effect::CopyToClipboard(json));
                    }
                }
            }

            // ========== Confirmation Dialog ==========
            Action::ConfirmYes => {
                if self.state.input_mode == InputMode::Confirming {
                    if let Some(context) = self.state.confirmation_dialog_state.context.clone() {
                        match context {
                            ConfirmationContext::ArchiveWorkspace(id) => {
                                effects.push(self.execute_archive_workspace(id));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                            }
                            ConfirmationContext::RemoveProject(id) => {
                                effects.push(self.execute_remove_project(id));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                            }
                            ConfirmationContext::CreatePullRequest {
                                tab_index,
                                working_dir,
                                preflight,
                            } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.submit_pr_workflow(
                                    tab_index,
                                    working_dir,
                                    preflight,
                                )?);
                            }
                            ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.push(Effect::OpenPrInBrowser { working_dir });
                            }
                            ConfirmationContext::SteerFallback { message_id } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.confirm_steer_fallback(message_id)?);
                            }
                            ConfirmationContext::ForkSession {
                                parent_workspace_id,
                                base_branch,
                            } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                if let Some(effect) =
                                    self.execute_fork_session(parent_workspace_id, base_branch)
                                {
                                    effects.push(effect);
                                }
                            }
                        }
                    }
                }
            }
            Action::ConfirmNo => {
                if self.state.input_mode == InputMode::Confirming {
                    self.state.input_mode = self.dismiss_confirmation_dialog();
                }
            }
            Action::ConfirmToggle => {
                if self.state.input_mode == InputMode::Confirming {
                    self.state.confirmation_dialog_state.toggle_selection();
                }
            }
            Action::ToggleDetails => {
                if self.state.input_mode == InputMode::ShowingError {
                    self.state.error_dialog_state.toggle_details();
                }
            }

            // ========== Agent Selection ==========
            Action::SelectAgent => {
                if self.state.input_mode == InputMode::SelectingAgent {
                    let agent_type = self.state.agent_selector_state.selected_agent();
                    self.state.agent_selector_state.hide();
                    self.create_tab_with_agent(agent_type);
                }
            }

            // ========== Command Mode ==========
            Action::ShowHelp => {
                self.state.close_overlays();
                self.state.help_dialog_state.show(&self.config.keybindings);
                self.state.input_mode = InputMode::ShowingHelp;
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
                self.state.close_overlays();
                let supports_plan_mode = self
                    .state
                    .tab_manager
                    .active_session()
                    .is_some_and(|s| s.capabilities.supports_plan_mode);
                self.state
                    .command_palette_state
                    .show(&self.config.keybindings, supports_plan_mode);
                self.state.input_mode = InputMode::CommandPalette;
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
                    let session_tab_dao = self.session_tab_dao.clone();
                    let app_state_dao = self.app_state_dao.clone();
                    if let Err(e) = tokio::task::spawn_blocking(move || {
                        Self::persist_session_state(snapshot, session_tab_dao, app_state_dao);
                    })
                    .await
                    {
                        eprintln!("Warning: Failed to save session state: {}", e);
                    }
                }
                Effect::StartAgent {
                    session_id,
                    agent_type,
                    config,
                } => {
                    let runner: Arc<dyn AgentRunner> = match agent_type {
                        AgentType::Claude => self.claude_runner.clone(),
                        AgentType::Codex => self.codex_runner.clone(),
                    };

                    let event_tx = self.event_tx.clone();

                    tokio::spawn(async move {
                        match runner.start(config).await {
                            Ok(mut handle) => {
                                // Send PID to main app for interrupt support
                                let pid = handle.pid;
                                send_app_event(
                                    &event_tx,
                                    AppEvent::AgentStarted { session_id, pid },
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
                Effect::CreateWorkspace { repo_id } => {
                    let repo_dao = self.repo_dao.clone();
                    let workspace_dao = self.workspace_dao.clone();
                    let worktree_manager = self.worktree_manager.clone();
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
                                .create_worktree(&base_path, &branch_name, &workspace_name)
                                .map_err(|e| format!("Failed to create worktree: {}", e))?;

                            let workspace = crate::data::Workspace::new(
                                repo_id,
                                &workspace_name,
                                &branch_name,
                                worktree_path,
                            );
                            let workspace_id = workspace.id;

                            if let Err(e) = workspace_dao.create(&workspace) {
                                if let Err(cleanup_err) =
                                    worktree_manager.remove_worktree(&base_path, &workspace.path)
                                {
                                    tracing::error!(
                                        error = %cleanup_err,
                                        base_path = %base_path.display(),
                                        workspace_path = %workspace.path.display(),
                                        "Failed to clean up worktree after DB error"
                                    );
                                }
                                if let Err(branch_err) =
                                    worktree_manager.delete_branch(&base_path, &branch_name)
                                {
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
                            AppEvent::WorkspaceCreated { result },
                            "workspace_created",
                        );
                    });
                }
                Effect::ForkWorkspace {
                    parent_workspace_id,
                    base_branch,
                } => {
                    let repo_dao = self.repo_dao.clone();
                    let workspace_dao = self.workspace_dao.clone();
                    let worktree_manager = self.worktree_manager.clone();
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
                                .create_worktree_from_branch(
                                    &base_path,
                                    &base_branch,
                                    &branch_name,
                                    &workspace_name,
                                )
                                .map_err(|e| format!("Failed to create worktree: {}", e))?;

                            let workspace = crate::data::Workspace::new(
                                parent_workspace.repository_id,
                                &workspace_name,
                                &branch_name,
                                worktree_path,
                            );
                            let workspace_id = workspace.id;

                            if let Err(e) = workspace_dao.create(&workspace) {
                                if let Err(cleanup_err) =
                                    worktree_manager.remove_worktree(&base_path, &workspace.path)
                                {
                                    tracing::error!(
                                        error = %cleanup_err,
                                        base_path = %base_path.display(),
                                        workspace_path = %workspace.path.display(),
                                        "Failed to clean up worktree after DB error"
                                    );
                                }
                                if let Err(branch_err) =
                                    worktree_manager.delete_branch(&base_path, &branch_name)
                                {
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
                            AppEvent::ForkWorkspaceCreated { result },
                            "fork_workspace_created",
                        );
                    });
                }
                Effect::ArchiveWorkspace { workspace_id } => {
                    let repo_dao = self.repo_dao.clone();
                    let workspace_dao = self.workspace_dao.clone();
                    let worktree_manager = self.worktree_manager.clone();
                    let event_tx = self.event_tx.clone();

                    tokio::task::spawn_blocking(move || {
                        let result: Result<WorkspaceArchived, String> = (|| {
                            let workspace_dao = workspace_dao
                                .ok_or_else(|| "No workspace DAO available".to_string())?;
                            let workspace = workspace_dao
                                .get_by_id(workspace_id)
                                .map_err(|e| format!("Failed to load workspace: {}", e))?
                                .ok_or_else(|| "Workspace not found".to_string())?;

                            let repo_base_path = repo_dao
                                .as_ref()
                                .and_then(|dao| {
                                    dao.get_by_id(workspace.repository_id).ok().flatten()
                                })
                                .and_then(|repo| repo.base_path);

                            let mut warnings = Vec::new();
                            let mut archived_commit_sha = None;
                            if let Some(base_path) = repo_base_path {
                                match worktree_manager.get_branch_sha(&base_path, &workspace.branch)
                                {
                                    Ok(commit_sha) => {
                                        archived_commit_sha = Some(commit_sha);
                                    }
                                    Err(e) => {
                                        warnings.push(format!("Failed to read branch SHA: {}", e));
                                    }
                                }

                                if let Err(e) =
                                    worktree_manager.remove_worktree(&base_path, &workspace.path)
                                {
                                    warnings.push(format!("Failed to remove worktree: {}", e));
                                }

                                if let Err(e) =
                                    worktree_manager.delete_branch(&base_path, &workspace.branch)
                                {
                                    warnings.push(format!(
                                        "Failed to delete branch '{}': {}",
                                        workspace.branch, e
                                    ));
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
                            AppEvent::WorkspaceArchived { result },
                            "workspace_archived",
                        );
                    });
                }
                Effect::RemoveProject { repo_id } => {
                    let repo_dao = self.repo_dao.clone();
                    let workspace_dao = self.workspace_dao.clone();
                    let worktree_manager = self.worktree_manager.clone();
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

                        let (repo_base_path, repo_name) = match repo_dao.get_by_id(repo_id) {
                            Ok(Some(repo)) => (repo.base_path, repo.name),
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
                            if let Some(ref base_path) = repo_base_path {
                                match worktree_manager.get_branch_sha(base_path, &ws.branch) {
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

                                if let Err(e) =
                                    worktree_manager.remove_worktree(base_path, &ws.path)
                                {
                                    errors.push(format!(
                                        "Failed to remove worktree '{}': {}",
                                        ws.name, e
                                    ));
                                }

                                if let Err(e) =
                                    worktree_manager.delete_branch(base_path, &ws.branch)
                                {
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
                        let repo_name_path = std::path::Path::new(&repo_name);
                        let mut components = repo_name_path.components();
                        let is_safe_repo_name =
                            matches!(components.next(), Some(Component::Normal(_)))
                                && components.next().is_none();
                        if !is_safe_repo_name {
                            errors.push(format!(
                                "Skipping project folder removal due to unsafe repo name: {}",
                                repo_name
                            ));
                        } else {
                            let project_workspaces_path = workspaces_dir.join(&repo_name);
                            match (
                                std::fs::canonicalize(&workspaces_dir),
                                std::fs::canonicalize(&project_workspaces_path),
                            ) {
                                (Ok(canonical_root), Ok(canonical_project)) => {
                                    if canonical_project.starts_with(&canonical_root) {
                                        if let Err(e) = std::fs::remove_dir_all(&canonical_project)
                                        {
                                            errors.push(format!(
                                                "Failed to remove project folder: {}",
                                                e
                                            ));
                                        }
                                    } else {
                                        errors.push(format!(
                                            "Skipping project folder removal outside managed root: {}",
                                            canonical_project.display()
                                        ));
                                    }
                                }
                                (Err(e), _) => {
                                    errors.push(format!(
                                        "Failed to canonicalize workspaces dir: {}",
                                        e
                                    ));
                                }
                                (_, Err(e)) => {
                                    if e.kind() != io::ErrorKind::NotFound {
                                        errors.push(format!(
                                            "Failed to canonicalize project folder: {}",
                                            e
                                        ));
                                    }
                                }
                            }
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
                        .unwrap_or_else(|| self.config.working_dir.clone());

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
                    let tools = self.tools.clone();
                    let event_tx = self.event_tx.clone();
                    let worktree_manager = self.worktree_manager.clone();
                    let workspace_dao = self.workspace_dao.clone();

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

    /// Check if a key event should be handled as text input
    /// Returns true if the key is a printable character without Control/Alt modifiers
    /// and we're in a text-input context
    fn should_handle_as_text_input(&self, key: &event::KeyEvent, context: KeyContext) -> bool {
        // Only handle plain characters (no Ctrl or Alt)
        let has_modifier = key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT);

        if has_modifier {
            return false;
        }

        // Check if this is a character key
        let is_char = matches!(key.code, KeyCode::Char(_));

        if !is_char {
            return false;
        }

        // Only treat as text input in appropriate contexts
        matches!(
            context,
            KeyContext::Chat
                | KeyContext::AddRepository
                | KeyContext::BaseDir
                | KeyContext::ProjectPicker
                | KeyContext::Command
                | KeyContext::HelpDialog
                | KeyContext::SessionImport
                | KeyContext::CommandPalette
                | KeyContext::ThemePicker
                | KeyContext::ModelSelector
        )
    }

    /// Handle text input for text-input contexts
    fn handle_text_input(&mut self, key: event::KeyEvent) {
        let KeyCode::Char(c) = key.code else {
            return;
        };

        match self.state.input_mode {
            InputMode::Normal => {
                // Note: ':' is handled globally in handle_key_event
                // Check for help trigger (? on empty input)
                if c == '?' {
                    if let Some(session) = self.state.tab_manager.active_session() {
                        if session.input_box.input().is_empty() {
                            self.state.close_overlays();
                            self.state.help_dialog_state.show(&self.config.keybindings);
                            self.state.input_mode = InputMode::ShowingHelp;
                            return;
                        }
                    }
                }
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.insert_char(c);
                }
            }
            InputMode::Command => {
                self.state.command_buffer.push(c);
            }
            InputMode::ShowingHelp => {
                self.state.help_dialog_state.insert_char(c);
            }
            InputMode::AddingRepository => {
                self.state.add_repo_dialog_state.insert_char(c);
            }
            InputMode::SettingBaseDir => {
                self.state.base_dir_dialog_state.insert_char(c);
            }
            InputMode::PickingProject => {
                self.state.project_picker_state.insert_char(c);
            }
            InputMode::ImportingSession => {
                self.state.session_import_state.insert_char(c);
            }
            InputMode::CommandPalette => {
                self.state.command_palette_state.insert_char(c);
            }
            InputMode::MissingTool => {
                self.state.missing_tool_dialog_state.insert_char(c);
            }
            InputMode::SelectingTheme => {
                self.state.theme_picker_state.insert_char(c);
            }
            InputMode::SelectingModel => {
                self.state.model_selector_state.insert_char(c);
            }
            _ => {}
        }
    }

    fn handle_paste_input(&mut self, pasted: String) {
        // Normalize line endings: CRLF → LF, then lone CR → LF
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        match self.state.input_mode {
            InputMode::Normal => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.handle_paste(pasted);
                }
            }
            InputMode::Command => {
                let sanitized = pasted.replace('\n', " ");
                self.state.command_buffer.push_str(&sanitized);
            }
            InputMode::ShowingHelp => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.help_dialog_state.insert_char(ch);
                }
            }
            InputMode::AddingRepository => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.add_repo_dialog_state.insert_char(ch);
                }
            }
            InputMode::SettingBaseDir => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.base_dir_dialog_state.insert_char(ch);
                }
            }
            InputMode::PickingProject => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.project_picker_state.insert_char(ch);
                }
            }
            InputMode::ImportingSession => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.session_import_state.insert_char(ch);
                }
            }
            InputMode::CommandPalette => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.command_palette_state.insert_char(ch);
                }
            }
            InputMode::MissingTool => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.missing_tool_dialog_state.insert_char(ch);
                }
            }
            InputMode::SelectingTheme => {
                let sanitized = pasted.replace('\n', " ");
                self.state.theme_picker_state.insert_str(&sanitized);
            }
            InputMode::SelectingModel => {
                let sanitized = pasted.replace('\n', " ");
                self.state.model_selector_state.insert_str(&sanitized);
            }
            _ => {}
        }
    }

    fn confirm_theme_picker(&mut self) -> anyhow::Result<Vec<Effect>> {
        let previous_theme_name = self.config.theme_name.clone();
        let previous_theme_path = self.config.theme_path.clone();

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
                self.config.theme_name = previous_theme_name;
                self.config.theme_path = previous_theme_path;
                self.state.theme_picker_state.hide(true); // Restore original theme
                                                          // Clear any pending theme picker error state.
                self.state.theme_picker_state.take_error();
                self.state.input_mode = InputMode::Normal;
                self.state.set_timed_footer_message(
                    format!("Failed to save theme: {err}"),
                    Duration::from_secs(5),
                );
                return Ok(Vec::new());
            }
            self.config.theme_name = name;
            self.config.theme_path = path;
            self.state.set_timed_footer_message(
                format!("Theme: {}", display_name),
                Duration::from_secs(3),
            );
        }

        self.state.theme_picker_state.hide(false); // Not cancelled
        self.state.input_mode = InputMode::Normal;
        Ok(Vec::new())
    }

    /// Execute a command from command mode
    /// Returns an action to execute if the command maps to one
    fn execute_command(&mut self) -> Option<Action> {
        let command = self.state.command_buffer.trim().to_lowercase();
        self.state.command_buffer.clear();
        self.state.input_mode = InputMode::Normal;

        // First check for built-in command aliases
        match command.as_str() {
            "help" | "h" | "?" => {
                self.state.close_overlays();
                self.state.help_dialog_state.show(&self.config.keybindings);
                self.state.input_mode = InputMode::ShowingHelp;
                return None;
            }
            "q" => {
                return Some(Action::Quit);
            }
            _ => {}
        }

        // Try to parse as an action name
        parse_action(&command)
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
                let next = matches
                    .iter()
                    .find(|&&cmd| cmd > current.as_str())
                    .or(matches.first())
                    .unwrap();
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
        let Some(workspace_dao) = &self.workspace_dao else {
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
            .repo_dao
            .as_ref()
            .and_then(|dao| dao.get_by_id(workspace.repository_id).ok().flatten())
            .map(|repo| repo.name);

        // Check if there's a saved session for this workspace (to restore chat history)
        let saved_tab = self
            .session_tab_dao
            .as_ref()
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
        let no_agents_available = !self.tools.is_available(crate::util::Tool::Claude)
            && !self.tools.is_available(crate::util::Tool::Codex);
        let tab_agent_type = saved_tab
            .as_ref()
            .map(|saved| saved.agent_type)
            .unwrap_or_else(|| {
                let default_agent = self.config.default_agent;
                let default_tool = Self::required_tool(default_agent);
                if self.tools.is_available(default_tool) {
                    default_agent
                } else if self.tools.is_available(crate::util::Tool::Claude) {
                    AgentType::Claude
                } else if self.tools.is_available(crate::util::Tool::Codex) {
                    AgentType::Codex
                } else {
                    AgentType::Claude
                }
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
        if !self.tools.is_available(required_tool) {
            self.show_missing_tool(
                required_tool,
                if has_saved_session {
                    format!(
                        "{} is required to open this workspace's saved session.",
                        required_tool.display_name()
                    )
                } else if no_agents_available {
                    "An agent tool (Claude Code or Codex CLI) is required to open this workspace."
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
        self.state
            .tab_manager
            .new_tab_with_working_dir(tab_agent_type, workspace.path.clone());

        // Store workspace info in session and restore chat history if available
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.workspace_id = Some(workspace_id);
            session.project_name = project_name;
            session.workspace_name = Some(workspace.name.clone());

            // Restore saved session data if available
            if let Some(saved) = saved_tab {
                session.set_agent_and_model(saved.agent_type, saved.model);
                if let Some(saved_mode) = saved_agent_mode {
                    session.agent_mode = saved_mode; // Pre-clamped above
                }
                session.fork_seed_id = saved.fork_seed_id;

                // Restore chat history from agent files
                if let Some(ref session_id_str) = saved.agent_session_id {
                    let session_id = SessionId::from_string(session_id_str.clone());
                    session.resume_session_id = Some(session_id.clone());
                    session.agent_session_id = Some(session_id);

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
                let model_id = self.config.default_model_for(tab_agent_type);
                session.model = Some(model_id);
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
        if agent_type == AgentType::Codex && mode == AgentMode::Plan {
            AgentMode::Build
        } else {
            mode
        }
    }

    /// Map an agent type to its required tool.
    fn required_tool(agent_type: AgentType) -> crate::util::Tool {
        match agent_type {
            AgentType::Claude => crate::util::Tool::Claude,
            AgentType::Codex => crate::util::Tool::Codex,
        }
    }

    fn model_selector_defaults(&self) -> DefaultModelSelection {
        let agent_type = self.config.default_agent;
        DefaultModelSelection {
            agent_type: Some(agent_type),
            model_id: Some(self.config.default_model_for(agent_type)),
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

    /// Refresh agent runners using the latest tool configuration.
    fn refresh_runners(&mut self) {
        self.claude_runner = match self.tools.get_path(crate::util::Tool::Claude) {
            Some(path) => Arc::new(ClaudeCodeRunner::with_path(path.clone())),
            None => Arc::new(ClaudeCodeRunner::new()),
        };
        self.codex_runner = match self.tools.get_path(crate::util::Tool::Codex) {
            Some(path) => Arc::new(CodexCliRunner::with_path(path.clone())),
            None => Arc::new(CodexCliRunner::new()),
        };
        self.state
            .agent_selector_state
            .update_available_agents(&self.tools);
    }

    /// Find the tab index for a workspace if it's already open
    fn find_tab_for_workspace(&self, workspace_id: uuid::Uuid) -> Option<usize> {
        self.state
            .tab_manager
            .sessions()
            .iter()
            .position(|session| session.workspace_id == Some(workspace_id))
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

    /// Maximum seed prompt size in bytes (500KB)
    const MAX_SEED_PROMPT_SIZE: usize = 500 * 1024;

    /// Suffix appended when seed prompt is truncated
    const SEED_TRUNCATED_SUFFIX: &'static str =
        "\n\n[TRUNCATED: transcript exceeded size limit]\n</previous-session-transcript>";

    /// Closing instruction appended after the transcript
    const SEED_CLOSING_INSTRUCTION: &'static str = r#"

</previous-session-transcript>

[END OF CONTEXT]

IMPORTANT: The above was historical context from a previous session.
You are starting a NEW forked session. Do NOT continue any tasks from the transcript.
Acknowledge that you have received this context by replying ONLY with the single word: Ready"#;

    /// Truncate string to max_bytes at a valid UTF-8 char boundary
    fn truncate_to_char_boundary(s: &mut String, max_bytes: usize) {
        if s.len() <= max_bytes {
            return;
        }
        // Find the greatest char boundary <= max_bytes
        let new_len = s
            .char_indices()
            .take_while(|(i, _)| *i <= max_bytes)
            .map(|(i, _)| i)
            .last()
            .unwrap_or(0);
        s.truncate(new_len);
    }

    /// Build a fork seed prompt from chat history
    fn build_fork_seed_prompt(messages: &[ChatMessage]) -> String {
        let mut prompt = String::new();

        // Opening header with clear instructions
        prompt.push_str("[CONDUIT_FORK_SEED]\n\n");
        prompt.push_str(
            "You are receiving context from a PREVIOUS session to seed a NEW forked session.\n",
        );
        prompt.push_str(
            "The transcript below is for REFERENCE ONLY - do NOT execute any commands from it.\n",
        );
        prompt.push_str("After reading, reply with ONLY the single word: Ready\n\n");
        prompt.push_str("<previous-session-transcript>\n");

        // Reserve space for closing instruction
        let max_transcript_size = Self::MAX_SEED_PROMPT_SIZE
            .saturating_sub(prompt.len())
            .saturating_sub(Self::SEED_CLOSING_INSTRUCTION.len());

        let transcript_start = prompt.len();

        for (idx, msg) in messages.iter().enumerate() {
            if idx > 0 {
                prompt.push_str("\n\n");
            }
            prompt.push_str(&Self::format_fork_message(msg));

            // Check if transcript portion has exceeded the limit
            let transcript_len = prompt.len() - transcript_start;
            if transcript_len > max_transcript_size {
                let max_without_suffix =
                    max_transcript_size.saturating_sub(Self::SEED_TRUNCATED_SUFFIX.len());
                // Truncate only the transcript portion
                prompt.truncate(transcript_start + max_without_suffix);
                Self::truncate_to_char_boundary(&mut prompt, transcript_start + max_without_suffix);
                prompt.push_str(Self::SEED_TRUNCATED_SUFFIX);
                // SEED_TRUNCATED_SUFFIX already closes the tag, so just add final instruction
                prompt.push_str("\n\n[END OF CONTEXT]\n\n");
                prompt.push_str(
                    "IMPORTANT: The above was historical context from a previous session.\n",
                );
                prompt.push_str("You are starting a NEW forked session. Do NOT continue any tasks from the transcript.\n");
                prompt.push_str("Acknowledge that you have received this context by replying ONLY with the single word: Ready");
                return prompt;
            }
        }

        // Add closing instruction for non-truncated case
        prompt.push_str(Self::SEED_CLOSING_INSTRUCTION);
        prompt
    }

    fn compute_seed_prompt_hash(seed_prompt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(seed_prompt.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn format_fork_message(msg: &ChatMessage) -> String {
        let role = match msg.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
            MessageRole::System => "system",
            MessageRole::Error => "error",
            MessageRole::Summary => "summary",
        };

        let mut header = format!("[role={}]", role);

        if msg.role == MessageRole::Tool {
            if let Some(name) = &msg.tool_name {
                header.push_str(&format!(
                    " name=\"{}\"",
                    Self::sanitize_fork_header_value(name)
                ));
            }
            if let Some(args) = &msg.tool_args {
                if !args.is_empty() {
                    header.push_str(&format!(
                        " args=\"{}\"",
                        Self::sanitize_fork_header_value(args)
                    ));
                }
            }
            if let Some(exit_code) = msg.exit_code {
                header.push_str(&format!(" exit={}", exit_code));
            }
        }

        let content = if msg.role == MessageRole::Summary {
            msg.summary
                .as_ref()
                .map(Self::format_turn_summary_for_seed)
                .unwrap_or_default()
        } else {
            msg.content.clone()
        };

        if content.trim().is_empty() {
            header
        } else {
            format!("{header}\n{content}")
        }
    }

    fn format_turn_summary_for_seed(summary: &crate::ui::components::TurnSummary) -> String {
        let mut parts = Vec::new();
        if summary.duration_secs > 0 {
            parts.push(format!("duration={}s", summary.duration_secs));
        }
        if summary.input_tokens > 0 || summary.output_tokens > 0 {
            parts.push(format!(
                "tokens_in={}, tokens_out={}",
                summary.input_tokens, summary.output_tokens
            ));
        }
        if !summary.files_changed.is_empty() {
            let files = summary
                .files_changed
                .iter()
                .map(|f| format!("{} +{} -{}", f.filename, f.additions, f.deletions))
                .collect::<Vec<_>>()
                .join("; ");
            parts.push(format!("files=[{}]", files));
        }
        if parts.is_empty() {
            "summary".to_string()
        } else {
            parts.join(", ")
        }
    }

    fn sanitize_fork_header_value(value: &str) -> String {
        value
            .chars()
            .map(|c| {
                if c == '"' {
                    '\''
                } else if c.is_control() {
                    ' '
                } else {
                    c
                }
            })
            .collect()
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
    fn start_workspace_creation(&mut self, repo_id: uuid::Uuid) -> Effect {
        Effect::CreateWorkspace { repo_id }
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

    /// Sync footer spinner state to the active tab's processing state
    fn sync_footer_spinner(&mut self) {
        let is_active_processing = self
            .state
            .tab_manager
            .active_session()
            .is_some_and(|s| s.is_processing);

        if is_active_processing {
            // Start spinner if active tab is processing and spinner not already running
            if self.state.footer_spinner.is_none() {
                self.state.start_footer_spinner(None);
            }
        } else if self.state.footer_spinner.is_some() {
            // Stop spinner if active tab is not processing and spinner is running
            self.state.stop_footer_spinner();
        }
    }

    /// Dismiss the confirmation dialog and clean up fork state if applicable.
    /// Returns the input mode to transition to.
    fn dismiss_confirmation_dialog(&mut self) -> InputMode {
        // Cache context before hide() clears it
        let ctx = self.state.confirmation_dialog_state.context.clone();

        // Clear pending fork request if dismissing a fork confirmation
        if matches!(&ctx, Some(ConfirmationContext::ForkSession { .. })) {
            self.state.pending_fork_request = None;
        }

        self.state.confirmation_dialog_state.hide();

        // Return appropriate input mode based on context
        match ctx {
            // PR/Fork/Steer dialogs originated from chat view, return to Normal
            Some(ConfirmationContext::CreatePullRequest { .. })
            | Some(ConfirmationContext::OpenExistingPr { .. })
            | Some(ConfirmationContext::ForkSession { .. })
            | Some(ConfirmationContext::SteerFallback { .. }) => InputMode::Normal,
            // Sidebar operations return to sidebar navigation
            Some(ConfirmationContext::ArchiveWorkspace(_))
            | Some(ConfirmationContext::RemoveProject(_)) => InputMode::SidebarNavigation,
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

    /// Initiate the archive workspace flow - check git status and show confirmation dialog
    fn initiate_archive_workspace(&mut self, workspace_id: uuid::Uuid) {
        // Get the workspace
        let Some(workspace_dao) = &self.workspace_dao else {
            return;
        };

        let Ok(Some(workspace)) = workspace_dao.get_by_id(workspace_id) else {
            tracing::error!(workspace_id = %workspace_id, "Workspace not found");
            return;
        };

        // Get git branch status
        let branch_status = self.worktree_manager.get_branch_status(&workspace.path);

        // Build warnings and determine confirmation type
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

        // Determine confirmation type based on warnings
        let confirmation_type = match (has_dirty, has_unmerged) {
            (true, true) => ConfirmationType::Danger,
            (true, false) | (false, true) => ConfirmationType::Warning,
            (false, false) => {
                if warnings.is_empty() {
                    ConfirmationType::Info
                } else {
                    ConfirmationType::Warning
                }
            }
        };

        // Show confirmation dialog
        self.state.close_overlays();
        self.state.confirmation_dialog_state.show(
            format!("Archive \"{}\"?", workspace.name),
            "This will remove the worktree and delete the branch.",
            warnings,
            confirmation_type,
            "Archive",
            Some(ConfirmationContext::ArchiveWorkspace(workspace_id)),
        );
        self.state.input_mode = InputMode::Confirming;
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
    fn execute_archive_workspace(&mut self, workspace_id: uuid::Uuid) -> Effect {
        Effect::ArchiveWorkspace { workspace_id }
    }

    /// Initiate project removal - shows confirmation dialog
    fn initiate_remove_project(&mut self, repo_id: uuid::Uuid) {
        // Get repository info
        let Some(repo_dao) = &self.repo_dao else {
            return;
        };

        let Ok(Some(repo)) = repo_dao.get_by_id(repo_id) else {
            tracing::error!(repo_id = %repo_id, "Repository not found");
            return;
        };

        // Get all workspaces for this repository
        let workspaces = if let Some(workspace_dao) = &self.workspace_dao {
            workspace_dao.get_by_repository(repo_id).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Check git status for each workspace
        let mut warnings = Vec::new();
        let mut has_dirty = false;
        let mut has_unmerged = false;

        for ws in &workspaces {
            if let Ok(status) = self.worktree_manager.get_branch_status(&ws.path) {
                if status.is_dirty {
                    has_dirty = true;
                }
                if !status.is_merged {
                    has_unmerged = true;
                }
            }
        }

        // Build warning messages
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

        // Determine confirmation type based on risk
        let confirmation_type = match (has_dirty, has_unmerged) {
            (true, true) => ConfirmationType::Danger,
            (true, false) | (false, true) => ConfirmationType::Warning,
            (false, false) => {
                if workspace_count > 0 {
                    ConfirmationType::Warning
                } else {
                    ConfirmationType::Info
                }
            }
        };

        // Show confirmation dialog
        self.state.close_overlays();
        self.state.confirmation_dialog_state.show(
            format!("Remove \"{}\"?", repo.name),
            "This will archive all workspaces and remove the project.",
            warnings,
            confirmation_type,
            "Remove",
            Some(ConfirmationContext::RemoveProject(repo_id)),
        );
        self.state.input_mode = InputMode::Confirming;
    }

    /// Execute project removal after confirmation
    fn execute_remove_project(&mut self, repo_id: uuid::Uuid) -> Effect {
        // Set spinner mode
        self.state.input_mode = InputMode::RemovingProject;

        Effect::RemoveProject { repo_id }
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
            self.state.tab_manager.close_tab(idx);
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
        let Some(repo_dao) = &self.repo_dao else {
            return None;
        };

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

        let Some(repo_dao) = &self.repo_dao else {
            return None;
        };

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
        self.state.tab_manager.new_tab(agent_type);
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            let model_id = self.config.default_model_for(agent_type);
            session.model = Some(model_id);
            session.init_context_for_model();
            session.update_status();
        }
        self.state.input_mode = InputMode::Normal;
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
        session.agent_session_id = Some(session_id);

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

    fn record_scroll(&mut self, lines: usize) {
        if lines > 0 {
            self.state.metrics.record_scroll_event(lines);
        }
    }

    fn should_route_scroll_to_chat(&self) -> bool {
        self.state.input_mode != InputMode::ShowingHelp
            && !(self.state.input_mode == InputMode::PickingProject
                && self.state.project_picker_state.is_visible())
            && !(self.state.input_mode == InputMode::ImportingSession
                && self.state.session_import_state.is_visible())
            && !(self.state.input_mode == InputMode::CommandPalette
                && self.state.command_palette_state.is_visible())
            && !(self.state.input_mode == InputMode::SelectingTheme
                && self.state.theme_picker_state.is_visible())
            && !(self.state.input_mode == InputMode::SelectingModel
                && self.state.model_selector_state.is_visible())
    }

    fn raw_events_list_visible_height(&self) -> usize {
        self.state
            .raw_events_area
            .map(|r| r.height.saturating_sub(2) as usize)
            .unwrap_or(20)
    }

    fn raw_events_detail_visible_height(&self) -> usize {
        let Some(area) = self.state.raw_events_area else {
            return 20;
        };
        if area.width < crate::ui::components::DETAIL_PANEL_BREAKPOINT {
            let overlay_height = (area.height as f32 * 0.8) as u16;
            overlay_height.saturating_sub(2) as usize
        } else {
            area.height.saturating_sub(2) as usize
        }
    }

    fn flush_scroll_deltas(&mut self, pending_up: &mut usize, pending_down: &mut usize) {
        if *pending_up == 0 && *pending_down == 0 {
            return;
        }

        if self.state.input_mode == InputMode::ShowingHelp {
            // Route scroll to help dialog
            if *pending_up > 0 {
                self.state.help_dialog_state.scroll_up(*pending_up);
            }
            if *pending_down > 0 {
                self.state.help_dialog_state.scroll_down(*pending_down);
            }
        } else if self.state.input_mode == InputMode::PickingProject
            && self.state.project_picker_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.project_picker_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.project_picker_state.select_next();
            }
        } else if self.state.input_mode == InputMode::ImportingSession
            && self.state.session_import_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.session_import_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.session_import_state.select_next();
            }
        } else if self.state.input_mode == InputMode::CommandPalette
            && self.state.command_palette_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.command_palette_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.command_palette_state.select_next();
            }
        } else if self.state.input_mode == InputMode::SelectingTheme
            && self.state.theme_picker_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.theme_picker_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.theme_picker_state.select_next();
            }
        } else if self.state.view_mode == ViewMode::RawEvents {
            let list_height = self.raw_events_list_visible_height();
            let detail_height = self.raw_events_detail_visible_height();
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                if session.raw_events_view.is_detail_visible() {
                    let content_height = session.raw_events_view.detail_content_height();
                    let visible_height = detail_height;
                    if *pending_up > 0 {
                        session.raw_events_view.event_detail.scroll_up(*pending_up);
                    }
                    if *pending_down > 0 {
                        session.raw_events_view.event_detail.scroll_down(
                            *pending_down,
                            content_height,
                            visible_height,
                        );
                    }
                } else {
                    if *pending_up > 0 {
                        session.raw_events_view.scroll_up(*pending_up);
                    }
                    if *pending_down > 0 {
                        session
                            .raw_events_view
                            .scroll_down(*pending_down, list_height);
                    }
                }
            }
        } else if let Some(session) = self.state.tab_manager.active_session_mut() {
            if *pending_up > 0 {
                session.chat_view.scroll_up(*pending_up);
            }
            if *pending_down > 0 {
                session.chat_view.scroll_down(*pending_down);
            }
        }

        *pending_up = 0;
        *pending_down = 0;
    }

    async fn handle_mouse_event(
        &mut self,
        mouse: event::MouseEvent,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        let x = mouse.column;
        let y = mouse.row;

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                // Route scroll to appropriate component based on mode
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_up(3);
                } else if self.state.input_mode == InputMode::PickingProject
                    && self.state.project_picker_state.is_visible()
                {
                    self.state.project_picker_state.select_prev();
                } else if self.state.input_mode == InputMode::ImportingSession
                    && self.state.session_import_state.is_visible()
                {
                    self.state.session_import_state.select_prev();
                } else if self.state.input_mode == InputMode::SelectingTheme
                    && self.state.theme_picker_state.is_visible()
                {
                    self.state.theme_picker_state.select_prev();
                } else if self.handle_tab_bar_wheel(x, y, true) {
                    return Ok(Vec::new());
                } else if self.state.view_mode == ViewMode::RawEvents {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if session.raw_events_view.is_detail_visible() {
                            session.raw_events_view.event_detail.scroll_up(3);
                        } else {
                            session.raw_events_view.scroll_up(3);
                        }
                    }
                    self.record_scroll(3);
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_up(1);
                    self.record_scroll(1);
                }
                Ok(Vec::new())
            }
            MouseEventKind::ScrollDown => {
                // Route scroll to appropriate component based on mode
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_down(3);
                } else if self.state.input_mode == InputMode::PickingProject
                    && self.state.project_picker_state.is_visible()
                {
                    self.state.project_picker_state.select_next();
                } else if self.state.input_mode == InputMode::ImportingSession
                    && self.state.session_import_state.is_visible()
                {
                    self.state.session_import_state.select_next();
                } else if self.state.input_mode == InputMode::SelectingTheme
                    && self.state.theme_picker_state.is_visible()
                {
                    self.state.theme_picker_state.select_next();
                } else if self.handle_tab_bar_wheel(x, y, false) {
                    return Ok(Vec::new());
                } else if self.state.view_mode == ViewMode::RawEvents {
                    let list_height = self.raw_events_list_visible_height();
                    let detail_height = self.raw_events_detail_visible_height();
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if session.raw_events_view.is_detail_visible() {
                            let content_height = session.raw_events_view.detail_content_height();
                            session.raw_events_view.event_detail.scroll_down(
                                3,
                                content_height,
                                detail_height,
                            );
                        } else {
                            session.raw_events_view.scroll_down(3, list_height);
                        }
                    }
                    self.record_scroll(3);
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_down(1);
                    self.record_scroll(1);
                }
                Ok(Vec::new())
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.handle_scrollbar_press(x, y) {
                    return Ok(Vec::new());
                }
                if self.handle_selection_start(x, y) {
                    return Ok(Vec::new());
                }
                // Handle left clicks based on position
                self.handle_mouse_click(x, y, terminal, guard).await
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.handle_scrollbar_drag(y) {
                    return Ok(Vec::new());
                }
                if self.handle_selection_drag(x, y) {
                    return Ok(Vec::new());
                }
                Ok(Vec::new())
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.state.scroll_drag = None;
                if let Some(effects) = self.handle_selection_end() {
                    return Ok(effects);
                }
                Ok(Vec::new())
            }
            MouseEventKind::Moved => {
                // Update hover state for sidebar workspace name expansion
                if let Some(sidebar_area) = self.state.sidebar_area {
                    // Tree view starts after header (uses centralized constant for consistency)
                    let tree_start_y = sidebar_area.y.saturating_add(SIDEBAR_HEADER_ROWS);
                    // Sidebar has no borders - tree renders directly in content area
                    let inner_x = sidebar_area.x;
                    let inner_width = sidebar_area.width as usize;

                    if Self::point_in_rect(x, y, sidebar_area) && y >= tree_start_y {
                        // Calculate visual row within the tree view
                        let visual_row = (y - tree_start_y) as usize;
                        // Calculate x position within the tree inner area
                        let x_in_tree = x.saturating_sub(inner_x) as usize;
                        let scroll_offset = self.state.sidebar_state.tree_state.offset;

                        // Check if hovering over a workspace name (not git stats or PR)
                        if let Some(workspace_id) = self.state.sidebar_data.workspace_at_name_line(
                            visual_row,
                            x_in_tree,
                            scroll_offset,
                            inner_width,
                        ) {
                            self.state.sidebar_state.tree_state.set_hover(workspace_id);
                        } else {
                            self.state.sidebar_state.tree_state.clear_hover();
                        }
                    } else {
                        // Mouse left sidebar, clear hover
                        self.state.sidebar_state.tree_state.clear_hover();
                    }
                }

                // Update hover state for raw events session ID
                if self.state.view_mode == ViewMode::RawEvents {
                    if let Some(raw_events_area) = self.state.raw_events_area {
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            let hover_changed = session.raw_events_view.update_session_id_hover(
                                x,
                                y,
                                raw_events_area,
                            );

                            if hover_changed {
                                // Check if now hovering (need to re-check since we mutated)
                                let is_hovered = session.raw_events_view.is_session_id_hovered();
                                if is_hovered {
                                    self.state.set_footer_message(Some(
                                        "Click session ID to copy".to_string(),
                                    ));
                                } else {
                                    // Clear the hint message when no longer hovering
                                    self.state.set_footer_message(None);
                                }
                            }
                        }
                    }
                }
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn handle_scrollbar_press(&mut self, x: u16, y: u16) -> bool {
        if let Some(target) = self.scrollbar_target_at(x, y) {
            self.state.scroll_drag = Some(target);
            return self.apply_scrollbar_drag(target, y);
        }
        false
    }

    fn handle_scrollbar_drag(&mut self, y: u16) -> bool {
        if let Some(target) = self.state.scroll_drag {
            return self.apply_scrollbar_drag(target, y);
        }
        false
    }

    fn handle_selection_start(&mut self, x: u16, y: u16) -> bool {
        if self.has_active_dialog() {
            return false;
        }
        if self.state.view_mode != ViewMode::Chat {
            return false;
        }

        let Some(session) = self.state.tab_manager.active_session_mut() else {
            return false;
        };

        if let Some(input_area) = self.state.input_area {
            if self.state.input_mode != InputMode::Command && Self::point_in_rect(x, y, input_area)
            {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                session.chat_view.clear_selection();
                if session.input_box.begin_selection(x, y, input_area) {
                    self.state.selection_drag = Some(SelectionDragTarget::Input);
                    return true;
                }
            }
        }

        if let Some(chat_area) = self.state.chat_area {
            if Self::point_in_rect(x, y, chat_area) {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                session.input_box.clear_selection();
                if session.chat_view.begin_selection(x, y, chat_area) {
                    self.state.selection_drag = Some(SelectionDragTarget::Chat);
                    return true;
                }
            }
        }

        false
    }

    fn handle_selection_drag(&mut self, x: u16, y: u16) -> bool {
        let Some(target) = self.state.selection_drag else {
            return false;
        };

        let mut scrolled_lines = 0usize;
        let mut handled = false;

        {
            let Some(session) = self.state.tab_manager.active_session_mut() else {
                return false;
            };

            match target {
                SelectionDragTarget::Input => {
                    if let Some(input_area) = self.state.input_area {
                        session.input_box.update_selection(x, y, input_area);
                        handled = true;
                    }
                }
                SelectionDragTarget::Chat => {
                    if let Some(chat_area) = self.state.chat_area {
                        let top_edge = chat_area.y;
                        let bottom_edge_inclusive = chat_area
                            .y
                            .saturating_add(chat_area.height.saturating_sub(1));
                        let bottom_edge_exclusive = chat_area.y.saturating_add(chat_area.height);
                        let should_scroll_up = if Self::AUTO_SCROLL_ON_EDGE_INCLUSIVE {
                            y <= top_edge
                        } else {
                            y < top_edge
                        };
                        let should_scroll_down = if Self::AUTO_SCROLL_ON_EDGE_INCLUSIVE {
                            y >= bottom_edge_inclusive
                        } else {
                            y >= bottom_edge_exclusive
                        };

                        if should_scroll_up {
                            session.chat_view.scroll_up(1);
                            scrolled_lines = scrolled_lines.saturating_add(1);
                        } else if should_scroll_down {
                            session.chat_view.scroll_down(1);
                            scrolled_lines = scrolled_lines.saturating_add(1);
                        }
                        session
                            .chat_view
                            .update_selection(x, y, chat_area, session.is_processing);
                        handled = true;
                    }
                }
            }
        }

        if scrolled_lines > 0 {
            self.record_scroll(scrolled_lines);
        }

        handled
    }

    fn handle_selection_end(&mut self) -> Option<Vec<Effect>> {
        let target = self.state.selection_drag.take()?;
        let mut copied_text = None;
        let mut should_clear_selection = false;
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            let has_selection = match target {
                SelectionDragTarget::Input => session.input_box.finalize_selection(),
                SelectionDragTarget::Chat => session.chat_view.finalize_selection(),
            };

            if has_selection && self.config.selection.auto_copy_selection {
                copied_text = Self::selection_text_for_target(session, target);
                should_clear_selection =
                    copied_text.is_some() && self.config.selection.clear_selection_after_copy;
            }
            if should_clear_selection {
                Self::clear_selection_for_target(session, target);
            }
        }

        let mut effects = Vec::new();
        if let Some(text) = copied_text {
            effects.push(Effect::CopyToClipboard(text));
            self.state
                .set_timed_footer_message("Copied selection".to_string(), Duration::from_secs(5));
        }

        Some(effects)
    }

    fn selection_text_for_target(
        session: &mut AgentSession,
        target: SelectionDragTarget,
    ) -> Option<String> {
        match target {
            SelectionDragTarget::Input => session.input_box.selected_text(),
            SelectionDragTarget::Chat => session.chat_view.copy_selection(),
        }
    }

    fn clear_selection_for_target(session: &mut AgentSession, target: SelectionDragTarget) {
        match target {
            SelectionDragTarget::Input => session.input_box.clear_selection(),
            SelectionDragTarget::Chat => session.chat_view.clear_selection(),
        }
    }

    fn scrollbar_target_at(&mut self, x: u16, y: u16) -> Option<ScrollDragTarget> {
        let mut targets = Vec::new();

        if self.state.input_mode == InputMode::ShowingHelp {
            targets.push(ScrollDragTarget::HelpDialog);
        } else if self.state.input_mode == InputMode::PickingProject
            && self.state.project_picker_state.is_visible()
        {
            targets.push(ScrollDragTarget::ProjectPicker);
        } else if self.state.input_mode == InputMode::ImportingSession
            && self.state.session_import_state.is_visible()
        {
            targets.push(ScrollDragTarget::SessionImport);
        } else if self.state.view_mode == ViewMode::RawEvents {
            targets.push(ScrollDragTarget::RawEventsDetail);
            targets.push(ScrollDragTarget::RawEventsList);
        } else {
            if self.state.input_mode != InputMode::Command {
                targets.push(ScrollDragTarget::Input);
            }
            targets.push(ScrollDragTarget::Chat);
        }

        for target in targets {
            if let Some(metrics) = self.scrollbar_metrics_for_target(target) {
                if Self::point_in_rect(x, y, metrics.area) {
                    return Some(target);
                }
            }
        }

        None
    }

    fn apply_scrollbar_drag(&mut self, target: ScrollDragTarget, y: u16) -> bool {
        let Some(metrics) = self.scrollbar_metrics_for_target(target) else {
            return false;
        };

        let new_offset =
            scrollbar_offset_from_point(y, metrics.area, metrics.total, metrics.visible);

        match target {
            ScrollDragTarget::Chat => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.set_scroll_from_top(
                        new_offset,
                        metrics.total,
                        metrics.visible,
                    );
                }
            }
            ScrollDragTarget::Input => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session
                        .input_box
                        .set_scroll_offset(new_offset, metrics.total, metrics.visible);
                }
            }
            ScrollDragTarget::HelpDialog => {
                let max_scroll = metrics.total.saturating_sub(metrics.visible);
                self.state.help_dialog_state.scroll_offset = new_offset.min(max_scroll);
            }
            ScrollDragTarget::ProjectPicker => {
                let max_scroll = metrics.total.saturating_sub(metrics.visible);
                self.state.project_picker_state.list.scroll_offset = new_offset.min(max_scroll);
            }
            ScrollDragTarget::SessionImport => {
                let max_scroll = metrics.total.saturating_sub(metrics.visible);
                self.state.session_import_state.list.scroll_offset = new_offset.min(max_scroll);
            }
            ScrollDragTarget::RawEventsList => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.set_list_scroll_offset(
                        new_offset,
                        metrics.total,
                        metrics.visible,
                    );
                }
            }
            ScrollDragTarget::RawEventsDetail => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.set_detail_scroll_offset(
                        new_offset,
                        metrics.total,
                        metrics.visible,
                    );
                }
            }
        }

        true
    }

    fn scrollbar_metrics_for_target(
        &mut self,
        target: ScrollDragTarget,
    ) -> Option<ScrollbarMetrics> {
        let (width, height) = crossterm::terminal::size().unwrap_or((0, 0));
        let screen = Rect::new(0, 0, width, height);

        match target {
            ScrollDragTarget::HelpDialog => self.state.help_dialog_state.scrollbar_metrics(screen),
            ScrollDragTarget::ProjectPicker => {
                self.state.project_picker_state.scrollbar_metrics(screen)
            }
            ScrollDragTarget::SessionImport => {
                self.state.session_import_state.scrollbar_metrics(screen)
            }
            ScrollDragTarget::Chat => {
                let area = self.state.chat_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                session.chat_view.scrollbar_metrics(
                    area,
                    session.is_processing,
                    session.queued_messages.len(),
                )
            }
            ScrollDragTarget::Input => {
                let area = self.state.input_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                session.input_box.scrollbar_metrics(area)
            }
            ScrollDragTarget::RawEventsList => {
                let area = self.state.raw_events_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                let RawEventsScrollbarMetrics { list, .. } =
                    session.raw_events_view.scrollbar_metrics(area);
                list
            }
            ScrollDragTarget::RawEventsDetail => {
                let area = self.state.raw_events_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                let RawEventsScrollbarMetrics { detail, .. } =
                    session.raw_events_view.scrollbar_metrics(area);
                detail
            }
        }
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
            self.state.input_mode = self.dismiss_confirmation_dialog();
            return Ok(effects);
        }

        // Handle model selector clicks first (it's a modal dialog)
        if self.state.input_mode == InputMode::SelectingModel
            && self.state.model_selector_state.is_visible()
        {
            if let Some(effect) = self.handle_model_selector_click(x, y) {
                effects.push(effect);
            }
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
                let base_dir = self
                    .app_state_dao
                    .as_ref()
                    .and_then(|dao| dao.get("projects_base_dir").ok().flatten());

                self.state.close_overlays();
                if let Some(base_dir_str) = base_dir {
                    let base_path = if base_dir_str.starts_with('~') {
                        dirs::home_dir()
                            .map(|h| h.join(base_dir_str[1..].trim_start_matches('/')))
                            .unwrap_or_else(|| PathBuf::from(&base_dir_str))
                    } else {
                        PathBuf::from(&base_dir_str)
                    };
                    self.state.project_picker_state.show(base_path);
                    self.state.input_mode = InputMode::PickingProject;
                } else {
                    self.state.base_dir_dialog_state.show();
                    self.state.input_mode = InputMode::SettingBaseDir;
                }
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
                        return Some(self.start_workspace_creation(parent_id));
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
        for session in sessions {
            pr_numbers.push(session.pr_number);
            processing_flags.push(session.is_processing);
            attention_flags.push(session.needs_attention);
        }

<<<<<<< HEAD
        TabBar::new(
            self.state.tab_manager.tab_names(),
            self.state.tab_manager.active_index(),
        )
        .focused(focused)
        .with_tab_states(pr_numbers, processing_flags, attention_flags)
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
                    self.state
                        .agent_selector_state
                        .show_with_default(self.config.default_agent);
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
        // Status bar format (Claude): "  Build  ModelName Agent"
        // Status bar format (Codex):  "  ModelName Agent"
        //
        // Layout with positions:
        // - 2 chars: leading spaces
        // - For Claude: 5 chars ("Build") or 4 chars ("Plan") + 2 chars separator
        // - Model name (variable length)
        // - 1 char space + Agent name

        let relative_x = x.saturating_sub(status_bar_area.x) as usize;

        // Extract info from session in a limited scope
        let (is_claude, mode_width, model_width, agent_width, model) = {
            let session = self.state.tab_manager.active_session()?;

            let is_claude = session.agent_type == AgentType::Claude;
            let mode_width = if is_claude {
                session.agent_mode.display_name().len()
            } else {
                0
            };

            // Calculate model display name
            let model_id = session
                .model
                .clone()
                .unwrap_or_else(|| crate::agent::ModelRegistry::default_model(session.agent_type));
            let model_display =
                crate::agent::ModelRegistry::find_model(session.agent_type, &model_id)
                    .map(|m| m.display_name.to_string())
                    .unwrap_or(model_id);
            let model_width = model_display.len();

            let agent_display = session.agent_type.display_name();
            let agent_width = agent_display.len();
            let model = session.model.clone();

            (is_claude, mode_width, model_width, agent_width, model)
        };

        // Calculate positions with 1 char padding on each side
        // Leading spaces: 2 chars
        let leading: usize = 2;

        if is_claude {
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
            } else if relative_x >= model_start && relative_x < model_end {
                // Click on model/agent area - open model selector
                self.state.close_overlays();
                let defaults = self.model_selector_defaults();
                self.state.model_selector_state.show(model, defaults);
                self.state.input_mode = InputMode::SelectingModel;
            }
        } else {
            // Codex: no mode area, just model/agent
            let model_start = leading.saturating_sub(1); // 1 char before model
            let model_end = leading + model_width + 1 + agent_width + 1; // 1 char after agent

            if relative_x >= model_start && relative_x < model_end {
                self.state.close_overlays();
                let defaults = self.model_selector_defaults();
                self.state.model_selector_state.show(model, defaults);
                self.state.input_mode = InputMode::SelectingModel;
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
    fn handle_model_selector_click(&mut self, x: u16, y: u16) -> Option<Effect> {
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
            self.state.input_mode = InputMode::Normal;
            return None;
        }

        let inner = Rect {
            x: dialog_area.x + 2,
            y: dialog_area.y + 1,
            width: dialog_area.width.saturating_sub(4),
            height: dialog_area.height.saturating_sub(2),
        };

        if inner.height < 4 {
            return None;
        }

        // Layout: search, separator, list, instructions
        let list_y = inner.y + 2;
        let list_height = inner.height.saturating_sub(3);

        if y >= list_y && y < list_y + list_height {
            let clicked_row = (y - list_y) as usize;
            if self.state.model_selector_state.select_at_row(clicked_row) {
                if let Some(model) = self.state.model_selector_state.selected_model().cloned() {
                    let required_tool = Self::required_tool(model.agent_type);
                    if !self.tools.is_available(required_tool) {
                        self.show_missing_tool(
                            required_tool,
                            format!(
                                "{} is required to use this model.",
                                required_tool.display_name()
                            ),
                        );
                        return None;
                    }

                    if let Some(session) = self.state.tab_manager.active_session_mut() {
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
                self.state.input_mode = InputMode::Normal;
            }
        }

        None
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
        let dialog_height = 7 + list_height;

        // Calculate dialog position (centered)
        let dialog_x = screen_width.saturating_sub(dialog_width) / 2;
        let dialog_y = screen_height.saturating_sub(dialog_height) / 2;

        // Inner area is dialog minus border (1 on each side)
        let inner_x = dialog_x + 1;
        let inner_y = dialog_y + 1;
        let inner_width = dialog_width.saturating_sub(2);

        // List area starts at row 3 within inner area (after search, search input, separator)
        // Layout: [0] search label, [1] search input, [2] separator, [3..] list
        let list_y = inner_y + 3;
        let list_height_actual = dialog_height.saturating_sub(7); // Same as list_height

        // Check if click is in the list area
        if x >= inner_x
            && x < inner_x + inner_width
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
        let hints: Vec<(&str, &str)> = match self.state.view_mode {
            ViewMode::Chat => GlobalFooter::chat_hints(),
            ViewMode::RawEvents => GlobalFooter::raw_events_hints(),
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

        // Determine context from current mode
        let context = KeyContext::from_input_mode(self.state.input_mode, self.state.view_mode);

        // Look up action in keybinding config
        self.config
            .keybindings
            .get_action(&key_combo, context)
            .cloned()
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
            AppEvent::WorkspaceCreated { result } => match result {
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
            },
            AppEvent::ForkWorkspaceCreated { result } => match result {
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
                                    if let Some(dao) = &self.fork_seed_dao {
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
                            let cleanup_msg =
                                self.cleanup_fork_workspace(created.workspace_id, created.repo_id);
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
                            if let Some(dao) = &self.fork_seed_dao {
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
            },
            AppEvent::WorkspaceArchived { result } => match result {
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
                }
                Err(err) => {
                    self.show_error("Archive Failed", &err);
                }
            },
            AppEvent::ProjectRemoved { result } => {
                for workspace_id in &result.workspace_ids {
                    self.close_tabs_for_workspace(*workspace_id);
                }

                if !result.errors.is_empty() {
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
                    self.state.input_mode = InputMode::SidebarNavigation;
                } else {
                    self.state.sidebar_state.tree_state.selected = 0;
                    self.state.show_first_time_splash = true;
                    self.state.input_mode = InputMode::Normal;
                }
            }
            AppEvent::AgentStarted { session_id, pid } => {
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
                // Update all sessions with this workspace
                for session in self.state.tab_manager.sessions_mut() {
                    if session.workspace_id == Some(workspace_id) {
                        session.status_bar.set_branch_name(branch.clone());
                    }
                }
                // Always update sidebar data (including None for detached HEAD)
                self.state
                    .sidebar_data
                    .update_workspace_branch(workspace_id, branch);
            }
        }
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
                | AgentEvent::ToolStarted(_)
                | AgentEvent::ToolCompleted(_)
                | AgentEvent::CommandOutput(_)
                | AgentEvent::TurnCompleted(_)
                | AgentEvent::TurnFailed(_)
        );

        // Track whether we need to stop footer spinner (done after session borrow ends)
        let mut should_stop_footer_spinner = false;
        let mut pending_sidebar_pr_update: Option<(Uuid, PrStatus)> = None;

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
            let event_type = event.event_type_name();
            let raw_json = serde_json::to_value(&event).unwrap_or_default();
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
                    // Update processing state to show tool name
                    session.set_processing_state(ProcessingState::ToolUse(tool.tool_name.clone()));
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
                    if err.is_fatal {
                        session.stop_processing();
                        session.chat_view.finalize_streaming();
                        session.tools_in_flight = 0;
                        session.set_processing_state(ProcessingState::Thinking);
                        // Only stop footer spinner if this is the active tab
                        if is_active_tab {
                            should_stop_footer_spinner = true;
                        }
                    }
                }
                AgentEvent::TokenUsage(usage_event) => {
                    session.update_context_usage(&usage_event);

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

        // Stop footer spinner after session borrow is released
        if should_stop_footer_spinner {
            self.state.stop_footer_spinner();
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
        self.submit_prompt_for_tab(tab_index, prompt, images, image_placeholders, false)
    }

    fn submit_prompt_hidden(
        &mut self,
        prompt: String,
        images: Vec<PathBuf>,
        image_placeholders: Vec<String>,
    ) -> anyhow::Result<Vec<Effect>> {
        let tab_index = self.state.tab_manager.active_index();
        self.submit_prompt_for_tab(tab_index, prompt, images, image_placeholders, true)
    }

    fn submit_prompt_for_tab(
        &mut self,
        tab_index: usize,
        prompt: String,
        mut images: Vec<PathBuf>,
        image_placeholders: Vec<String>,
        hidden: bool,
    ) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();

        if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
            Self::flush_pending_agent_output(session);
        }

        // Extract session info in a limited borrow scope
        // NOTE: We don't take() resume_session_id here because early returns below
        // (e.g., working_dir validation) would consume it incorrectly. We only
        // consume resume_session_id later when we're committed to spawning the agent.
        let (
            agent_type,
            agent_mode,
            model,
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
            // Use agent_session_id if available (set by agent after first prompt)
            // Fall back to resume_session_id (clone, don't take - we consume it later)
            let session_id_to_use = session
                .agent_session_id
                .clone()
                .or_else(|| session.resume_session_id.clone());
            // Use session's working_dir if set, otherwise fall back to config
            let working_dir = session
                .working_dir
                .clone()
                .unwrap_or_else(|| self.config.working_dir.clone());
            let session_id = session.id;

            (
                agent_type,
                agent_mode,
                model,
                session_id_to_use,
                working_dir,
                !has_visible_user_message,
                session_id,
            )
        };

        let mut prompt = prompt;

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

        // Capture original user message for title generation BEFORE agent-specific transformations
        // (e.g., Codex placeholder stripping, Claude image-path appends)
        let prompt_for_title = prompt.clone();
        let working_dir_for_title = working_dir.clone();

        // Add user message to chat and start processing (after validation passes)
        // For hidden prompts (like fork seeds), skip showing in chat and pending_user_message
        if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
            if !hidden {
                let display = MessageDisplay::User {
                    content: prompt.clone(),
                };
                session.chat_view.push(display.to_chat_message());
                // Store pending message for persistence (cleared on agent confirmation)
                session.pending_user_message = Some(prompt.clone());
            }
            session.start_processing();
        }
        if self.state.tab_manager.active_index() == tab_index {
            self.state.start_footer_spinner(None);
        }

        // Start agent
        // Strip placeholders unconditionally for Codex (handles edge case where user
        // manually typed placeholder text without attaching images)
        if agent_type == AgentType::Codex {
            prompt = Self::strip_image_placeholders(prompt, &image_placeholders);
        }
        if agent_type == AgentType::Claude && !images.is_empty() {
            prompt = Self::append_image_paths_to_prompt(prompt, &images);
            images.clear();
        }

        if prompt.trim().is_empty() && images.is_empty() {
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

        // Record user input for debug view (post-processing)
        // For hidden prompts (like fork seeds), redact content to avoid storing ~500KB
        let mut debug_payload = serde_json::json!({
            "agent_type": agent_type.as_str(),
            "hidden": hidden,
        });
        if hidden {
            debug_payload["prompt_len"] = serde_json::json!(prompt.len());
            debug_payload["prompt_hash"] =
                serde_json::json!(Self::compute_seed_prompt_hash(&prompt));
        } else {
            debug_payload["prompt"] = serde_json::json!(&prompt);
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

        let mut config = AgentStartConfig::new(prompt, working_dir)
            .with_tools(self.config.claude_allowed_tools.clone())
            .with_images(images)
            .with_agent_mode(agent_mode);

        // Add model if specified
        if let Some(model_id) = model {
            config = config.with_model(model_id);
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
        let mut queued_handled = false;

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

            let effective_mode = if mode == QueuedMessageMode::Steer
                && self.config.steer.behavior == crate::config::SteerBehavior::Soft
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

                if mode == QueuedMessageMode::Steer && effective_mode == QueuedMessageMode::Steer {
                    match self.config.steer.fallback {
                        crate::config::SteerFallback::Interrupt => {
                            let (text, image_paths, image_placeholders) =
                                queued_to_submission(&queued);
                            immediate_submit = Some((text, image_paths, image_placeholders));
                            interrupt_before_submit = true;
                            queued_handled = true;
                        }
                        crate::config::SteerFallback::Prompt => {
                            session.queue_message(queued.clone());
                            prompt_fallback_id = Some(queued.id);
                            footer_message = Some(
                                "Steering queued · press Enter to confirm interrupt".to_string(),
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

    fn append_image_paths_to_prompt(prompt: String, images: &[PathBuf]) -> String {
        if images.is_empty() {
            return prompt;
        }

        let mut lines: Vec<String> = Vec::new();
        if !prompt.trim().is_empty() {
            lines.push(prompt.trim_end().to_string());
        }

        lines.push("Image file(s):".to_string());
        for path in images {
            lines.push(format!("- {}", path.display()));
        }

        lines.join("\n")
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

    /// Handle Ctrl+P: Open existing PR or create new one
    fn handle_pr_action(&mut self) -> Option<Effect> {
        let tab_index = self.state.tab_manager.active_index();
        let session = self.state.tab_manager.active_session()?;

        let working_dir = match &session.working_dir {
            Some(d) => d.clone(),
            None => return None, // No working dir
        };

        // Show loading dialog immediately
        self.state.close_overlays();
        self.state
            .confirmation_dialog_state
            .show_loading("Create Pull Request", "Checking repository status...");
        self.state.input_mode = InputMode::Confirming;

        Some(Effect::PrPreflight {
            tab_index,
            working_dir,
        })
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

        if self.fork_seed_dao.is_none() {
            self.show_error("Cannot Fork", "Fork metadata store unavailable.");
            return;
        }

        let workspace_dao = match &self.workspace_dao {
            Some(dao) => dao,
            None => {
                self.show_error("Cannot Fork", "Workspace database unavailable.");
                return;
            }
        };

        let Ok(Some(workspace)) = workspace_dao.get_by_id(parent_workspace_id) else {
            self.show_error("Cannot Fork", "Workspace not found.");
            return;
        };

        // Get actual current branch for display (may differ from stored workspace.branch)
        let base_branch = self
            .worktree_manager
            .get_current_branch(&workspace.path)
            .unwrap_or_else(|_| workspace.branch.clone());

        let seed_prompt = Self::build_fork_seed_prompt(session.chat_view.messages());

        let model_id = session
            .model
            .clone()
            .unwrap_or_else(|| ModelRegistry::default_model(session.agent_type));
        let context_window = ModelRegistry::context_window(session.agent_type, &model_id);
        let token_estimate = Self::estimate_tokens(&seed_prompt);
        let usage_pct = if context_window > 0 {
            (token_estimate as f64 / context_window as f64) * 100.0
        } else {
            0.0
        };

        let mut warnings = Vec::new();
        let mut has_dirty = false;

        if let Ok(status) = self.worktree_manager.get_branch_status(&workspace.path) {
            if status.is_dirty {
                has_dirty = true;
                if let Some(desc) = &status.dirty_description {
                    warnings.push(desc.clone());
                } else {
                    warnings.push("Uncommitted changes detected".to_string());
                }
                warnings.push("Commit before forking to preserve changes.".to_string());
            }
        }

        if usage_pct >= 100.0 {
            warnings.push(format!(
                "Seed exceeds context window ({} / {} tokens, ~{:.0}%).",
                token_estimate, context_window, usage_pct
            ));
        } else if usage_pct >= 80.0 {
            warnings.push(format!(
                "Seed uses ~{:.0}% of context window ({} / {}).",
                usage_pct, token_estimate, context_window
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
            base_branch,
            token_estimate,
            context_window,
            usage_pct
        );

        self.state.pending_fork_request = Some(PendingForkRequest {
            agent_type: session.agent_type,
            agent_mode: session.agent_mode,
            model: session.model.clone(),
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

        self.state.close_overlays();
        self.state.confirmation_dialog_state.show(
            "Fork session?",
            message,
            warnings,
            confirmation_type,
            "Fork",
            Some(ConfirmationContext::ForkSession {
                parent_workspace_id,
                base_branch: base_branch.clone(),
            }),
        );
        self.state.input_mode = InputMode::Confirming;
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

        let fork_seed_dao = match &self.fork_seed_dao {
            Some(dao) => dao,
            None => {
                self.show_error("Fork Failed", "Fork metadata store unavailable.");
                self.state.pending_fork_request = None;
                return None;
            }
        };

        let seed_prompt_hash = Self::compute_seed_prompt_hash(&pending.seed_prompt);
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
            .workspace_dao
            .as_ref()
            .ok_or_else(|| anyhow!("Workspace database unavailable."))?;

        let repo_dao = self
            .repo_dao
            .as_ref()
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
                    self.state.tab_manager.close_tab(new_index);
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
                    self.state.tab_manager.close_tab(new_index);
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

        let workspace_dao = self.workspace_dao.as_ref()?;
        let repo_dao = self.repo_dao.as_ref()?;

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
                        if let Err(prune_err) = self.worktree_manager.prune_worktrees(base_path) {
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
            } else if let Err(e) = self
                .worktree_manager
                .remove_worktree(base_path, &workspace.path)
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
                if let Err(e) = self
                    .worktree_manager
                    .delete_branch(base_path, &workspace.branch)
                {
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
        self.submit_prompt_for_tab(target_tab_index, prompt, Vec::new(), Vec::new(), false)
    }

    fn build_queue_lines(
        session: &AgentSession,
        width: u16,
        input_mode: InputMode,
    ) -> Option<Vec<Line<'static>>> {
        if session.queued_messages.is_empty() {
            return None;
        }

        let max_width = width.saturating_sub(2) as usize;
        let selected = if input_mode == InputMode::QueueEditing {
            session.queue_selection
        } else {
            None
        };

        let mut lines = Vec::new();
        for (idx, msg) in session.queued_messages.iter().enumerate() {
            let raw = msg.text.trim();
            let mut preview = raw.lines().next().unwrap_or("").trim().to_string();
            if preview.is_empty() {
                preview = "<empty>".to_string();
            }
            if raw.contains('\n') {
                preview.push_str(" ...");
            }
            let mut text = format!("{}: {}", msg.mode.label(), preview);
            if !msg.images.is_empty() {
                let count = msg.images.len();
                let suffix = if count == 1 { "image" } else { "images" };
                text.push_str(&format!(" [{} {}]", count, suffix));
            }

            if max_width > 0 {
                text = truncate_queue_line(&text, max_width);
            }

            let prefix = if selected == Some(idx) { "› " } else { "  " };
            let line_text = format!("{prefix}{text}");
            let style = if selected == Some(idx) {
                Style::default()
                    .fg(text_primary())
                    .bg(bg_highlight())
                    .add_modifier(Modifier::ITALIC)
            } else {
                Style::default()
                    .fg(text_muted())
                    .add_modifier(Modifier::ITALIC)
            };
            lines.push(Line::from(Span::styled(line_text, style)));
        }

        Some(lines)
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

    fn clamp_queue_selection(session: &mut AgentSession) {
        if session.queued_messages.is_empty() {
            session.queue_selection = None;
            return;
        }
        if let Some(idx) = session.queue_selection {
            let max = session.queued_messages.len().saturating_sub(1);
            if idx > max {
                session.queue_selection = Some(max);
            }
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
            let (text, images, placeholders) = queued_to_submission(&message);
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
        let (queue_mode, queue_delivery) = (self.config.queue.mode, self.config.queue.delivery);

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

        let (prompt, images, placeholders) = build_queued_submission(&queued, queue_delivery);
        effects.extend(self.submit_prompt_for_tab(
            tab_index,
            prompt,
            images,
            placeholders,
            false,
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
            let sidebar = Sidebar::new(&self.state.sidebar_data);
            ratatui::widgets::StatefulWidget::render(
                sidebar,
                sidebar_area,
                f.buffer_mut(),
                &mut self.state.sidebar_state,
            );
        }

        match self.state.view_mode {
            ViewMode::Chat => {
                // Handle empty state - no tabs open
                if self.state.tab_manager.is_empty() {
                    use crate::ui::components::{text_muted, FooterContext};
                    use ratatui::style::Style;
                    use ratatui::text::{Line, Span};
                    use ratatui::widgets::{Paragraph, Widget};

                    // Layout with tab bar, content, and footer
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1), // Tab bar
                            Constraint::Min(5),    // Content area
                            Constraint::Length(1), // Footer
                        ])
                        .split(content_area);

                    // Store areas for mouse hit-testing
                    self.state.tab_bar_area = Some(chunks[0]);
                    self.state.chat_area = None;
                    self.state.raw_events_area = None;
                    self.state.input_area = None;
                    self.state.status_bar_area = None;
                    self.state.footer_area = Some(chunks[2]);

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
                        let dialog = ConfirmationDialog::new(&self.state.confirmation_dialog_state);
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
                        let dialog = MissingToolDialog::new(&self.state.missing_tool_dialog_state);
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

                    // Draw command palette (on top of everything)
                    if self.state.command_palette_state.is_visible() {
                        CommandPalette::new().render(
                            size,
                            f.buffer_mut(),
                            &self.state.command_palette_state,
                        );
                    }

                    // Draw footer for empty state (sidebar-aware)
                    let footer_context = if self.state.input_mode == InputMode::SidebarNavigation {
                        FooterContext::Sidebar
                    } else {
                        FooterContext::Empty
                    };
                    let footer = GlobalFooter::for_context(footer_context)
                        .with_spinner(self.state.footer_spinner.as_ref())
                        .with_message(self.state.footer_message.as_deref());
                    footer.render(chunks[2], f.buffer_mut());

                    return;
                }

                // Margins for input area (constants to avoid duplication)
                const INPUT_MARGIN_LEFT: u16 = 2;
                const INPUT_MARGIN_RIGHT: u16 = 2;
                let input_total_margin = INPUT_MARGIN_LEFT + INPUT_MARGIN_RIGHT;

                // Calculate dynamic input height (max 30% of screen)
                let max_input_height = (content_area.height as f32 * 0.30).ceil() as u16;
                let input_width = content_area.width.saturating_sub(input_total_margin);
                let input_height = if let Some(session) = self.state.tab_manager.active_session() {
                    session
                        .input_box
                        .desired_height(max_input_height, input_width)
                } else {
                    3 // Minimum height
                };

                // Chat layout with session header, input box, status bar, and gap
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),            // Tab bar
                        Constraint::Length(1),            // Session header
                        Constraint::Min(5),               // Chat view
                        Constraint::Length(input_height), // Input box (dynamic)
                        Constraint::Length(1),            // Status bar
                        Constraint::Length(1),            // Gap row before footer
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
                        let right_start = row_area.x + row_area.width.saturating_sub(right_width);
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
                use crate::ui::components::status_bar_bg;
                for x in gap_area_inner.x..gap_area_inner.x + gap_area_inner.width {
                    buf[(x, gap_area_inner.y)]
                        .set_char('▀')
                        .set_fg(status_bar_bg());
                }

                // Store layout areas for mouse hit-testing
                self.state.tab_bar_area = Some(tab_bar_chunk);
                self.state.chat_area = Some(chat_chunk);
                self.state.raw_events_area = None;
                self.state.input_area = Some(input_area_inner);
                self.state.status_bar_area = Some(status_bar_area_inner);
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
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    // Render chat with thinking indicator if processing
                    let thinking_line = if session.is_processing {
                        Some(session.thinking_indicator.render())
                    } else {
                        None
                    };
                    let input_mode = self.state.input_mode;
                    let queue_lines =
                        Self::build_queue_lines(session, chat_chunk.width, input_mode);
                    session.chat_view.render_with_indicator(
                        chat_chunk,
                        f.buffer_mut(),
                        thinking_line,
                        queue_lines,
                    );

                    // Render input box (not in command mode)
                    if !is_command_mode {
                        session.input_box.render(input_area_inner, f.buffer_mut());
                    }
                    // Update status bar with performance metrics
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
                        .render(status_bar_area_inner, f.buffer_mut());

                    // Set cursor position (accounting for scroll)
                    if self.state.input_mode == InputMode::Normal {
                        let scroll_offset = session.input_box.scroll_offset();
                        let (cx, cy) = session
                            .input_box
                            .cursor_position(input_area_inner, scroll_offset);
                        f.set_cursor_position((cx, cy));
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

        // Draw agent selector dialog if needed
        if self.state.agent_selector_state.is_visible() {
            let selector = AgentSelector::new();
            selector.render(size, f.buffer_mut(), &self.state.agent_selector_state);
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

        // Draw command palette (on top of everything)
        if self.state.command_palette_state.is_visible() {
            CommandPalette::new().render(size, f.buffer_mut(), &self.state.command_palette_state);
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

fn truncate_queue_line(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let ellipsis = "...";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis);
    let current_width = UnicodeWidthStr::width(text);
    if current_width <= max_width {
        return text.to_string();
    }
    if max_width <= ellipsis_width {
        return ellipsis[..max_width.min(ellipsis.len())].to_string();
    }
    let target = max_width - ellipsis_width;
    let mut width = 0;
    let mut result = String::new();
    for c in text.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if width + char_width > target {
            break;
        }
        result.push(c);
        width += char_width;
    }
    result.push_str(ellipsis);
    result
}

fn queued_to_submission(message: &QueuedMessage) -> (String, Vec<PathBuf>, Vec<String>) {
    let image_paths = message.images.iter().map(|img| img.path.clone()).collect();
    let placeholders = message
        .images
        .iter()
        .map(|img| img.placeholder.clone())
        .collect();
    (message.text.clone(), image_paths, placeholders)
}

fn build_queued_submission(
    messages: &[QueuedMessage],
    delivery: crate::config::QueueDelivery,
) -> (String, Vec<PathBuf>, Vec<String>) {
    let mut lines = Vec::new();
    if messages.len() == 1 || delivery == crate::config::QueueDelivery::Concat {
        for msg in messages {
            lines.push(msg.text.trim().to_string());
        }
    } else {
        for (idx, msg) in messages.iter().enumerate() {
            lines.push(format!("[Queued {} of {}]", idx + 1, messages.len()));
            lines.push(msg.text.trim().to_string());
        }
    }

    let mut image_paths = Vec::new();
    let mut placeholders = Vec::new();
    for msg in messages {
        for img in &msg.images {
            image_paths.push(img.path.clone());
            placeholders.push(img.placeholder.clone());
        }
    }

    (lines.join("\n\n"), image_paths, placeholders)
}

struct SessionStateSnapshot {
    tabs: Vec<SessionTab>,
    active_tab_index: usize,
    sidebar_visible: bool,
    tree_selected_index: usize,
    collapsed_repo_ids: Vec<uuid::Uuid>,
}

/// Async helper for generating title and branch name
async fn generate_title_and_branch_impl(
    tools: ToolAvailability,
    user_message: String,
    working_dir: PathBuf,
    workspace_id: Option<uuid::Uuid>,
    current_branch: String,
    worktree_manager: WorktreeManager,
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
        title: sanitize_title(&metadata.title),
        new_branch,
        workspace_id,
        tool_used: metadata.tool_used.clone(),
        used_fallback: metadata.used_fallback,
    })
}

/// Sanitize a generated title: trim whitespace, remove control characters and newlines,
/// enforce max length, and provide fallback for empty titles.
fn sanitize_title(title: &str) -> String {
    const MAX_TITLE_LENGTH: usize = 200;
    const FALLBACK_TITLE: &str = "Untitled task";

    // Collapse all whitespace (including newlines) to single spaces and trim
    let sanitized: String = title
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Enforce max length
    let truncated = if sanitized.chars().count() > MAX_TITLE_LENGTH {
        sanitized.chars().take(MAX_TITLE_LENGTH).collect()
    } else {
        sanitized
    };

    // Fallback for empty titles
    if truncated.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::AssistantMessageEvent;
    use crate::agent::AgentType;
    use crate::config::Config;
    use crate::ui::components::MessageRole;
    use crate::ui::session::AgentSession;
    use crate::util::ToolAvailability;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn build_test_app_with_sessions(session_ids: &[Uuid]) -> App {
        let config = Config::default();
        let tools = ToolAvailability::default();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new(10);

        for session_id in session_ids {
            let mut session = AgentSession::new(AgentType::Codex);
            session.id = *session_id;
            state.tab_manager.add_session(session);
        }

        App {
            config,
            tools,
            state,
            claude_runner: Arc::new(ClaudeCodeRunner::new()),
            codex_runner: Arc::new(CodexCliRunner::new()),
            event_tx,
            event_rx,
            repo_dao: None,
            workspace_dao: None,
            app_state_dao: None,
            session_tab_dao: None,
            fork_seed_dao: None,
            worktree_manager: WorktreeManager::new(),
            git_tracker: None,
        }
    }

    /// Helper to check if a colon keypress should trigger command mode.
    /// This mirrors the logic in handle_key_event (lines 572-601).
    fn should_trigger_command_mode(
        key_code: KeyCode,
        key_modifiers: KeyModifiers,
        input_mode: InputMode,
        input_box_content: &str,
    ) -> bool {
        key_code == KeyCode::Char(':')
            && key_modifiers.is_empty()
            && input_box_content.is_empty() // Only trigger when input is empty
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
                    | InputMode::SelectingTheme
            )
    }

    #[test]
    fn test_colon_triggers_command_mode_on_empty_input() {
        // Typing ":" on empty input SHOULD trigger command mode
        let result = should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::Normal,
            "", // empty input
        );
        assert!(result, "Colon should trigger command mode on empty input");
    }

    #[test]
    fn test_colon_with_modifiers_does_not_trigger_command_mode() {
        // Typing "Shift+:" should NOT trigger command mode
        let result = should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::SHIFT,
            InputMode::Normal,
            "",
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
        let result = should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::Normal,
            "hello", // input already has content
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
        let result = should_trigger_command_mode(
            KeyCode::Char(':'),
            KeyModifiers::NONE,
            InputMode::Normal,
            "localhost", // input has content from paste
        );

        assert!(
            !result,
            "Pasting 'localhost:8080' should not trigger command mode at ':'"
        );
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

        let prompt = App::build_fork_seed_prompt(&messages);

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
}
