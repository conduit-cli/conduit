use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use crate::ui::components::{
    AddRepoDialogState, AgentSelectorState, BaseDirDialogState, ConfirmationDialogState,
    ErrorDialogState, HelpDialogState, ModelSelectorState, ProjectPickerState,
    SessionImportPickerState, SidebarData, SidebarState,
};
use crate::ui::events::{InputMode, ViewMode};
use crate::ui::tab_manager::TabManager;

/// Performance metrics for monitoring frame timing.
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    /// Time for the last complete frame.
    pub frame_time: Duration,
    /// Time spent in terminal.draw().
    pub draw_time: Duration,
    /// Time spent processing events.
    pub event_time: Duration,
    /// Calculated FPS (rolling average).
    pub fps: f64,
    /// Input-to-render latency for the last scroll event.
    pub scroll_latency: Duration,
    /// Average scroll input-to-render latency.
    pub scroll_latency_avg: Duration,
    /// Scroll lines per second (rolling window).
    pub scroll_lines_per_sec: f64,
    /// Scroll events per second (rolling window).
    pub scroll_events_per_sec: f64,
    /// Whether scroll activity happened recently.
    pub scroll_active: bool,
    /// History of frame times for FPS calculation.
    frame_history: VecDeque<Duration>,
    /// Scroll latency history for averaging.
    scroll_latency_history: VecDeque<Duration>,
    /// Scroll events for rolling throughput (timestamp, lines).
    scroll_events: VecDeque<(Instant, usize)>,
    /// Last scroll input time.
    last_scroll_input_at: Option<Instant>,
    /// Whether a scroll latency sample is pending next render.
    pending_scroll_latency: bool,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            frame_time: Duration::ZERO,
            draw_time: Duration::ZERO,
            event_time: Duration::ZERO,
            fps: 0.0,
            frame_history: VecDeque::with_capacity(60),
            scroll_latency: Duration::ZERO,
            scroll_latency_avg: Duration::ZERO,
            scroll_lines_per_sec: 0.0,
            scroll_events_per_sec: 0.0,
            scroll_active: false,
            scroll_latency_history: VecDeque::with_capacity(120),
            scroll_events: VecDeque::with_capacity(240),
            last_scroll_input_at: None,
            pending_scroll_latency: false,
        }
    }

    /// Record a frame's duration and update FPS.
    pub fn record_frame(&mut self, duration: Duration) {
        self.frame_time = duration;
        self.frame_history.push_back(duration);
        if self.frame_history.len() > 60 {
            self.frame_history.pop_front();
        }
        self.update_fps();
    }

    fn update_fps(&mut self) {
        if self.frame_history.is_empty() {
            self.fps = 0.0;
            return;
        }
        let total: Duration = self.frame_history.iter().sum();
        let avg = total.as_secs_f64() / self.frame_history.len() as f64;
        self.fps = if avg > 0.0 { 1.0 / avg } else { 0.0 };
    }

    /// Record a scroll input event with number of lines moved.
    pub fn record_scroll_event(&mut self, lines: usize) {
        let now = Instant::now();
        self.last_scroll_input_at = Some(now);
        self.pending_scroll_latency = true;
        self.scroll_events.push_back((now, lines));
    }

    /// Update scroll throughput/active metrics at end of frame.
    pub fn on_frame_end(&mut self, frame_end: Instant) {
        let window = Duration::from_secs(1);

        // Prune old scroll events outside the rolling window.
        while let Some((ts, _)) = self.scroll_events.front() {
            if frame_end.duration_since(*ts) > window {
                self.scroll_events.pop_front();
            } else {
                break;
            }
        }

        let total_lines: usize = self.scroll_events.iter().map(|(_, lines)| *lines).sum();
        let events = self.scroll_events.len();
        let window_secs = window.as_secs_f64();
        self.scroll_lines_per_sec = total_lines as f64 / window_secs;
        self.scroll_events_per_sec = events as f64 / window_secs;

        self.scroll_active = self
            .last_scroll_input_at
            .map(|ts| frame_end.duration_since(ts) <= window)
            .unwrap_or(false);
    }

    /// Record scroll latency at draw completion.
    pub fn on_draw_end(&mut self, draw_end: Instant) {
        if !self.pending_scroll_latency {
            return;
        }

        if let Some(input_at) = self.last_scroll_input_at {
            let latency = draw_end.duration_since(input_at);
            self.scroll_latency = latency;
            self.scroll_latency_history.push_back(latency);
            if self.scroll_latency_history.len() > 120 {
                self.scroll_latency_history.pop_front();
            }
            if !self.scroll_latency_history.is_empty() {
                let total: Duration = self.scroll_latency_history.iter().sum();
                let avg = total.as_secs_f64() / self.scroll_latency_history.len() as f64;
                self.scroll_latency_avg = Duration::from_secs_f64(avg);
            }
        }
        self.pending_scroll_latency = false;
    }
}

/// UI state snapshot for the application.
pub struct AppState {
    pub should_quit: bool,
    pub tab_manager: TabManager,
    pub input_mode: InputMode,
    pub view_mode: ViewMode,
    pub tick_count: u32,
    pub show_first_time_splash: bool,
    pub sidebar_state: SidebarState,
    pub sidebar_data: SidebarData,
    pub add_repo_dialog_state: AddRepoDialogState,
    pub model_selector_state: ModelSelectorState,
    pub agent_selector_state: AgentSelectorState,
    pub base_dir_dialog_state: BaseDirDialogState,
    pub project_picker_state: ProjectPickerState,
    pub session_import_state: SessionImportPickerState,
    pub confirmation_dialog_state: ConfirmationDialogState,
    pub error_dialog_state: ErrorDialogState,
    pub help_dialog_state: HelpDialogState,
    pub command_buffer: String,
    pub sidebar_area: Option<Rect>,
    pub tab_bar_area: Option<Rect>,
    pub chat_area: Option<Rect>,
    pub input_area: Option<Rect>,
    pub status_bar_area: Option<Rect>,
    pub footer_area: Option<Rect>,
    pub raw_events_area: Option<Rect>,
    pub metrics: PerformanceMetrics,
    pub show_metrics: bool,
    pub spinner_frame: usize,
    pub last_sidebar_click: Option<(Instant, usize)>,
    pub last_raw_events_click: Option<(Instant, usize)>,
    pub scroll_drag: Option<ScrollDragTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDragTarget {
    Chat,
    Input,
    HelpDialog,
    ProjectPicker,
    SessionImport,
    RawEventsList,
    RawEventsDetail,
}

impl AppState {
    pub fn new(max_tabs: usize) -> Self {
        Self {
            should_quit: false,
            tab_manager: TabManager::new(max_tabs),
            input_mode: InputMode::Normal,
            view_mode: ViewMode::Chat,
            tick_count: 0,
            show_first_time_splash: true,
            sidebar_state: SidebarState::new(),
            sidebar_data: SidebarData::new(),
            add_repo_dialog_state: AddRepoDialogState::new(),
            model_selector_state: ModelSelectorState::default(),
            agent_selector_state: AgentSelectorState::new(),
            base_dir_dialog_state: BaseDirDialogState::new(),
            project_picker_state: ProjectPickerState::new(),
            session_import_state: SessionImportPickerState::new(),
            confirmation_dialog_state: ConfirmationDialogState::new(),
            error_dialog_state: ErrorDialogState::new(),
            help_dialog_state: HelpDialogState::new(),
            command_buffer: String::new(),
            sidebar_area: None,
            tab_bar_area: None,
            chat_area: None,
            input_area: None,
            status_bar_area: None,
            footer_area: None,
            raw_events_area: None,
            metrics: PerformanceMetrics::new(),
            show_metrics: false,
            spinner_frame: 0,
            last_sidebar_click: None,
            last_raw_events_click: None,
            scroll_drag: None,
        }
    }
}
