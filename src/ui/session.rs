use std::path::PathBuf;

use serde_json::Value;
use uuid::Uuid;

use crate::agent::{
    events::{ContextCompactionEvent, ContextWarningLevel, ContextWindowState, TokenUsageEvent},
    models::ModelRegistry,
    AgentHandle, AgentMode, AgentType, SessionId, TokenUsage,
};
use crate::git::PrManager;
use crate::ui::components::{
    ChatView, EventDirection, InputBox, ProcessingState, RawEventsView, StatusBar,
    ThinkingIndicator, TurnSummary,
};

/// Represents a single agent session (one tab)
pub struct AgentSession {
    /// Unique identifier for this session
    pub id: Uuid,
    /// Type of agent (Claude or Codex)
    pub agent_type: AgentType,
    /// Agent mode (Build vs Plan) - only applicable to Claude
    pub agent_mode: AgentMode,
    /// Selected model for this session
    pub model: Option<String>,
    /// Associated workspace ID (for project context)
    pub workspace_id: Option<Uuid>,
    /// Working directory for the agent (workspace path)
    pub working_dir: Option<PathBuf>,
    /// Project/repository name (for display in tab)
    pub project_name: Option<String>,
    /// Workspace name (for display in tab)
    pub workspace_name: Option<String>,
    /// Session ID to resume on next prompt (set when restoring from saved state)
    pub resume_session_id: Option<SessionId>,
    /// Chat view component
    pub chat_view: ChatView,
    /// Raw events view (debug)
    pub raw_events_view: RawEventsView,
    /// Input box component
    pub input_box: InputBox,
    /// Status bar component
    pub status_bar: StatusBar,
    /// Thinking indicator (shown while processing)
    pub thinking_indicator: ThinkingIndicator,
    /// Current turn summary (built during processing)
    pub current_turn_summary: TurnSummary,
    /// Handle to the running agent process (if any)
    pub agent_handle: Option<AgentHandle>,
    /// Agent session ID (from the agent itself)
    pub agent_session_id: Option<SessionId>,
    /// Whether the agent is currently processing
    pub is_processing: bool,
    /// Accumulated token usage
    pub total_usage: TokenUsage,
    /// Turn count
    pub turn_count: u32,
    /// PR number if current branch has an open PR
    pub pr_number: Option<u32>,
    /// Whether this tab has unread content (new messages arrived while not focused)
    pub needs_attention: bool,
    /// PID of the running agent subprocess (for interrupt/kill)
    pub agent_pid: Option<u32>,
    /// Pending user message that hasn't been confirmed by agent yet
    pub pending_user_message: Option<String>,
    /// Context window tracking state
    pub context_state: ContextWindowState,
    /// Pending context warning to display (cleared after display)
    pub pending_context_warning: Option<ContextWarning>,
}

/// Context warning notification
#[derive(Debug, Clone)]
pub struct ContextWarning {
    pub level: ContextWarningLevel,
    pub message: String,
}

impl AgentSession {
    pub fn new(agent_type: AgentType) -> Self {
        let default_context = ModelRegistry::default_context_window(agent_type);
        Self {
            id: Uuid::new_v4(),
            agent_type,
            agent_mode: AgentMode::default(),
            model: None,
            workspace_id: None,
            working_dir: None,
            project_name: None,
            workspace_name: None,
            resume_session_id: None,
            chat_view: ChatView::new(),
            raw_events_view: RawEventsView::new(),
            input_box: InputBox::new(),
            status_bar: StatusBar::new(agent_type),
            thinking_indicator: ThinkingIndicator::new(),
            current_turn_summary: TurnSummary::new(),
            agent_handle: None,
            agent_session_id: None,
            is_processing: false,
            total_usage: TokenUsage::default(),
            turn_count: 0,
            pr_number: None,
            needs_attention: false,
            agent_pid: None,
            pending_user_message: None,
            context_state: ContextWindowState::new(default_context),
            pending_context_warning: None,
        }
    }

    /// Create a new session with a specific working directory
    pub fn with_working_dir(agent_type: AgentType, working_dir: PathBuf) -> Self {
        let mut session = Self::new(agent_type);
        session.working_dir = Some(working_dir);
        session
    }

    /// Get display name for the tab
    pub fn tab_name(&self) -> String {
        // Use project_name and workspace_name if available
        match (&self.project_name, &self.workspace_name) {
            (Some(project), Some(workspace)) => {
                format!("{} ({})", project, workspace)
            }
            (Some(project), None) => project.clone(),
            (None, Some(workspace)) => workspace.clone(),
            (None, None) => {
                // Fall back to deriving from working_dir
                let name = self
                    .working_dir
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(String::from);

                match name {
                    Some(dir_name) => dir_name,
                    None => format!("{} (new)", self.agent_type.as_str()),
                }
            }
        }
    }

    /// Update status bar with current state
    pub fn update_status(&mut self) {
        self.status_bar.set_agent_type(self.agent_type);
        self.status_bar.set_agent_mode(self.agent_mode);
        self.status_bar.set_model(self.model.clone());
        self.status_bar
            .set_session_id(self.agent_session_id.clone());
        self.status_bar.set_token_usage(self.total_usage.clone());
        self.status_bar
            .set_context_state(self.context_state.clone());

        // Update project info for right side of status bar
        if let Some(working_dir) = &self.working_dir {
            let repo_name = PrManager::get_repo_name(working_dir);
            let branch_name = PrManager::get_current_branch(working_dir);
            let folder_name = working_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from);
            self.status_bar
                .set_project_info(repo_name, branch_name, folder_name);
        }

        let session_id = self
            .agent_session_id
            .as_ref()
            .or(self.resume_session_id.as_ref())
            .map(|s| s.as_str().to_string());
        self.raw_events_view.set_session_id(session_id);
    }

    /// Add token usage from a turn
    pub fn add_usage(&mut self, usage: TokenUsage) {
        // Update turn summary with this turn's tokens
        self.current_turn_summary.input_tokens = usage.input_tokens.max(0) as u64;
        self.current_turn_summary.output_tokens = usage.output_tokens.max(0) as u64;

        // Accumulate total usage
        self.total_usage.input_tokens += usage.input_tokens;
        self.total_usage.output_tokens += usage.output_tokens;
        self.total_usage.cached_tokens += usage.cached_tokens;
        self.total_usage.total_tokens += usage.total_tokens;
        self.turn_count += 1;
        self.update_status();
    }

    /// Start processing (resets thinking indicator and turn summary)
    pub fn start_processing(&mut self) {
        self.is_processing = true;
        self.thinking_indicator.reset();
        self.current_turn_summary = TurnSummary::new();
        self.update_status();
    }

    /// Stop processing and finalize turn summary
    pub fn stop_processing(&mut self) {
        self.is_processing = false;
        // Finalize the turn summary with duration and tokens
        let duration = self.thinking_indicator.elapsed();
        self.current_turn_summary.duration_secs = duration.as_secs();
        self.update_status();
    }

    /// Record a file change for the current turn
    pub fn record_file_change(
        &mut self,
        filename: impl Into<String>,
        additions: usize,
        deletions: usize,
    ) {
        self.current_turn_summary
            .add_file(filename, additions, deletions);
    }

    /// Add tokens to the thinking indicator
    pub fn add_streaming_tokens(&mut self, count: usize) {
        self.thinking_indicator.add_tokens(count);
    }

    /// Set the current processing state
    pub fn set_processing_state(&mut self, state: ProcessingState) {
        self.thinking_indicator.set_state(state);
    }

    /// Advance animation (called on tick)
    pub fn tick(&mut self) {
        if self.is_processing {
            self.thinking_indicator.tick();
        }
    }

    /// Record a raw event for the debug view
    pub fn record_raw_event(
        &mut self,
        direction: EventDirection,
        event_type: impl Into<String>,
        raw_json: Value,
    ) {
        self.raw_events_view
            .push_event(direction, event_type, raw_json);
    }

    /// Initialize context state based on selected model
    pub fn init_context_for_model(&mut self) {
        let default_model = ModelRegistry::default_model(self.agent_type);
        let model_id = self.model.as_deref().unwrap_or(&default_model);
        let max_tokens = ModelRegistry::context_window(self.agent_type, model_id);
        self.context_state = ContextWindowState::new(max_tokens);
    }

    /// Update context state from token usage event
    pub fn update_context_usage(&mut self, event: &TokenUsageEvent) {
        let prev_level = self.context_state.warning_level();
        self.context_state.update_from_usage(event);
        let new_level = self.context_state.warning_level();

        // Generate warning on level escalation
        if new_level != prev_level && new_level != ContextWarningLevel::Normal {
            let pct = self.context_state.usage_percent();
            self.pending_context_warning = Some(ContextWarning {
                level: new_level,
                message: Self::warning_message(new_level, pct),
            });
        }

        self.update_status();
    }

    /// Handle context compaction event
    pub fn handle_compaction(&mut self, event: ContextCompactionEvent) {
        let tokens_freed = event.tokens_before - event.tokens_after;
        self.context_state.record_compaction(event.clone());

        // Create notification for user
        self.pending_context_warning = Some(ContextWarning {
            level: ContextWarningLevel::Normal, // Compaction is informational
            message: format!(
                "Context compacted: {} tokens freed ({})",
                ContextWindowState::format_tokens(tokens_freed),
                event.reason
            ),
        });

        self.update_status();
    }

    fn warning_message(level: ContextWarningLevel, pct: f32) -> String {
        match level {
            ContextWarningLevel::Critical => {
                format!("Context {:.0}% full - compaction imminent", pct * 100.0)
            }
            ContextWarningLevel::High => {
                format!("Context {:.0}% full - approaching limit", pct * 100.0)
            }
            ContextWarningLevel::Medium => {
                format!("Context {:.0}% used", pct * 100.0)
            }
            ContextWarningLevel::Normal => String::new(),
        }
    }
}
