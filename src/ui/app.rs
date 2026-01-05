use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
        KeyboardEnhancementFlags, MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Frame, Terminal,
};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

use crate::agent::{
    load_claude_history_with_debug, load_codex_history_with_debug, AgentEvent, AgentRunner,
    AgentStartConfig, AgentType, ClaudeCodeRunner, CodexCliRunner, HistoryDebugEntry,
    MessageDisplay, SessionId,
};
use crate::config::{
    parse_action, parse_key_notation, Config, KeyCombo, KeyContext, COMMAND_NAMES,
};
use crate::data::{
    AppStateStore, Database, Repository, RepositoryStore, SessionTab, SessionTabStore,
    WorkspaceStore,
};
use crate::git::{PrManager, WorktreeManager};
use crate::ui::action::Action;
use crate::ui::app_state::{AppState, ScrollDragTarget};
use crate::ui::clipboard_paste::paste_image_to_temp_png;
use crate::ui::components::{
    scrollbar_offset_from_point, AddRepoDialog, AgentSelector, BaseDirDialog, ChatMessage,
    ConfirmationContext, ConfirmationDialog, ConfirmationType, ErrorDialog, EventDirection,
    GlobalFooter, HelpDialog, ModelSelector, ProcessingState, ProjectPicker, RawEventsClick,
    RawEventsScrollbarMetrics, ScrollbarMetrics, SessionImportPicker, Sidebar, SidebarData, TabBar,
};
use crate::ui::effect::Effect;
use crate::ui::events::{
    AppEvent, InputMode, RemoveProjectResult, ViewMode, WorkspaceArchived, WorkspaceCreated,
};
use crate::ui::session::AgentSession;

/// Main application state
pub struct App {
    /// Application configuration
    config: Config,
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
    /// Worktree manager
    worktree_manager: WorktreeManager,
}

impl App {
    pub fn new(config: Config) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Initialize database and DAOs
        let (repo_dao, workspace_dao, app_state_dao, session_tab_dao) =
            match Database::open_default() {
                Ok(db) => {
                    let repo_dao = RepositoryStore::new(db.connection());
                    let workspace_dao = WorkspaceStore::new(db.connection());
                    let app_state_dao = AppStateStore::new(db.connection());
                    let session_tab_dao = SessionTabStore::new(db.connection());
                    (
                        Some(repo_dao),
                        Some(workspace_dao),
                        Some(app_state_dao),
                        Some(session_tab_dao),
                    )
                }
                Err(e) => {
                    eprintln!("Warning: Failed to open database: {}", e);
                    (None, None, None, None)
                }
            };

        // Initialize worktree manager with managed directory (~/.conduit/worktrees)
        let worktree_manager = WorktreeManager::with_managed_dir(crate::util::worktrees_dir());

        let mut app = Self {
            config: config.clone(),
            state: AppState::new(config.max_tabs),
            claude_runner: Arc::new(ClaudeCodeRunner::new()),
            codex_runner: Arc::new(CodexCliRunner::new()),
            event_tx,
            event_rx,
            repo_dao,
            workspace_dao,
            app_state_dao,
            session_tab_dao,
            worktree_manager,
        };

        // Load sidebar data
        app.refresh_sidebar_data();

        // Restore session state
        app.restore_session_state();

        app
    }

    /// Restore session state from database
    fn restore_session_state(&mut self) {
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
            return;
        }

        // Has repos, don't show first-time splash
        self.state.show_first_time_splash = false;

        // Try to restore saved tabs
        let Some(session_tab_dao) = &self.session_tab_dao else {
            return;
        };
        let Some(app_state_dao) = &self.app_state_dao else {
            return;
        };

        let saved_tabs = match session_tab_dao.get_all() {
            Ok(tabs) => tabs,
            Err(_) => return,
        };

        if saved_tabs.is_empty() {
            // Has repos but no saved tabs - show main UI without tabs
            return;
        }

        // Restore each tab
        for tab in saved_tabs {
            let mut session = AgentSession::new(tab.agent_type);
            session.workspace_id = tab.workspace_id;
            session.model = tab.model;
            session.pr_number = tab.pr_number.map(|n| n as u32);

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

            session.update_status();

            self.state.tab_manager.add_session(session);
        }

        // Restore active tab
        if let Ok(Some(index_str)) = app_state_dao.get("active_tab_index") {
            if let Ok(index) = index_str.parse::<usize>() {
                self.state.tab_manager.switch_to(index);
            }
        }

        // Restore sidebar visibility
        if let Ok(Some(visible_str)) = app_state_dao.get("sidebar_visible") {
            self.state.sidebar_state.visible = visible_str == "true";
        }

        // Restore expanded repos
        if let Ok(Some(expanded_str)) = app_state_dao.get("tree_expanded_repos") {
            if !expanded_str.is_empty() {
                for id_str in expanded_str.split(',') {
                    if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                        self.state.sidebar_data.expand_repo(id);
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
                SessionTab::new(
                    index as i32,
                    session.agent_type,
                    session.workspace_id,
                    session
                        .agent_session_id
                        .as_ref()
                        .map(|s| s.as_str().to_string()),
                    session.model.clone(),
                    session.pr_number.map(|n| n as i32),
                )
            })
            .collect();

        SessionStateSnapshot {
            tabs,
            active_tab_index: self.state.tab_manager.active_index(),
            sidebar_visible: self.state.sidebar_state.visible,
            tree_selected_index: self.state.sidebar_state.tree_state.selected,
            expanded_repo_ids: self.state.sidebar_data.expanded_repo_ids(),
        }
    }

    fn persist_session_state(
        snapshot: SessionStateSnapshot,
        session_tab_dao: Option<SessionTabStore>,
        app_state_dao: Option<AppStateStore>,
    ) {
        let Some(session_tab_dao) = session_tab_dao else {
            return;
        };
        let Some(app_state_dao) = app_state_dao else {
            return;
        };

        if let Err(e) = session_tab_dao.clear_all() {
            eprintln!("Warning: Failed to clear session tabs: {}", e);
            return;
        }

        for tab in &snapshot.tabs {
            if let Err(e) = session_tab_dao.create(tab) {
                eprintln!("Warning: Failed to save session tab: {}", e);
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

        let expanded_ids: Vec<String> = snapshot
            .expanded_repo_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        if let Err(e) = app_state_dao.set("tree_expanded_repos", &expanded_ids.join(",")) {
            eprintln!("Warning: Failed to save expanded repos: {}", e);
        }
    }

    /// Run the application main loop
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();

        // Enable Kitty keyboard protocol for proper Ctrl+Shift detection
        // This MUST be done before EnterAlternateScreen for proper detection
        // Supported terminals: kitty, foot, WezTerm, alacritty, Ghostty
        let keyboard_enhancement_enabled =
            if supports_keyboard_enhancement().map_or(false, |supported| supported) {
                execute!(
                    stdout,
                    PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                            | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    )
                )
                .is_ok()
            } else {
                false
            };

        if keyboard_enhancement_enabled {
            tracing::info!("Kitty keyboard protocol enabled");
        } else {
            tracing::warn!(
                "Kitty keyboard protocol NOT available - Ctrl+Shift combos may not work"
            );
        }

        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Clear screen
        terminal.clear()?;

        // Main event loop
        let result = self.event_loop(&mut terminal).await;

        // Restore terminal
        if keyboard_enhancement_enabled {
            let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
        }
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            let frame_start = Instant::now();

            // Draw UI with timing
            let draw_start = Instant::now();
            terminal.draw(|f| self.draw(f))?;
            let draw_end = Instant::now();
            self.state.metrics.draw_time = draw_end.duration_since(draw_start);
            self.state.metrics.on_draw_end(draw_end);

            // Wait for next frame (16ms target = 60 FPS)
            tokio::select! {
                // Terminal input events + tick
                _ = tokio::time::sleep(Duration::from_millis(16)) => {
                    // Measure event processing time (after sleep)
                    let event_start = Instant::now();

                    // Handle keyboard and mouse input
                    let mut pending_scroll_up = 0usize;
                    let mut pending_scroll_down = 0usize;

                    while event::poll(Duration::from_millis(0))? {
                        match event::read()? {
                            Event::Key(key) => {
                                self.flush_scroll_deltas(&mut pending_scroll_up, &mut pending_scroll_down);
                                self.dispatch_event(AppEvent::Input(Event::Key(key))).await?;
                            }
                            Event::Mouse(mouse) => {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        if self.should_route_scroll_to_chat() {
                                            self.record_chat_scroll(1);
                                        }
                                        pending_scroll_up = pending_scroll_up.saturating_add(1);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        if self.should_route_scroll_to_chat() {
                                            self.record_chat_scroll(1);
                                        }
                                        pending_scroll_down = pending_scroll_down.saturating_add(1);
                                    }
                                    _ => {
                                        self.flush_scroll_deltas(
                                            &mut pending_scroll_up,
                                            &mut pending_scroll_down,
                                        );
                                        self.dispatch_event(AppEvent::Input(Event::Mouse(mouse))).await?;
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
                    self.dispatch_event(event).await?;
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

    async fn dispatch_event(&mut self, event: AppEvent) -> anyhow::Result<()> {
        let effects = match event {
            AppEvent::Input(input) => self.handle_input_event(input).await?,
            AppEvent::Tick => {
                self.handle_tick();
                Vec::new()
            }
            _ => self.handle_app_event(event).await?,
        };

        self.run_effects(effects).await
    }

    async fn handle_input_event(&mut self, input: Event) -> anyhow::Result<Vec<Effect>> {
        match input {
            Event::Key(key) => self.handle_key_event(key).await,
            Event::Mouse(mouse) => self.handle_mouse_event(mouse).await,
            Event::Paste(text) => {
                self.handle_paste_input(text);
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn handle_tick(&mut self) {
        // Tick animations (every 6 frames = ~100ms)
        self.state.tick_count += 1;
        if self.state.tick_count % 6 != 0 {
            return;
        }

        // Advance spinner frame for PR processing indicator
        self.state.spinner_frame = self.state.spinner_frame.wrapping_add(1);

        // Tick confirmation dialog spinner (for loading state)
        self.state.confirmation_dialog_state.tick();

        // Tick session import spinner (for loading state)
        self.state.session_import_state.tick();

        if self.state.show_first_time_splash {
            // Animate splash screen
            self.state.splash_screen.tick();
        } else if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.tick();
        }
    }

    async fn handle_key_event(&mut self, key: event::KeyEvent) -> anyhow::Result<Vec<Effect>> {
        // Special handling for modes that bypass normal key processing
        if self.state.input_mode == InputMode::RemovingProject {
            // Ignore all input while removing project
            return Ok(Vec::new());
        }

        // First-time splash screen handling (only when no dialogs are visible)
        if self.state.show_first_time_splash
            && key.modifiers.is_empty()
            && !self.state.base_dir_dialog_state.is_visible()
            && !self.state.project_picker_state.is_visible()
            && !self.state.add_repo_dialog_state.is_visible()
            && self.state.input_mode != InputMode::SelectingAgent
            && self.state.input_mode != InputMode::ShowingError
        {
            match key.code {
                KeyCode::Enter => {
                    // Start new project workflow
                    return self.execute_action(Action::NewProject).await;
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    return self.execute_action(Action::Quit).await;
                }
                _ => {}
            }
        }

        if self.state.input_mode == InputMode::Normal
            && key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
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

        // Debug logging for key events (helps diagnose Kitty protocol issues)
        tracing::debug!(
            "Key event: {:?} modifiers={:?} -> KeyCombo: {}",
            key.code,
            key.modifiers,
            key_combo
        );

        // Look up action in config (context-specific first, then global)
        if let Some(action) = self.config.keybindings.get_action(&key_combo, context) {
            tracing::debug!("Matched action: {:?}", action);
            return self.execute_action(action.clone()).await;
        }

        Ok(Vec::new())
    }

    /// Execute a keybinding action
    async fn execute_action(&mut self, action: Action) -> anyhow::Result<Vec<Effect>> {
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

                if let Some(base_dir_str) = base_dir {
                    let base_path = if base_dir_str.starts_with('~') {
                        dirs::home_dir()
                            .map(|h| h.join(&base_dir_str[1..].trim_start_matches('/')))
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
            Action::OpenPr => {
                if let Some(effect) = self.handle_pr_action() {
                    effects.push(effect);
                }
            }
            Action::InterruptAgent => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.is_processing {
                        let display = MessageDisplay::System {
                            content: "Interrupted".to_string(),
                        };
                        session.chat_view.push(display.to_chat_message());
                        session.stop_processing();
                    }
                }
            }
            Action::ToggleViewMode => {
                self.state.view_mode = match self.state.view_mode {
                    ViewMode::Chat => ViewMode::RawEvents,
                    ViewMode::RawEvents => ViewMode::Chat,
                };
            }
            Action::ShowModelSelector => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    self.state.model_selector_state.show(session.model.clone());
                    self.state.input_mode = InputMode::SelectingModel;
                }
            }
            Action::OpenSessionImport => {
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
            }
            Action::DumpDebugState => {
                effects.push(Effect::DumpDebugState);
            }

            // ========== Tab Management ==========
            Action::CloseTab => {
                let active = self.state.tab_manager.active_index();
                self.state.tab_manager.close_tab(active);
                if self.state.tab_manager.is_empty() {
                    self.state.sidebar_state.visible = true;
                    self.state.input_mode = InputMode::SidebarNavigation;
                }
            }
            Action::NextTab => {
                // Include sidebar in tab cycle when visible
                if self.state.input_mode == InputMode::SidebarNavigation {
                    // From sidebar, go to first tab
                    if !self.state.tab_manager.is_empty() {
                        self.state.tab_manager.switch_to(0);
                        self.state.sidebar_state.set_focused(false);
                        self.state.input_mode = InputMode::Normal;
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
                    }
                } else {
                    self.state.tab_manager.next_tab();
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
                    }
                } else {
                    self.state.tab_manager.prev_tab();
                }
            }
            Action::SwitchToTab(n) => {
                if n > 0 {
                    self.state.tab_manager.switch_to((n - 1) as usize);
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
                    self.record_chat_scroll(n as usize);
                }
            }
            Action::ScrollDown(n) => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_down(n as usize);
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_down(n as usize);
                    }
                    self.record_chat_scroll(n as usize);
                }
            }
            Action::ScrollPageUp => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.page_up();
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_up(10);
                    }
                    self.record_chat_scroll(10);
                }
            }
            Action::ScrollPageDown => {
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.page_down();
                } else {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.chat_view.scroll_down(10);
                    }
                    self.record_chat_scroll(10);
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

            // ========== Input Box Editing ==========
            Action::InsertNewline => {
                // Don't insert newlines in help dialog or command mode
                if self.state.input_mode != InputMode::ShowingHelp
                    && self.state.input_mode != InputMode::Command
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
                    _ => {
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            session.input_box.backspace();
                        }
                    }
                }
            }
            Action::Delete => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
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
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_left();
                }
            }
            Action::MoveCursorRight => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_right();
                }
            }
            Action::MoveCursorStart => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_start();
                }
            }
            Action::MoveCursorEnd => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
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
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.move_up() {
                        if session.input_box.is_cursor_on_first_line() {
                            session.input_box.history_prev();
                        }
                    }
                }
            }
            Action::MoveCursorDown => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.move_down() {
                        if session.input_box.is_cursor_on_last_line() {
                            session.input_box.history_next();
                        }
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
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.is_empty() {
                        let submission = session.input_box.submit();
                        if !submission.text.trim().is_empty() || !submission.image_paths.is_empty()
                        {
                            effects.extend(self.submit_prompt(
                                submission.text,
                                submission.image_paths,
                                submission.image_placeholders,
                            )?);
                        }
                    }
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
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.select_next();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.select_next();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.select_next();
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
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.select_previous();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.select_prev();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.select_prev();
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
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            let agent_changed = session.agent_type != agent_type;
                            session.model = Some(model_id.clone());
                            session.agent_type = agent_type;
                            session.update_status();
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
                                    return Ok(effects);
                                }
                                ConfirmationContext::CreatePullRequest { preflight, .. } => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::Normal;
                                    effects.extend(self.submit_pr_workflow(preflight)?);
                                    return Ok(effects);
                                }
                                ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::Normal;
                                    effects.push(Effect::OpenPrInBrowser { working_dir });
                                    return Ok(effects);
                                }
                            }
                        }
                    }
                    self.state.confirmation_dialog_state.hide();
                    self.state.input_mode = InputMode::SidebarNavigation;
                }
                InputMode::ShowingError => {
                    self.state.error_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                _ => {}
            },
            Action::Cancel => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.hide();
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
                    self.state.confirmation_dialog_state.hide();
                    if matches!(
                        self.state.confirmation_dialog_state.context,
                        Some(ConfirmationContext::CreatePullRequest { .. })
                            | Some(ConfirmationContext::OpenExistingPr { .. })
                    ) {
                        self.state.input_mode = InputMode::Normal;
                    } else {
                        self.state.input_mode = InputMode::SidebarNavigation;
                    }
                }
                InputMode::ShowingError => {
                    self.state.error_dialog_state.hide();
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
                    self.state.add_repo_dialog_state.show();
                    self.state.input_mode = InputMode::AddingRepository;
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.hide();
                    self.state.add_repo_dialog_state.show();
                    self.state.input_mode = InputMode::AddingRepository;
                }
                _ => {}
            },
            Action::OpenSettings => {
                if self.state.input_mode == InputMode::SidebarNavigation {
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
                            }
                            ConfirmationContext::CreatePullRequest { preflight, .. } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.submit_pr_workflow(preflight)?);
                            }
                            ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.push(Effect::OpenPrInBrowser { working_dir });
                            }
                        }
                    }
                }
            }
            Action::ConfirmNo => {
                if self.state.input_mode == InputMode::Confirming {
                    self.state.confirmation_dialog_state.hide();
                    self.state.input_mode = InputMode::SidebarNavigation;
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
                self.state.help_dialog_state.show(&self.config.keybindings);
                self.state.input_mode = InputMode::ShowingHelp;
            }
            Action::ExecuteCommand => {
                if self.state.input_mode == InputMode::Command {
                    if let Some(action) = self.execute_command() {
                        // Prevent recursion - ExecuteCommand can't call itself
                        if !matches!(action, Action::ExecuteCommand) {
                            effects.extend(Box::pin(self.execute_action(action)).await?);
                        }
                    }
                }
            }
            Action::CompleteCommand => {
                if self.state.input_mode == InputMode::Command {
                    self.complete_command();
                }
            }
        }

        Ok(effects)
    }

    async fn run_effects(&mut self, effects: Vec<Effect>) -> anyhow::Result<()> {
        for effect in effects {
            match effect {
                Effect::SaveSessionState => {
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
                    tab_index,
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
                                while let Some(event) = handle.events.recv().await {
                                    if event_tx.send(AppEvent::Agent { tab_index, event }).is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ =
                                    event_tx.send(AppEvent::Error(format!("Agent error: {}", e)));
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
                        let _ = event_tx.send(AppEvent::PrPreflightCompleted {
                            tab_index,
                            working_dir,
                            result,
                        });
                    });
                }
                Effect::OpenPrInBrowser { working_dir } => {
                    let event_tx = self.event_tx.clone();
                    tokio::task::spawn_blocking(move || {
                        let result =
                            PrManager::open_pr_in_browser(&working_dir).map_err(|e| e.to_string());
                        let _ = event_tx.send(AppEvent::OpenPrCompleted { result });
                    });
                }
                Effect::DumpDebugState => {
                    let result = self.dump_debug_state();
                    let _ = self.event_tx.send(AppEvent::DebugDumped { result });
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

                            let existing_names: Vec<String> = workspace_dao
                                .get_by_repository(repo_id)
                                .unwrap_or_default()
                                .iter()
                                .map(|w| w.name.clone())
                                .collect();

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
                                let _ = worktree_manager
                                    .remove_worktree(&base_path, &workspace.path)
                                    .map_err(|cleanup_err| {
                                        tracing::error!(
                                            error = %cleanup_err,
                                            "Failed to clean up worktree after DB error"
                                        );
                                    });
                                return Err(format!("Failed to save workspace to database: {}", e));
                            }

                            Ok(WorkspaceCreated {
                                repo_id,
                                workspace_id,
                            })
                        })();

                        let _ = event_tx.send(AppEvent::WorkspaceCreated { result });
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

                            let mut worktree_error = None;
                            if let Some(base_path) = repo_base_path {
                                if let Err(e) =
                                    worktree_manager.remove_worktree(&base_path, &workspace.path)
                                {
                                    worktree_error =
                                        Some(format!("Failed to remove worktree: {}", e));
                                }
                            }

                            workspace_dao.archive(workspace_id).map_err(|e| {
                                format!("Failed to archive workspace in database: {}", e)
                            })?;

                            Ok(WorkspaceArchived {
                                workspace_id,
                                worktree_error,
                            })
                        })(
                        );

                        let _ = event_tx.send(AppEvent::WorkspaceArchived { result });
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
                            let _ = event_tx.send(AppEvent::ProjectRemoved {
                                result: RemoveProjectResult {
                                    repo_id,
                                    workspace_ids,
                                    errors,
                                },
                            });
                            return;
                        };
                        let Some(workspace_dao) = workspace_dao else {
                            errors.push("No workspace DAO available".to_string());
                            let _ = event_tx.send(AppEvent::ProjectRemoved {
                                result: RemoveProjectResult {
                                    repo_id,
                                    workspace_ids,
                                    errors,
                                },
                            });
                            return;
                        };

                        let (repo_base_path, repo_name) = match repo_dao.get_by_id(repo_id) {
                            Ok(Some(repo)) => (repo.base_path, repo.name),
                            Ok(None) => {
                                errors.push("Repository not found".to_string());
                                let _ = event_tx.send(AppEvent::ProjectRemoved {
                                    result: RemoveProjectResult {
                                        repo_id,
                                        workspace_ids,
                                        errors,
                                    },
                                });
                                return;
                            }
                            Err(e) => {
                                errors.push(format!("Failed to load repository: {}", e));
                                let _ = event_tx.send(AppEvent::ProjectRemoved {
                                    result: RemoveProjectResult {
                                        repo_id,
                                        workspace_ids,
                                        errors,
                                    },
                                });
                                return;
                            }
                        };

                        let workspaces =
                            workspace_dao.get_by_repository(repo_id).unwrap_or_default();
                        for ws in workspaces {
                            workspace_ids.push(ws.id);
                            if let Some(ref base_path) = repo_base_path {
                                if let Err(e) =
                                    worktree_manager.remove_worktree(base_path, &ws.path)
                                {
                                    errors.push(format!(
                                        "Failed to remove worktree '{}': {}",
                                        ws.name, e
                                    ));
                                }
                            }
                            if let Err(e) = workspace_dao.archive(ws.id) {
                                errors.push(format!(
                                    "Failed to archive workspace '{}': {}",
                                    ws.name, e
                                ));
                            }
                        }

                        let worktrees_dir = crate::util::worktrees_dir();
                        let project_worktrees_path = worktrees_dir.join(&repo_name);
                        if project_worktrees_path.exists() {
                            if let Err(e) = std::fs::remove_dir_all(&project_worktrees_path) {
                                errors.push(format!("Failed to remove project folder: {}", e));
                            }
                        }

                        if let Err(e) = repo_dao.delete(repo_id) {
                            errors
                                .push(format!("Failed to delete repository from database: {}", e));
                        }

                        let _ = event_tx.send(AppEvent::ProjectRemoved {
                            result: RemoveProjectResult {
                                repo_id,
                                workspace_ids,
                                errors,
                            },
                        });
                    });
                }
                Effect::CopyToClipboard(text) => {
                    use arboard::Clipboard;
                    if let Ok(mut clipboard) = Clipboard::new() {
                        let _ = clipboard.set_text(text);
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
                            let _ = event_tx.send(event);
                        });
                    });
                }
                Effect::ImportSession(session) => {
                    // Create a new tab with the session's agent type and working directory
                    let agent_type = session.agent_type.clone();
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
            _ => {}
        }
    }

    fn handle_paste_input(&mut self, pasted: String) {
        let pasted = pasted.replace('\r', "\n");
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
            _ => {}
        }
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
        let _ = workspace_dao.update_last_accessed(workspace_id);

        // Create a new tab with the workspace's working directory
        self.state
            .tab_manager
            .new_tab_with_working_dir(AgentType::Claude, workspace.path.clone());

        // Store workspace info in session and restore chat history if available
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            session.workspace_id = Some(workspace_id);
            session.project_name = project_name;
            session.workspace_name = Some(workspace.name.clone());

            // Restore saved session data if available
            if let Some(saved) = saved_tab {
                session.model = saved.model;

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
            }
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
        self.state.confirmation_dialog_state.show(
            format!("Archive \"{}\"?", workspace.name),
            "This will remove the worktree but keep the branch.",
            warnings,
            confirmation_type,
            "Archive",
            Some(ConfirmationContext::ArchiveWorkspace(workspace_id)),
        );
        self.state.input_mode = InputMode::Confirming;
    }

    /// Show an error dialog with a simple message
    fn show_error(&mut self, title: &str, message: &str) {
        self.state.error_dialog_state.show(title, message);
        self.state.input_mode = InputMode::ShowingError;
    }

    /// Show an error dialog with technical details
    fn show_error_with_details(&mut self, title: &str, message: &str, details: &str) {
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
        let mut session = AgentSession::with_working_dir(agent_type.clone(), working_dir);
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

        Ok(())
    }

    fn record_chat_scroll(&mut self, lines: usize) {
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
                        session.raw_events_view.scroll_down(*pending_down, list_height);
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
                } else if self.state.view_mode == ViewMode::RawEvents {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if session.raw_events_view.is_detail_visible() {
                            session.raw_events_view.event_detail.scroll_up(3);
                        } else {
                            session.raw_events_view.scroll_up(3);
                        }
                    }
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_up(1);
                    self.record_chat_scroll(1);
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
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_down(1);
                    self.record_chat_scroll(1);
                }
                Ok(Vec::new())
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.handle_scrollbar_press(x, y) {
                    return Ok(Vec::new());
                }
                // Handle left clicks based on position
                self.handle_mouse_click(x, y).await
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.handle_scrollbar_drag(y) {
                    return Ok(Vec::new());
                }
                Ok(Vec::new())
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.state.scroll_drag = None;
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
                session
                    .chat_view
                    .scrollbar_metrics(area, session.is_processing)
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
    async fn handle_mouse_click(&mut self, x: u16, y: u16) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();

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
                self.handle_status_bar_click(x, y, status_bar_area);
                return Ok(effects);
            }
        }

        // Check footer
        if let Some(footer_area) = self.state.footer_area {
            if Self::point_in_rect(x, y, footer_area) {
                if let Some(action) = self.handle_footer_click(x, y, footer_area) {
                    effects.extend(self.execute_action(action).await?);
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
                                        effects.push(Effect::CopyToClipboard(
                                            session_id.to_string(),
                                        ));
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

        // Click in chat area - could be used for text selection in future
        // For now, clicking in chat area while in sidebar mode returns to normal
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
    fn handle_sidebar_click(&mut self, _x: u16, y: u16, sidebar_area: Rect) -> Option<Effect> {
        // Account for border (1 row at top for title)
        let inner_y = sidebar_area.y + 1;
        if y < inner_y {
            return None; // Clicked on title bar
        }

        // Always focus sidebar when clicking on it
        self.state.sidebar_state.set_focused(true);
        self.state.input_mode = InputMode::SidebarNavigation;

        let clicked_row = (y - inner_y) as usize;
        let clicked_index = clicked_row + self.state.sidebar_state.tree_state.offset;

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

    /// Handle click in tab bar area
    fn handle_tab_bar_click(&mut self, x: u16, _y: u16, tab_bar_area: Rect) {
        let relative_x = x.saturating_sub(tab_bar_area.x) as usize;

        // Calculate tab positions
        let mut current_x: usize = 0;
        let tab_names = self.state.tab_manager.tab_names();
        let active_index = self.state.tab_manager.active_index();

        for (i, name) in tab_names.iter().enumerate() {
            // Format: " ▶ [N] Name " for active, "  [N] Name " for inactive
            let tab_width = if i == active_index {
                4 + 3 + name.len() + 1 // " ▶ " + "[N]" + " Name" + " "
            } else {
                2 + 3 + name.len() + 1 // "  " + "[N]" + " Name" + " "
            };

            if relative_x < current_x + tab_width {
                // Clicked on this tab
                self.state.tab_manager.switch_to(i);
                return;
            }
            current_x += tab_width;
        }

        // Check for "+ New" button
        if self.state.tab_manager.can_add_tab() {
            // "+ New" button width is about 7 characters: "  [+]  "
            let new_button_width = 7;
            if relative_x >= current_x && relative_x < current_x + new_button_width {
                // Show agent selector for new tab
                self.state.agent_selector_state.show();
                self.state.input_mode = InputMode::SelectingAgent;
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
    fn handle_status_bar_click(&mut self, x: u16, _y: u16, status_bar_area: Rect) {
        // Status bar format: "agent │ model │ status"
        // Click on agent or model portion to open model selector
        let relative_x = x.saturating_sub(status_bar_area.x) as usize;

        // Approximate positions - agent type is ~6 chars, separator is 3, model starts around 9
        // Click on either agent tag (0-8) or model area (9-25) opens the model selector
        if relative_x < 25 {
            if let Some(session) = self.state.tab_manager.active_session() {
                self.state.model_selector_state.show(session.model.clone());
                self.state.input_mode = InputMode::SelectingModel;
            }
        }
    }

    /// Handle click in model selector dialog
    fn handle_model_selector_click(&mut self, x: u16, y: u16) -> Option<Effect> {
        use crate::ui::components::ModelSelectorItem;

        // Calculate dialog dimensions (must match model_selector.rs render logic)
        let terminal_size = crossterm::terminal::size().unwrap_or((80, 24));
        let screen_width = terminal_size.0;
        let screen_height = terminal_size.1;

        let content_height = self.state.model_selector_state.items.len() as u16 + 4;
        let dialog_height = content_height.min(screen_height.saturating_sub(4));
        let dialog_width: u16 = 40;

        // Calculate dialog position (aligned with status bar)
        let (dialog_x, dialog_y) = if let Some(sb_area) = self.state.status_bar_area {
            let dx = sb_area.x;
            let dy = sb_area.y.saturating_sub(dialog_height);
            (dx, dy)
        } else {
            let dx = (screen_width.saturating_sub(dialog_width)) / 2;
            let dy = (screen_height.saturating_sub(dialog_height)) / 2;
            (dx, dy)
        };

        // Ensure dialog stays within screen bounds
        let dialog_x = dialog_x.min(screen_width.saturating_sub(dialog_width));

        // Check if click is outside dialog - close it
        if x < dialog_x
            || x >= dialog_x + dialog_width
            || y < dialog_y
            || y >= dialog_y + dialog_height
        {
            self.state.model_selector_state.hide();
            self.state.input_mode = InputMode::Normal;
            return None;
        }

        // Inner area (accounting for border and padding)
        let inner_x = dialog_x + 2; // border + padding
        let inner_y = dialog_y + 1; // border
        let inner_height = dialog_height.saturating_sub(2);

        // Check if click is in the content area
        if x < inner_x || y < inner_y || y >= inner_y + inner_height {
            return None;
        }

        // Map y position to item index
        // The items render with section headers and spacing
        let relative_y = (y - inner_y) as usize;

        // Walk through items to find which one was clicked
        let mut current_row: usize = 0;
        let mut clicked_selectable_idx: Option<usize> = None;

        for (item_idx, item) in self.state.model_selector_state.items.iter().enumerate() {
            match item {
                ModelSelectorItem::SectionHeader(_) => {
                    // Section headers have spacing before them (except first)
                    if item_idx > 0 {
                        current_row += 1; // spacing line
                    }
                    if current_row == relative_y {
                        // Clicked on section header - do nothing
                        return None;
                    }
                    current_row += 1; // header line
                }
                ModelSelectorItem::Model(_) => {
                    if current_row == relative_y {
                        // Find which selectable index this corresponds to
                        for (sel_idx, &sel_item_idx) in self
                            .state
                            .model_selector_state
                            .selectable_indices
                            .iter()
                            .enumerate()
                        {
                            if sel_item_idx == item_idx {
                                clicked_selectable_idx = Some(sel_idx);
                                break;
                            }
                        }
                        break;
                    }
                    current_row += 1;
                }
            }

            if current_row > relative_y {
                break;
            }
        }

        // If a model was clicked, select it and apply
        if let Some(sel_idx) = clicked_selectable_idx {
            self.state.model_selector_state.selected = sel_idx;

            // Apply the selection (same logic as Enter key)
            if let Some(model) = self.state.model_selector_state.selected_model().cloned() {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let agent_changed = session.agent_type != model.agent_type;
                    session.model = Some(model.id.clone());
                    session.agent_type = model.agent_type;
                    session.update_status();

                    // Add system message about the change
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
            ViewMode::Chat => vec![
                ("Tab", "Switch"),
                ("C-t", "Sidebar"),
                ("C-n", "Project"),
                ("M-S-w", "Close"),
                ("C-c", "Stop"),
                ("C-q", "Quit"),
            ],
            ViewMode::RawEvents => vec![
                ("j/k", "Nav"),
                ("l/CR", "Expand"),
                ("h/Esc", "Collapse"),
                ("C-g", "Chat"),
                ("C-q", "Quit"),
            ],
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
            AppEvent::Agent { tab_index, event } => {
                self.handle_agent_event(tab_index, event).await?;
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
                }
            }
            AppEvent::PrPreflightCompleted {
                tab_index,
                working_dir,
                result,
            } => {
                effects.extend(self.handle_pr_preflight_result(tab_index, working_dir, result));
            }
            AppEvent::OpenPrCompleted { result } => {
                if let Err(err) = result {
                    self.state.error_dialog_state.show(
                        "Failed to Open PR",
                        &format!("Could not open PR in browser: {}", err),
                    );
                    self.state.input_mode = InputMode::ShowingError;
                }
            }
            AppEvent::DebugDumped { result } => match result {
                Ok(path) => {
                    self.state.error_dialog_state.show_with_details(
                        "Debug Export Complete",
                        "Session debug info has been exported.",
                        &format!("File saved to:\n{}", path),
                    );
                    self.state.input_mode = InputMode::ShowingError;
                }
                Err(err) => {
                    self.state.error_dialog_state.show("Export Failed", &err);
                    self.state.input_mode = InputMode::ShowingError;
                }
            },
            AppEvent::WorkspaceCreated { result } => match result {
                Ok(created) => {
                    self.refresh_sidebar_data();
                    self.state.sidebar_data.expand_repo(created.repo_id);
                    if let Some(index) = self.find_workspace_index(created.workspace_id) {
                        self.state.sidebar_state.tree_state.selected = index;
                    }
                    self.open_workspace_with_options(created.workspace_id, false);
                }
                Err(err) => {
                    self.show_error("Workspace Creation Failed", &err);
                }
            },
            AppEvent::WorkspaceArchived { result } => match result {
                Ok(archived) => {
                    if let Some(error_msg) = archived.worktree_error {
                        self.show_error_with_details(
                            "Worktree Warning",
                            "Workspace archived but worktree removal failed",
                            &error_msg,
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
            AppEvent::AgentStreamEnded { tab_index } => {
                // Agent event stream ended (process exited) - ensure processing is stopped
                if let Some(session) = self.state.tab_manager.session_mut(tab_index) {
                    if session.is_processing {
                        session.stop_processing();
                        session.chat_view.finalize_streaming();
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
            _ => {}
        }

        Ok(effects)
    }

    async fn handle_agent_event(
        &mut self,
        tab_index: usize,
        event: AgentEvent,
    ) -> anyhow::Result<()> {
        // Check if this is a non-active tab receiving content - mark as needing attention
        let is_active_tab = self.state.tab_manager.active_index() == tab_index;
        let is_content_event = matches!(
            &event,
            AgentEvent::AssistantMessage(_)
                | AgentEvent::ToolCompleted(_)
                | AgentEvent::TurnCompleted(_)
                | AgentEvent::TurnFailed(_)
        );

        let Some(session) = self.state.tab_manager.session_mut(tab_index) else {
            return Ok(());
        };

        // Mark non-active tabs as needing attention when content arrives
        if !is_active_tab && is_content_event {
            session.needs_attention = true;
        }

        // Record raw event for debug view
        let event_type = event.event_type_name();
        let raw_json = serde_json::to_value(&event).unwrap_or_default();
        session.record_raw_event(EventDirection::Received, event_type, raw_json);

        match event {
            AgentEvent::SessionInit(init) => {
                session.agent_session_id = Some(init.session_id);
                session.update_status();
            }
            AgentEvent::TurnStarted => {
                session.is_processing = true;
                session.update_status();
            }
            AgentEvent::TurnCompleted(completed) => {
                session.add_usage(completed.usage);
                session.stop_processing();
                session.chat_view.finalize_streaming();
                // Add turn summary to chat
                let summary = session.current_turn_summary.clone();
                session.chat_view.push(ChatMessage::turn_summary(summary));
            }
            AgentEvent::TurnFailed(failed) => {
                session.stop_processing();
                let display = MessageDisplay::Error {
                    content: failed.error,
                };
                session.chat_view.push(display.to_chat_message());
            }
            AgentEvent::AssistantMessage(msg) => {
                // Track streaming tokens (rough estimate: ~4 chars per token)
                let token_estimate = (msg.text.len() / 4).max(1);
                session.add_streaming_tokens(token_estimate);

                // Check for PR URL in the message and capture PR number
                if session.pr_number.is_none() {
                    if let Some(pr_num) = Self::extract_pr_number_from_text(&msg.text) {
                        session.pr_number = Some(pr_num);
                    }
                }

                if msg.is_final {
                    let display = MessageDisplay::Assistant {
                        content: msg.text,
                        is_streaming: false,
                    };
                    session.chat_view.push(display.to_chat_message());
                } else {
                    session.chat_view.stream_append(&msg.text);
                }
            }
            AgentEvent::ToolStarted(tool) => {
                // Update processing state to show tool name
                session.set_processing_state(ProcessingState::ToolUse(tool.tool_name.clone()));

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
                };
                session.chat_view.push(display.to_chat_message());
            }
            AgentEvent::ToolCompleted(tool) => {
                // Return to thinking state
                session.set_processing_state(ProcessingState::Thinking);

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
                let display = MessageDisplay::Tool {
                    name: MessageDisplay::tool_display_name_owned(&tool.tool_id),
                    args: String::new(),
                    output,
                    exit_code: None,
                };
                session.chat_view.push(display.to_chat_message());
            }
            AgentEvent::CommandOutput(cmd) => {
                // Check for PR URL in command output (e.g., from gh pr create)
                if session.pr_number.is_none() {
                    if let Some(pr_num) = Self::extract_pr_number_from_text(&cmd.output) {
                        session.pr_number = Some(pr_num);
                    }
                }

                let display = MessageDisplay::Tool {
                    name: "Bash".to_string(),
                    args: cmd.command.clone(),
                    output: cmd.output.clone(),
                    exit_code: cmd.exit_code,
                };
                session.chat_view.push(display.to_chat_message());
            }
            AgentEvent::Error(err) => {
                let display = MessageDisplay::Error {
                    content: err.message,
                };
                session.chat_view.push(display.to_chat_message());
                if err.is_fatal {
                    session.stop_processing();
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn submit_prompt(
        &mut self,
        prompt: String,
        mut images: Vec<PathBuf>,
        image_placeholders: Vec<String>,
    ) -> anyhow::Result<Vec<Effect>> {
        let mut effects = Vec::new();
        let tab_index = self.state.tab_manager.active_index();
        let Some(session) = self.state.tab_manager.active_session_mut() else {
            return Ok(effects);
        };

        let mut prompt = prompt;

        let agent_type = session.agent_type;
        let model = session.model.clone();
        // Use agent_session_id if available (set by agent after first prompt)
        // Fall back to resume_session_id only for initial session restoration
        let session_id_to_use = session
            .agent_session_id
            .clone()
            .or_else(|| session.resume_session_id.take());
        // Use session's working_dir if set, otherwise fall back to config
        let working_dir = session
            .working_dir
            .clone()
            .unwrap_or_else(|| self.config.working_dir.clone());

        // Add user message to chat
        let display = MessageDisplay::User {
            content: prompt.clone(),
        };
        session.chat_view.push(display.to_chat_message());
        session.start_processing();

        // Validate working directory exists
        if !working_dir.exists() {
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                let display = MessageDisplay::Error {
                    content: format!(
                        "Working directory does not exist: {}",
                        working_dir.display()
                    ),
                };
                session.chat_view.push(display.to_chat_message());
                session.stop_processing();
            }
            return Ok(effects);
        }

        // Start agent
        if agent_type == AgentType::Codex && !images.is_empty() {
            prompt = Self::strip_image_placeholders(prompt, &image_placeholders);
        }
        if agent_type == AgentType::Claude && !images.is_empty() {
            prompt = Self::append_image_paths_to_prompt(prompt, &images);
            images.clear();
        }

        if prompt.trim().is_empty() && images.is_empty() {
            session.stop_processing();
            return Ok(effects);
        }

        // Record user input for debug view (post-processing)
        let mut debug_payload =
            serde_json::json!({ "prompt": &prompt, "agent_type": agent_type.as_str() });
        if !images.is_empty() {
            let image_paths: Vec<String> = images
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            debug_payload["images"] = serde_json::json!(image_paths);
        }
        session.record_raw_event(EventDirection::Sent, "UserPrompt", debug_payload);

        let mut config = AgentStartConfig::new(prompt, working_dir)
            .with_tools(self.config.claude_allowed_tools.clone())
            .with_images(images);

        // Add model if specified
        if let Some(model_id) = model {
            config = config.with_model(model_id);
        }

        // Add session ID to continue existing conversation
        if let Some(session_id) = session_id_to_use {
            config = config.with_resume(session_id);
        }

        effects.push(Effect::StartAgent {
            tab_index,
            agent_type,
            config,
        });

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

    /// Handle Ctrl+P: Open existing PR or create new one
    fn handle_pr_action(&mut self) -> Option<Effect> {
        let tab_index = self.state.tab_manager.active_index();
        let session = match self.state.tab_manager.active_session() {
            Some(s) => s,
            None => return None, // No active tab
        };

        let working_dir = match &session.working_dir {
            Some(d) => d.clone(),
            None => return None, // No working dir
        };

        // Show loading dialog immediately
        self.state
            .confirmation_dialog_state
            .show_loading("Create Pull Request", "Checking repository status...");
        self.state.input_mode = InputMode::Confirming;

        Some(Effect::PrPreflight {
            tab_index,
            working_dir,
        })
    }

    /// Handle the result of the PR preflight check
    fn handle_pr_preflight_result(
        &mut self,
        _tab_index: usize,
        working_dir: std::path::PathBuf,
        preflight: crate::git::PrPreflightResult,
    ) -> Vec<Effect> {
        let effects = Vec::new();
        // Handle blocking errors
        if !preflight.gh_installed {
            self.state.confirmation_dialog_state.hide();
            self.state.error_dialog_state.show_with_details(
                "GitHub CLI Not Found",
                "The 'gh' command is not installed.",
                "Install from: https://cli.github.com/\n\nbrew install gh  # macOS\napt install gh   # Debian/Ubuntu",
            );
            self.state.input_mode = InputMode::ShowingError;
            return effects;
        }

        if !preflight.gh_authenticated {
            self.state.confirmation_dialog_state.hide();
            self.state.error_dialog_state.show_with_details(
                "Not Authenticated",
                "GitHub CLI is not authenticated.",
                "Run: gh auth login",
            );
            self.state.input_mode = InputMode::ShowingError;
            return effects;
        }

        if preflight.on_main_branch {
            self.state.confirmation_dialog_state.hide();
            self.state.error_dialog_state.show(
                "Cannot Create PR",
                &format!(
                    "You're on the '{}' branch. Create a feature branch first.",
                    preflight.branch_name
                ),
            );
            self.state.input_mode = InputMode::ShowingError;
            return effects;
        }

        // If PR exists, show confirmation dialog to open in browser
        if let Some(ref pr) = preflight.existing_pr {
            if pr.exists {
                // Update session's pr_number
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.pr_number = pr.number;
                }

                let pr_url = pr.url.clone().unwrap_or_else(|| "Unknown URL".to_string());
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
                // Already in Confirming mode
                return effects;
            }
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
                tab_index: _tab_index,
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
        preflight: crate::git::PrPreflightResult,
    ) -> anyhow::Result<Vec<Effect>> {
        // Generate prompt for PR creation
        let prompt = PrManager::generate_pr_prompt(&preflight);

        // Submit to current chat session
        self.submit_prompt(prompt, Vec::new(), Vec::new())
    }

    fn draw(&mut self, f: &mut Frame) {
        let size = f.area();

        // Show splash screen only for first-time users (no repos)
        if self.state.show_first_time_splash {
            self.state.splash_screen.first_time_mode = true;
            self.state.splash_screen.render(size, f.buffer_mut());

            // Draw dialogs over splash screen
            if self.state.base_dir_dialog_state.is_visible() {
                let dialog = BaseDirDialog::new();
                dialog.render(size, f.buffer_mut(), &self.state.base_dir_dialog_state);
            } else if self.state.project_picker_state.is_visible() {
                let picker = ProjectPicker::new();
                picker.render(size, f.buffer_mut(), &self.state.project_picker_state);
            } else if self.state.add_repo_dialog_state.is_visible() {
                let dialog = AddRepoDialog::new();
                dialog.render(size, f.buffer_mut(), &self.state.add_repo_dialog_state);
            }

            // Draw agent selector dialog if needed
            if self.state.agent_selector_state.is_visible() {
                let selector = AgentSelector::new();
                selector.render(size, f.buffer_mut(), &self.state.agent_selector_state);
            }
            return;
        }

        // Calculate sidebar width
        let sidebar_width = if self.state.sidebar_state.visible {
            30u16
        } else {
            0
        };

        // Split horizontally for sidebar
        let (sidebar_area, content_area) = if sidebar_width > 0 {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(sidebar_width), Constraint::Min(20)])
                .split(size);
            (chunks[0], chunks[1])
        } else {
            // No sidebar - use full width
            (Rect::default(), size)
        };

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
                // Calculate dynamic input height (max 30% of screen)
                let max_input_height = (content_area.height as f32 * 0.30).ceil() as u16;
                let input_height = if let Some(session) = self.state.tab_manager.active_session() {
                    session
                        .input_box
                        .desired_height(max_input_height, content_area.width)
                } else {
                    3 // Minimum height
                };

                // Chat layout with input box
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),            // Tab bar
                        Constraint::Min(5),               // Chat view
                        Constraint::Length(input_height), // Input box (dynamic)
                        Constraint::Length(1),            // Status bar
                        Constraint::Length(1),            // Footer
                    ])
                    .split(content_area);

                // Store layout areas for mouse hit-testing
                self.state.tab_bar_area = Some(chunks[0]);
                self.state.chat_area = Some(chunks[1]);
                self.state.raw_events_area = None;
                self.state.input_area = Some(chunks[2]);
                self.state.status_bar_area = Some(chunks[3]);
                self.state.footer_area = Some(chunks[4]);

                // Draw tab bar (unfocused when sidebar is focused)
                let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;

                // Collect tab states for PR indicators
                let pr_numbers: Vec<Option<u32>> = self
                    .state
                    .tab_manager
                    .sessions()
                    .iter()
                    .map(|s| s.pr_number)
                    .collect();
                let processing_flags: Vec<bool> = self
                    .state
                    .tab_manager
                    .sessions()
                    .iter()
                    .map(|s| s.is_processing)
                    .collect();
                let attention_flags: Vec<bool> = self
                    .state
                    .tab_manager
                    .sessions()
                    .iter()
                    .map(|s| s.needs_attention)
                    .collect();

                let tab_bar = TabBar::new(
                    self.state.tab_manager.tab_names(),
                    self.state.tab_manager.active_index(),
                    self.state.tab_manager.can_add_tab(),
                )
                .focused(tabs_focused)
                .with_tab_states(pr_numbers, processing_flags, attention_flags)
                .with_spinner_frame(self.state.spinner_frame);
                tab_bar.render(chunks[0], f.buffer_mut());

                // Draw active session components
                let is_command_mode = self.state.input_mode == InputMode::Command;
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    // Render chat with thinking indicator if processing
                    let thinking_line = if session.is_processing {
                        Some(session.thinking_indicator.render())
                    } else {
                        None
                    };
                    session.chat_view.render_with_indicator(
                        chunks[1],
                        f.buffer_mut(),
                        thinking_line,
                        session.pr_number,
                    );

                    // Render input box (not in command mode)
                    if !is_command_mode {
                        session.input_box.render(chunks[2], f.buffer_mut());
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
                    session.status_bar.render(chunks[3], f.buffer_mut());

                    // Set cursor position (accounting for scroll)
                    if self.state.input_mode == InputMode::Normal {
                        let scroll_offset = session.input_box.scroll_offset();
                        let (cx, cy) = session.input_box.cursor_position(chunks[2], scroll_offset);
                        f.set_cursor_position((cx, cy));
                    }
                }

                // Render command prompt if in command mode (outside session borrow)
                if is_command_mode {
                    self.render_command_prompt(chunks[2], f.buffer_mut());
                    // Cursor at end of command buffer (after ":" in padded area)
                    let prompt = format!(":{}", self.state.command_buffer);
                    let prompt_width = prompt.width() as u16;
                    let max_x = chunks[2].x + chunks[2].width.saturating_sub(1);
                    let cx = (chunks[2].x + prompt_width).min(max_x);
                    let cy = chunks[2].y + 1; // top padding
                    f.set_cursor_position((cx, cy));
                }

                // Draw footer
                let footer = GlobalFooter::new().with_view_mode(self.state.view_mode);
                footer.render(chunks[4], f.buffer_mut());
            }
            ViewMode::RawEvents => {
                // Raw events layout - no input box, full height for events
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1), // Tab bar
                        Constraint::Min(5),    // Raw events view (full height)
                        Constraint::Length(1), // Footer
                    ])
                    .split(content_area);

                // Store layout areas for mouse hit-testing (no input/status in this mode)
                self.state.tab_bar_area = Some(chunks[0]);
                self.state.chat_area = None;
                self.state.raw_events_area = Some(chunks[1]);
                self.state.input_area = None;
                self.state.status_bar_area = None;
                self.state.footer_area = Some(chunks[2]);

                // Draw tab bar (unfocused when sidebar is focused)
                let tabs_focused = self.state.input_mode != InputMode::SidebarNavigation;
                // Collect tab states for PR indicators
                let pr_numbers: Vec<Option<u32>> = self
                    .state
                    .tab_manager
                    .sessions()
                    .iter()
                    .map(|s| s.pr_number)
                    .collect();
                let processing_flags: Vec<bool> = self
                    .state
                    .tab_manager
                    .sessions()
                    .iter()
                    .map(|s| s.is_processing)
                    .collect();
                let attention_flags: Vec<bool> = self
                    .state
                    .tab_manager
                    .sessions()
                    .iter()
                    .map(|s| s.needs_attention)
                    .collect();
                let tab_bar = TabBar::new(
                    self.state.tab_manager.tab_names(),
                    self.state.tab_manager.active_index(),
                    self.state.tab_manager.can_add_tab(),
                )
                .focused(tabs_focused)
                .with_tab_states(pr_numbers, processing_flags, attention_flags)
                .with_spinner_frame(self.state.spinner_frame);
                tab_bar.render(chunks[0], f.buffer_mut());

                // Draw raw events view
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.render(chunks[1], f.buffer_mut());
                }

                // Draw footer
                let footer = GlobalFooter::new().with_view_mode(self.state.view_mode);
                footer.render(chunks[2], f.buffer_mut());
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
            let model_selector = ModelSelector::new();
            model_selector.render(
                size,
                f.buffer_mut(),
                &self.state.model_selector_state,
                self.state.status_bar_area,
            );
        }

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

        // Draw help dialog (on top of everything)
        if self.state.help_dialog_state.is_visible() {
            HelpDialog::new().render(size, f.buffer_mut(), &mut self.state.help_dialog_state);
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

    /// Render command mode prompt
    fn render_command_prompt(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        use ratatui::style::{Color, Style};
        use ratatui::widgets::{Clear, Paragraph, Widget};
        use unicode_width::UnicodeWidthStr;

        Clear.render(area, buf);
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_bg(crate::ui::components::INPUT_BG);
            }
        }

        if area.height < 3 || area.width == 0 {
            return;
        }

        let padding_top: u16 = 1;
        let padding_bottom: u16 = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return;
        }

        let prompt = format!(":{}", self.state.command_buffer);
        let prompt_width = UnicodeWidthStr::width(prompt.as_str()) as u16;
        let content_width = area.width;
        let display = if prompt_width > content_width {
            let mut truncated = String::new();
            let mut width = 0usize;
            for ch in prompt.chars().rev() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if width + w > content_width.saturating_sub(1) as usize {
                    break;
                }
                width += w;
                truncated.insert(0, ch);
            }
            format!("…{}", truncated)
        } else {
            prompt
        };

        let para = Paragraph::new(display).style(
            Style::default()
                .fg(Color::White)
                .bg(crate::ui::components::INPUT_BG),
        );
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

struct SessionStateSnapshot {
    tabs: Vec<SessionTab>,
    active_tab_index: usize,
    sidebar_visible: bool,
    tree_selected_index: usize,
    expanded_repo_ids: Vec<uuid::Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
