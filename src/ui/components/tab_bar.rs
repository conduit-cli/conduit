use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{
    accent_primary, accent_success, accent_warning, bg_elevated, tab_bar_bg, text_muted,
    text_primary, text_secondary,
};
/// Spinner animation frames
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Tab bar component for switching between sessions
pub struct TabBar {
    tabs: Vec<String>,
    active: usize,
    focused: bool,
    /// PR numbers for each tab (None = no PR)
    /// TODO: Remove if sidebar PR display is not implemented, or use for future tab tooltip
    #[allow(dead_code)]
    pr_numbers: Vec<Option<u32>>,
    /// Whether each tab is currently processing (agent working)
    processing_flags: Vec<bool>,
    /// Whether each tab has unread content (new messages arrived while not focused)
    attention_flags: Vec<bool>,
    /// Current spinner frame index
    spinner_frame: usize,
}

impl TabBar {
    pub fn new(tabs: Vec<String>, active: usize) -> Self {
        let tab_count = tabs.len();
        Self {
            tabs,
            active,
            focused: true,
            pr_numbers: vec![None; tab_count],
            processing_flags: vec![false; tab_count],
            attention_flags: vec![false; tab_count],
            spinner_frame: 0,
        }
    }

    /// Set whether the tab bar is focused
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set tab states (PR numbers, processing, attention flags)
    pub fn with_tab_states(
        mut self,
        pr_numbers: Vec<Option<u32>>,
        processing: Vec<bool>,
        attention: Vec<bool>,
    ) -> Self {
        self.pr_numbers = pr_numbers;
        self.processing_flags = processing;
        self.attention_flags = attention;
        self
    }

    /// Set current spinner frame
    pub fn with_spinner_frame(mut self, frame: usize) -> Self {
        self.spinner_frame = frame;
        self
    }

    /// Get spinner character for current frame
    fn spinner_char(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()]
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut spans = Vec::new();
        let mut _total_width: usize = 0;

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active;
            let is_processing = self.processing_flags.get(i).copied().unwrap_or(false);
            let needs_attention = self.attention_flags.get(i).copied().unwrap_or(false);

            // Base style for active tabs (with background)
            let active_bg_style = if is_active {
                Style::default().bg(bg_elevated())
            } else {
                Style::default()
            };

            // Tab indicator - subtle marker for active tab
            if is_active && self.focused {
                spans.push(Span::styled(" ▸ ", active_bg_style.fg(accent_primary())));
                _total_width += 3;
            } else if is_active {
                // Active but unfocused - just padding with background
                spans.push(Span::styled("   ", active_bg_style));
                _total_width += 3;
            } else {
                spans.push(Span::raw("   "));
                _total_width += 3;
            }

            // Processing spinner
            if is_processing {
                spans.push(Span::styled(
                    format!("{} ", self.spinner_char()),
                    active_bg_style.fg(accent_warning()),
                ));
                _total_width += 2;
            }
            // Attention indicator (dot) - only if not processing
            else if needs_attention {
                spans.push(Span::styled("● ", active_bg_style.fg(accent_success())));
                _total_width += 2;
            }

            // Tab name with proper text hierarchy
            let tab_style = if is_active {
                if self.focused {
                    active_bg_style
                        .fg(text_primary())
                        .add_modifier(Modifier::BOLD)
                } else {
                    // Active but unfocused - secondary text
                    active_bg_style.fg(text_secondary())
                }
            } else {
                Style::default().fg(text_muted())
            };

            let tab_text = format!("[{}] {}", i + 1, tab);
            _total_width += tab_text.len();
            spans.push(Span::styled(tab_text, tab_style));

            // Trailing padding (with background for active tabs)
            if is_active {
                spans.push(Span::styled("  ", active_bg_style));
            } else {
                spans.push(Span::raw("  "));
            }
            _total_width += 2;

            // Note: PR badge moved to status bar
        }

        // Render the tab line on the first row
        let tab_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(tab_bar_bg()));
        paragraph.render(tab_area, buf);
    }
}
