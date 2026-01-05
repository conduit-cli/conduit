use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{
    ACCENT_PRIMARY, ACCENT_SUCCESS, ACCENT_WARNING, BG_ELEVATED, TAB_BAR_BG, TEXT_MUTED,
    TEXT_PRIMARY, TEXT_SECONDARY,
};
/// Spinner animation frames
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Tab bar component for switching between sessions
pub struct TabBar {
    tabs: Vec<String>,
    active: usize,
    can_add: bool,
    focused: bool,
    /// PR numbers for each tab (None = no PR)
    pr_numbers: Vec<Option<u32>>,
    /// Whether each tab is currently processing (agent working)
    processing_flags: Vec<bool>,
    /// Whether each tab has unread content (new messages arrived while not focused)
    attention_flags: Vec<bool>,
    /// Current spinner frame index
    spinner_frame: usize,
}

impl TabBar {
    pub fn new(tabs: Vec<String>, active: usize, can_add: bool) -> Self {
        let tab_count = tabs.len();
        Self {
            tabs,
            active,
            can_add,
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
            let pr_number = self.pr_numbers.get(i).copied().flatten();

            // Tab indicator - subtle marker for active tab
            if is_active && self.focused {
                spans.push(Span::styled(" ▸ ", Style::default().fg(ACCENT_PRIMARY)));
                _total_width += 3;
            } else {
                spans.push(Span::raw("   "));
                _total_width += 3;
            }

            // Processing spinner
            if is_processing {
                spans.push(Span::styled(
                    format!("{} ", self.spinner_char()),
                    Style::default().fg(ACCENT_WARNING),
                ));
                _total_width += 2;
            }
            // Attention indicator (dot) - only if not processing
            else if needs_attention {
                spans.push(Span::styled("● ", Style::default().fg(ACCENT_SUCCESS)));
                _total_width += 2;
            }

            // Tab name with proper text hierarchy
            let tab_style = if is_active {
                if self.focused {
                    Style::default()
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    // Active but unfocused - secondary text
                    Style::default().fg(TEXT_SECONDARY)
                }
            } else {
                Style::default().fg(TEXT_MUTED)
            };

            let tab_text = format!("[{}] {}", i + 1, tab);
            _total_width += tab_text.len();
            spans.push(Span::styled(tab_text, tab_style));

            spans.push(Span::raw("  "));
            _total_width += 2;

            // Show PR badge inline for active tab
            if is_active {
                if let Some(pr) = pr_number {
                    let badge = format!(" PR #{} ", pr);
                    _total_width += badge.len();
                    spans.push(Span::styled(
                        badge,
                        Style::default()
                            .bg(BG_ELEVATED)
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::raw(" "));
                    _total_width += 1;
                }
            }
        }

        // Add new tab button - muted until hovered
        if self.can_add {
            spans.push(Span::styled(
                " [+] New ",
                Style::default().fg(TEXT_MUTED),
            ));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line)
            .style(Style::default().bg(TAB_BAR_BG));

        paragraph.render(area, buf);
    }
}
