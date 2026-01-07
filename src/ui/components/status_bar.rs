use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

use crate::agent::{
    events::{ContextWarningLevel, ContextWindowState},
    AgentMode, AgentType, ModelRegistry, SessionId, TokenUsage,
};
use crate::git::{GitDiffStats, PrState, PrStatus};
use crate::ui::components::{
    ACCENT_ERROR, ACCENT_PRIMARY, ACCENT_SUCCESS, ACCENT_WARNING, PR_CLOSED_BG, PR_DRAFT_BG,
    PR_MERGED_BG, PR_OPEN_BG, STATUS_BAR_BG, TEXT_BRIGHT, TEXT_FAINT, TEXT_MUTED,
};
use ratatui::style::Color;

/// Status bar component showing session info
pub struct StatusBar {
    agent_type: AgentType,
    agent_mode: AgentMode,
    model: Option<String>,
    session_id: Option<SessionId>,
    token_usage: TokenUsage,
    estimated_cost: f64,
    /// Whether to show performance metrics
    show_metrics: bool,
    /// Repository name (from git remote or directory)
    repo_name: Option<String>,
    /// Current git branch
    branch_name: Option<String>,
    /// Working directory folder name
    folder_name: Option<String>,
    /// PR status for the current session
    pr_status: Option<PrStatus>,
    /// Git diff stats (+/- counts)
    git_diff_stats: GitDiffStats,
    /// Time spent in draw()
    draw_time: Duration,
    /// Time spent processing events
    event_time: Duration,
    /// Calculated FPS
    fps: f64,
    /// Scroll input-to-render latency (latest)
    scroll_latency: Duration,
    /// Average scroll input-to-render latency
    scroll_latency_avg: Duration,
    /// Scroll lines per second (rolling window)
    scroll_lines_per_sec: f64,
    /// Scroll events per second (rolling window)
    scroll_events_per_sec: f64,
    /// Whether scroll activity happened recently
    scroll_active: bool,
    /// Context window state for display
    context_state: Option<ContextWindowState>,
}

impl StatusBar {
    pub fn new(agent_type: AgentType) -> Self {
        Self {
            agent_type,
            agent_mode: AgentMode::default(),
            model: None,
            session_id: None,
            token_usage: TokenUsage::default(),
            estimated_cost: 0.0,
            show_metrics: false,
            repo_name: None,
            branch_name: None,
            folder_name: None,
            pr_status: None,
            git_diff_stats: GitDiffStats::default(),
            draw_time: Duration::ZERO,
            event_time: Duration::ZERO,
            fps: 0.0,
            scroll_latency: Duration::ZERO,
            scroll_latency_avg: Duration::ZERO,
            scroll_lines_per_sec: 0.0,
            scroll_events_per_sec: 0.0,
            scroll_active: false,
            context_state: None,
        }
    }

    pub fn set_session_id(&mut self, id: Option<SessionId>) {
        self.session_id = id;
    }

    pub fn set_agent_type(&mut self, agent_type: AgentType) {
        self.agent_type = agent_type;
    }

    pub fn set_agent_mode(&mut self, mode: AgentMode) {
        self.agent_mode = mode;
    }

    pub fn set_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    pub fn set_token_usage(&mut self, usage: TokenUsage) {
        self.token_usage = usage;
        self.update_cost();
    }

    pub fn set_context_state(&mut self, state: ContextWindowState) {
        self.context_state = Some(state);
    }

    /// Set PR status for display
    pub fn set_pr_status(&mut self, status: Option<PrStatus>) {
        self.pr_status = status;
    }

    /// Set git diff stats for display
    pub fn set_git_diff_stats(&mut self, stats: GitDiffStats) {
        self.git_diff_stats = stats;
    }

    /// Set branch name directly (from git tracker)
    pub fn set_branch_name(&mut self, branch: Option<String>) {
        self.branch_name = branch;
    }

    /// Get branch name
    pub fn branch_name(&self) -> Option<&str> {
        self.branch_name.as_deref()
    }

    /// Get git diff stats
    pub fn git_diff_stats(&self) -> &GitDiffStats {
        &self.git_diff_stats
    }

    /// Set project info for right side of status bar
    pub fn set_project_info(
        &mut self,
        repo: Option<String>,
        branch: Option<String>,
        folder: Option<String>,
    ) {
        self.repo_name = repo;
        self.branch_name = branch;
        self.folder_name = folder;
    }

    /// Set performance metrics for display
    #[allow(clippy::too_many_arguments)]
    pub fn set_metrics(
        &mut self,
        show: bool,
        draw_time: Duration,
        event_time: Duration,
        fps: f64,
        scroll_latency: Duration,
        scroll_latency_avg: Duration,
        scroll_lines_per_sec: f64,
        scroll_events_per_sec: f64,
        scroll_active: bool,
    ) {
        self.show_metrics = show;
        self.draw_time = draw_time;
        self.event_time = event_time;
        self.fps = fps;
        self.scroll_latency = scroll_latency;
        self.scroll_latency_avg = scroll_latency_avg;
        self.scroll_lines_per_sec = scroll_lines_per_sec;
        self.scroll_events_per_sec = scroll_events_per_sec;
        self.scroll_active = scroll_active;
    }

    fn update_cost(&mut self) {
        // Claude Sonnet pricing: $3/1M input, $15/1M output
        let input_cost = (self.token_usage.input_tokens as f64 / 1_000_000.0) * 3.0;
        let output_cost = (self.token_usage.output_tokens as f64 / 1_000_000.0) * 15.0;
        self.estimated_cost = input_cost + output_cost;
    }

    #[allow(dead_code)]
    fn format_tokens(&self, tokens: i64) -> String {
        if tokens >= 1_000_000 {
            format!("{:.1}M", tokens as f64 / 1_000_000.0)
        } else if tokens >= 1_000 {
            format!("{:.1}k", tokens as f64 / 1_000.0)
        } else {
            format!("{}", tokens)
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut spans = Vec::new();

        // Leading spaces
        spans.push(Span::raw("  "));

        // Mode indicator - ONLY for Claude, with accent color
        if self.agent_type == AgentType::Claude {
            spans.push(Span::styled(
                self.agent_mode.display_name(),
                Style::default().fg(ACCENT_PRIMARY),
            ));
            // Two spaces separator between mode and model
            spans.push(Span::raw("  "));
        }

        // Model name first - bright/primary color
        let model_id = self
            .model
            .clone()
            .unwrap_or_else(|| ModelRegistry::default_model(self.agent_type));
        let model_display = ModelRegistry::find_model(self.agent_type, &model_id)
            .map(|m| m.display_name)
            .unwrap_or(model_id);

        spans.push(Span::styled(
            model_display,
            Style::default().fg(TEXT_BRIGHT),
        ));

        // Agent name - muted color
        spans.push(Span::styled(
            format!(" {}", self.agent_type.display_name()),
            Style::default().fg(TEXT_MUTED),
        ));

        // Context usage indicator
        if let Some(ref ctx) = self.context_state {
            let pct = ctx.usage_percent();
            // Only show if we have meaningful context usage (> 0%)
            if pct > 0.0 || ctx.max_tokens > 0 {
                let color = match ctx.warning_level() {
                    ContextWarningLevel::Critical => ACCENT_ERROR,
                    ContextWarningLevel::High => ACCENT_WARNING,
                    ContextWarningLevel::Medium => ACCENT_WARNING,
                    ContextWarningLevel::Normal => TEXT_MUTED,
                };

                spans.push(Span::styled(" │ ", Style::default().fg(TEXT_FAINT)));
                spans.push(Span::styled("ctx:", Style::default().fg(TEXT_FAINT)));
                spans.push(Span::styled(
                    format!("{:.0}%", pct * 100.0),
                    Style::default().fg(color),
                ));

                // Show compaction count if any
                if ctx.compaction_count > 0 {
                    spans.push(Span::styled(
                        format!(" ({}×)", ctx.compaction_count),
                        Style::default().fg(TEXT_FAINT),
                    ));
                }
            }
        }

        // Note: Old processing spinner removed - now using Knight Rider spinner in footer

        // Performance metrics (when enabled)
        if self.show_metrics {
            spans.push(Span::styled(" │ ", Style::default().fg(TEXT_FAINT)));

            // FPS indicator
            let fps_color = if self.fps >= 55.0 {
                ACCENT_SUCCESS
            } else if self.fps >= 30.0 {
                ACCENT_WARNING
            } else {
                ACCENT_ERROR
            };
            spans.push(Span::styled(
                format!("FPS:{:.0} ", self.fps),
                Style::default().fg(fps_color),
            ));

            // Work time = draw + event (actual CPU work, excluding sleep)
            let work_ms = self.draw_time.as_millis() + self.event_time.as_millis();
            let work_color = if work_ms <= 8 {
                ACCENT_SUCCESS
            } else if work_ms <= 14 {
                ACCENT_WARNING
            } else {
                ACCENT_ERROR
            };
            spans.push(Span::styled(
                format!("work:{}ms ", work_ms),
                Style::default().fg(work_color),
            ));

            // Breakdown: draw/event
            spans.push(Span::styled(
                format!(
                    "(draw:{} evt:{})",
                    self.draw_time.as_millis(),
                    self.event_time.as_millis()
                ),
                Style::default().fg(TEXT_MUTED),
            ));

            // Scroll responsiveness (only highlight if active)
            let latency_ms = self.scroll_latency.as_secs_f64() * 1000.0;
            let latency_avg_ms = self.scroll_latency_avg.as_secs_f64() * 1000.0;
            let scroll_color = if self.scroll_active {
                if latency_ms <= 16.0 {
                    ACCENT_SUCCESS
                } else if latency_ms <= 33.0 {
                    ACCENT_WARNING
                } else {
                    ACCENT_ERROR
                }
            } else {
                TEXT_MUTED
            };
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("lag:{:.1}ms(avg:{:.1}) ", latency_ms, latency_avg_ms),
                Style::default().fg(scroll_color),
            ));
            spans.push(Span::styled(
                format!(
                    "scroll:{:.0}l/s ev:{:.0}/s",
                    self.scroll_lines_per_sec, self.scroll_events_per_sec
                ),
                Style::default().fg(TEXT_MUTED),
            ));
        }

        // Build right-side spans (project info)
        let right_spans = self.build_project_info_spans();

        // Render with split layout
        self.render_split_line(area, buf, spans, right_spans);
    }

    /// Build project info spans for right side of status bar
    /// New format: PR #123 · +44 -10 · feature-branch (or without PR if none)
    fn build_project_info_spans(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let mut has_content = false;

        // PR badge with colored background (if PR exists)
        if let Some(ref pr) = self.pr_status {
            if pr.exists {
                let (bg_color, fg_color) = match pr.state {
                    PrState::Open => (PR_OPEN_BG, Color::White),
                    PrState::Merged => (PR_MERGED_BG, Color::White),
                    PrState::Closed => (PR_CLOSED_BG, Color::White),
                    PrState::Draft => (PR_DRAFT_BG, Color::White),
                    PrState::Unknown => (PR_OPEN_BG, Color::White), // Default to open styling
                };

                let badge = format!(" PR #{} ", pr.number.unwrap_or(0));
                spans.push(Span::styled(
                    badge,
                    Style::default().bg(bg_color).fg(fg_color),
                ));
                has_content = true;
            }
        }

        // Git stats: +44 -10 (omit zeros)
        if self.git_diff_stats.has_changes() {
            if has_content {
                spans.push(Span::styled(" · ", Style::default().fg(TEXT_FAINT)));
            }

            let has_additions = self.git_diff_stats.additions > 0;
            let has_deletions = self.git_diff_stats.deletions > 0;

            if has_additions {
                spans.push(Span::styled(
                    format!("+{}", self.git_diff_stats.additions),
                    Style::default().fg(ACCENT_SUCCESS), // Green
                ));
            }

            if has_additions && has_deletions {
                spans.push(Span::raw(" "));
            }

            if has_deletions {
                spans.push(Span::styled(
                    format!("-{}", self.git_diff_stats.deletions),
                    Style::default().fg(ACCENT_ERROR), // Red
                ));
            }

            has_content = true;
        }

        // Branch name
        if let Some(ref branch) = self.branch_name {
            if has_content {
                spans.push(Span::styled(" · ", Style::default().fg(TEXT_FAINT)));
            }
            spans.push(Span::styled(
                branch.clone(),
                Style::default().fg(TEXT_MUTED),
            ));
            has_content = true;
        }

        // Trailing padding if we have content
        if has_content {
            spans.push(Span::raw("  "));
        }

        spans
    }

    /// Render status bar with left and right content
    fn render_split_line(
        &self,
        area: Rect,
        buf: &mut Buffer,
        left_spans: Vec<Span<'static>>,
        right_spans: Vec<Span<'static>>,
    ) {
        // Fill background
        buf.set_style(area, Style::default().bg(STATUS_BAR_BG));

        // Calculate widths
        let left_width: usize = left_spans.iter().map(|s| s.width()).sum();
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let total_width = area.width as usize;

        // Render left content
        let left_line = Line::from(left_spans);
        let left_area = Rect {
            x: area.x,
            y: area.y,
            width: (left_width as u16).min(area.width),
            height: 1,
        };
        buf.set_line(left_area.x, left_area.y, &left_line, left_area.width);

        // Render right content (if it fits)
        if !right_spans.is_empty() && right_width < total_width {
            let right_x = area.x + (total_width - right_width) as u16;
            let right_line = Line::from(right_spans);
            buf.set_line(right_x, area.y, &right_line, right_width as u16);
        }
    }
}
