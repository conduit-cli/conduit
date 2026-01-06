use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::agent::{AgentType, ModelRegistry, SessionId, TokenUsage};
use crate::ui::components::{
    Spinner, ACCENT_ERROR, ACCENT_SUCCESS, ACCENT_WARNING, STATUS_BAR_BG, TEXT_BRIGHT, TEXT_FAINT,
    TEXT_MUTED,
};

/// Status bar component showing session info
pub struct StatusBar {
    agent_type: AgentType,
    model: Option<String>,
    session_id: Option<SessionId>,
    token_usage: TokenUsage,
    estimated_cost: f64,
    is_processing: bool,
    spinner: Spinner,
    /// Whether to show performance metrics
    show_metrics: bool,
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
}

impl StatusBar {
    pub fn new(agent_type: AgentType) -> Self {
        Self {
            agent_type,
            model: None,
            session_id: None,
            token_usage: TokenUsage::default(),
            estimated_cost: 0.0,
            is_processing: false,
            spinner: Spinner::dots(),
            show_metrics: false,
            draw_time: Duration::ZERO,
            event_time: Duration::ZERO,
            fps: 0.0,
            scroll_latency: Duration::ZERO,
            scroll_latency_avg: Duration::ZERO,
            scroll_lines_per_sec: 0.0,
            scroll_events_per_sec: 0.0,
            scroll_active: false,
        }
    }

    /// Advance spinner animation
    pub fn tick(&mut self) {
        if self.is_processing {
            self.spinner.tick();
        }
    }

    pub fn set_session_id(&mut self, id: Option<SessionId>) {
        self.session_id = id;
    }

    pub fn set_agent_type(&mut self, agent_type: AgentType) {
        self.agent_type = agent_type;
    }

    pub fn set_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    pub fn set_token_usage(&mut self, usage: TokenUsage) {
        self.token_usage = usage;
        self.update_cost();
    }

    pub fn set_processing(&mut self, processing: bool) {
        self.is_processing = processing;
    }

    /// Set performance metrics for display
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

        // Processing indicator with spinner (compact)
        if self.is_processing {
            spans.push(Span::raw(" "));
            spans.push(self.spinner.span(ACCENT_WARNING));
        }

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

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(STATUS_BAR_BG));

        paragraph.render(area, buf);
    }
}
