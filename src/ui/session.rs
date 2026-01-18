use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::{
    events::{ContextCompactionEvent, ContextWarningLevel, ContextWindowState, TokenUsageEvent},
    models::ModelRegistry,
    AgentHandle, AgentInput, AgentMode, AgentType, SessionId, TokenUsage,
};
use crate::data::{QueuedMessage, QueuedMessageMode};
use crate::git::PrManager;
use crate::ui::capabilities::AgentCapabilities;
use crate::ui::components::{
    ChatView, EventDirection, InlinePromptState, InputBox, ProcessingState, RawEventsView,
    StatusBar, ThinkingIndicator, TurnSummary,
};

/// Represents a single agent session (one tab)
pub struct AgentSession {
    /// Unique identifier for this session
    pub id: Uuid,
    /// Type of agent (Claude, Codex, or Gemini)
    pub agent_type: AgentType,
    /// Agent mode (Build vs Plan)
    pub agent_mode: AgentMode,
    /// Last agent mode that received a mode prompt (Plan/Build switch)
    pub last_mode_prompt: Option<AgentMode>,
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
    /// Best-effort PID start time to reduce kill reuse risk (platform-dependent)
    pub agent_pid_start_time: Option<u64>,
    /// Optional input channel for streaming stdin payloads
    pub agent_input_tx: Option<mpsc::Sender<AgentInput>>,
    /// Pending user message that hasn't been confirmed by agent yet
    pub pending_user_message: Option<String>,
    /// Queued messages waiting to be delivered
    pub queued_messages: Vec<QueuedMessage>,
    /// Selected queued message index (for inline queue editing)
    pub queue_selection: Option<usize>,
    /// Agent capability flags
    pub capabilities: AgentCapabilities,
    /// Context window tracking state
    pub context_state: ContextWindowState,
    /// Pending context warning to display (cleared after display)
    pub pending_context_warning: Option<ContextWarning>,
    /// Fork seed ID (if this tab was created via fork)
    pub fork_seed_id: Option<Uuid>,
    /// Whether the fork welcome message has been shown (one-shot)
    pub fork_welcome_shown: bool,
    /// Suppress the next assistant reply (used for fork seed ack)
    pub suppress_next_assistant_reply: bool,
    /// Suppress the next turn summary (paired with fork seed ack)
    pub suppress_next_turn_summary: bool,
    /// AI-generated session title/description (set after first message)
    pub title: Option<String>,
    /// Whether title generation is currently in flight (prevents duplicate calls)
    pub title_generation_pending: bool,
    /// Turn summary buffered until stream end
    pub pending_turn_summary: Option<TurnSummary>,
    /// Number of tools currently in flight for this turn
    pub tools_in_flight: usize,
    /// Active inline prompt (AskUserQuestion or ExitPlanMode)
    pub inline_prompt: Option<InlinePromptState>,
    /// Pending permission prompt requests keyed by tool use ID
    pub pending_tool_permissions: HashMap<String, String>,
    /// Pending control responses waiting for a permission request
    pub pending_tool_permission_responses: HashMap<String, serde_json::Value>,
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
        let mut session = Self {
            id: Uuid::new_v4(),
            agent_type,
            agent_mode: AgentMode::default(),
            last_mode_prompt: None,
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
            agent_pid_start_time: None,
            agent_input_tx: None,
            pending_user_message: None,
            context_state: ContextWindowState::new(default_context),
            pending_context_warning: None,
            queued_messages: Vec::new(),
            queue_selection: None,
            capabilities: AgentCapabilities::for_agent(agent_type),
            fork_seed_id: None,
            fork_welcome_shown: false,
            suppress_next_assistant_reply: false,
            suppress_next_turn_summary: false,
            title: None,
            title_generation_pending: false,
            pending_turn_summary: None,
            tools_in_flight: 0,
            inline_prompt: None,
            pending_tool_permissions: HashMap::new(),
            pending_tool_permission_responses: HashMap::new(),
        };
        session.update_status();
        session
    }

    /// Create a new session with a specific working directory
    pub fn with_working_dir(agent_type: AgentType, working_dir: PathBuf) -> Self {
        let mut session = Self::new(agent_type);
        session.working_dir = Some(working_dir);
        session.update_status();
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
            .set_shell_mode(self.input_box.is_shell_mode());
        self.status_bar
            .set_session_id(self.agent_session_id.clone());
        self.status_bar.set_token_usage(self.total_usage.clone());
        self.status_bar
            .set_context_state(self.context_state.clone());
        self.status_bar.set_queue_count(self.queued_messages.len());
        self.status_bar
            .set_supports_plan_mode(self.capabilities.supports_plan_mode);

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

    /// Change agent type and/or model, updating all related state.
    /// Returns true if the agent type changed.
    pub fn set_agent_and_model(&mut self, agent_type: AgentType, model: Option<String>) -> bool {
        let agent_changed = self.agent_type != agent_type;

        self.agent_type = agent_type;
        self.capabilities = AgentCapabilities::for_agent(agent_type);
        self.model = model;
        self.last_mode_prompt = None;

        if !self.capabilities.supports_plan_mode {
            self.agent_mode = AgentMode::Build;
        }

        self.update_status();
        agent_changed
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
        self.pending_turn_summary = None;
        self.tools_in_flight = 0;
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

    pub fn queue_message(&mut self, message: QueuedMessage) {
        self.queued_messages.push(message);
        self.update_status();
    }

    pub fn queued_message_count(&self) -> usize {
        self.queued_messages.len()
    }

    pub fn clear_queue(&mut self) {
        self.queued_messages.clear();
        self.queue_selection = None;
        self.update_status();
    }

    pub fn select_queue_next(&mut self) {
        if self.queued_messages.is_empty() {
            self.queue_selection = None;
            return;
        }
        let next = match self.queue_selection {
            Some(idx) if idx + 1 < self.queued_messages.len() => idx + 1,
            Some(idx) => idx,
            None => 0,
        };
        self.queue_selection = Some(next);
    }

    pub fn select_queue_prev(&mut self) {
        if self.queued_messages.is_empty() {
            self.queue_selection = None;
            return;
        }
        let prev = match self.queue_selection {
            Some(idx) if idx > 0 => idx - 1,
            Some(idx) => idx,
            None => 0,
        };
        self.queue_selection = Some(prev);
    }

    pub fn move_queue_up(&mut self) {
        if let Some(idx) = self.queue_selection {
            if idx > 0 && idx < self.queued_messages.len() {
                self.queued_messages.swap(idx, idx - 1);
                self.queue_selection = Some(idx - 1);
            }
        }
    }

    pub fn move_queue_down(&mut self) {
        if let Some(idx) = self.queue_selection {
            if idx + 1 < self.queued_messages.len() {
                self.queued_messages.swap(idx, idx + 1);
                self.queue_selection = Some(idx + 1);
            }
        }
    }

    pub fn remove_queue_at(&mut self, idx: usize) -> Option<QueuedMessage> {
        if idx >= self.queued_messages.len() {
            return None;
        }
        let removed = self.queued_messages.remove(idx);
        if let Some(sel) = self.queue_selection {
            if sel >= self.queued_messages.len() {
                self.queue_selection = self.queued_messages.len().checked_sub(1);
            }
        }
        self.update_status();
        Some(removed)
    }

    pub fn dequeue_last(&mut self) -> Option<QueuedMessage> {
        let message = self.queued_messages.pop();
        // If selection pointed to the removed (last) element, adjust to new last.
        if self.queue_selection == Some(self.queued_messages.len()) {
            self.queue_selection = self.queued_messages.len().checked_sub(1);
        }
        if message.is_some() {
            self.update_status();
        }
        message
    }

    pub fn dequeue_selected(&mut self) -> Option<QueuedMessage> {
        if let Some(idx) = self.queue_selection {
            return self.remove_queue_at(idx);
        }
        self.dequeue_last()
    }

    pub fn has_steering_queue(&self) -> bool {
        self.queued_messages
            .iter()
            .any(|msg| msg.mode == QueuedMessageMode::Steer)
    }

    pub fn set_capabilities(&mut self, capabilities: AgentCapabilities) {
        self.capabilities = capabilities;
        self.update_status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_agent_and_model_updates_capabilities() {
        // Start with Claude session
        let mut session = AgentSession::new(AgentType::Claude);
        assert!(session.capabilities.supports_plan_mode);
        assert_eq!(session.agent_type, AgentType::Claude);

        // Switch to Codex
        let agent_changed =
            session.set_agent_and_model(AgentType::Codex, Some("gpt-4".to_string()));

        assert!(agent_changed);
        assert_eq!(session.agent_type, AgentType::Codex);
        assert!(session.capabilities.supports_plan_mode);
        assert_eq!(session.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_set_agent_and_model_preserves_plan_mode() {
        // Start with Claude in Plan mode
        let mut session = AgentSession::new(AgentType::Claude);
        session.agent_mode = AgentMode::Plan;
        assert_eq!(session.agent_mode, AgentMode::Plan);

        // Switch to Codex - should preserve Plan mode
        session.set_agent_and_model(AgentType::Codex, Some("gpt-4".to_string()));

        assert_eq!(session.agent_mode, AgentMode::Plan);
    }

    #[test]
    fn test_set_agent_and_model_preserves_build_mode() {
        // Start with Claude in Build mode
        let mut session = AgentSession::new(AgentType::Claude);
        session.agent_mode = AgentMode::Build;

        // Switch to Codex - should stay Build
        session.set_agent_and_model(AgentType::Codex, Some("gpt-4".to_string()));

        assert_eq!(session.agent_mode, AgentMode::Build);
    }

    #[test]
    fn test_set_agent_and_model_returns_false_for_same_agent() {
        let mut session = AgentSession::new(AgentType::Claude);

        // Change model but not agent type
        let agent_changed =
            session.set_agent_and_model(AgentType::Claude, Some("claude-opus".to_string()));

        assert!(!agent_changed);
        assert_eq!(session.model, Some("claude-opus".to_string()));
    }

    #[test]
    fn test_codex_session_has_correct_capabilities() {
        let session = AgentSession::new(AgentType::Codex);

        assert!(session.capabilities.supports_plan_mode);
        assert_eq!(session.agent_type, AgentType::Codex);
    }

    #[test]
    fn test_claude_session_has_correct_capabilities() {
        let session = AgentSession::new(AgentType::Claude);

        assert!(session.capabilities.supports_plan_mode);
        assert_eq!(session.agent_type, AgentType::Claude);
    }
}
