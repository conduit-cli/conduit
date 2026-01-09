use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

use super::KnightRiderSpinner;
use crate::ui::components::{render_key_hints_responsive, text_muted, KeyHintBarStyle};
use crate::ui::events::{InputMode, ViewMode};

/// Context for determining which footer hints to show
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FooterContext {
    /// Empty state - no tabs open
    Empty,
    /// Normal chat mode with tabs
    #[default]
    Chat,
    /// Sidebar navigation mode
    Sidebar,
    /// Raw events view mode
    RawEvents,
}

impl FooterContext {
    /// Determine footer context from view mode, input mode, and whether tabs exist
    pub fn from_state(view_mode: ViewMode, input_mode: InputMode, has_tabs: bool) -> Self {
        if !has_tabs {
            return FooterContext::Empty;
        }

        match view_mode {
            ViewMode::RawEvents => FooterContext::RawEvents,
            ViewMode::Chat => {
                if input_mode == InputMode::SidebarNavigation {
                    FooterContext::Sidebar
                } else {
                    FooterContext::Chat
                }
            }
        }
    }
}

/// Global footer showing keyboard shortcuts in minimal style
/// Layout: [Spinner][Message]                    [Key Hints (right-aligned)]
pub struct GlobalFooter<'a> {
    hints: Vec<(&'static str, &'static str)>,
    spinner: Option<&'a KnightRiderSpinner>,
    message: Option<&'a str>,
}

impl<'a> GlobalFooter<'a> {
    pub fn new() -> Self {
        Self {
            hints: Self::chat_hints(),
            spinner: None,
            message: None,
        }
    }

    /// Create footer for a specific context
    pub fn for_context(context: FooterContext) -> Self {
        Self {
            hints: match context {
                FooterContext::Empty => Self::empty_hints(),
                FooterContext::Chat => Self::chat_hints(),
                FooterContext::Sidebar => Self::sidebar_hints(),
                FooterContext::RawEvents => Self::raw_events_hints(),
            },
            spinner: None,
            message: None,
        }
    }

    /// Create footer from app state
    pub fn from_state(view_mode: ViewMode, input_mode: InputMode, has_tabs: bool) -> Self {
        let context = FooterContext::from_state(view_mode, input_mode, has_tabs);
        Self::for_context(context)
    }

    /// Set spinner for left side of footer
    pub fn with_spinner(mut self, spinner: Option<&'a KnightRiderSpinner>) -> Self {
        self.spinner = spinner;
        self
    }

    /// Set message for left side of footer (after spinner)
    pub fn with_message(mut self, message: Option<&'a str>) -> Self {
        self.message = message;
        self
    }

    /// Get hints for empty state (no tabs open)
    pub fn empty_hints() -> Vec<(&'static str, &'static str)> {
        vec![
            ("C-n", "new project"),
            ("C-t", "sidebar"),
            ("M-i", "import session"),
            ("C-q", "quit"),
        ]
    }

    /// Get hints for chat mode
    pub fn chat_hints() -> Vec<(&'static str, &'static str)> {
        vec![
            ("tab", "next tab"),
            ("C-o", "model"),
            ("C-t", "sidebar"),
            ("C-n", "new project"),
            ("M-S-w", "close"),
            ("C-c", "stop"),
            ("C-q", "quit"),
        ]
    }

    /// Get hints for sidebar navigation mode
    pub fn sidebar_hints() -> Vec<(&'static str, &'static str)> {
        vec![
            ("↑↓", "navigate"),
            ("enter", "select"),
            ("h/l", "collapse/expand"),
            ("r", "add repo"),
            ("C-n", "new project"),
            ("esc", "exit"),
        ]
    }

    /// Get hints for raw events view mode
    pub fn raw_events_hints() -> Vec<(&'static str, &'static str)> {
        vec![
            ("j/k", "nav"),
            ("e", "detail"),
            ("C-j/k", "panel"),
            ("c", "copy"),
            ("C-g", "chat"),
        ]
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Build left side content (spinner + message)
        let mut left_spans: Vec<Span> = Vec::new();

        // Add spinner if present
        if let Some(spinner) = self.spinner {
            left_spans.push(Span::raw("  "));
            left_spans.extend(spinner.render());
        }

        // Add message if present
        if let Some(message) = self.message {
            if !left_spans.is_empty() {
                left_spans.push(Span::raw("  ")); // Gap between spinner and message
            } else {
                left_spans.push(Span::raw("  ")); // Leading space
            }
            left_spans.push(Span::styled(message, Style::default().fg(text_muted())));
        }

        // Calculate left side width
        let left_width: u16 = left_spans.iter().map(|s| s.width() as u16).sum();

        // Render left side if present
        if !left_spans.is_empty() {
            let left_line = Line::from(left_spans);
            buf.set_line(area.x, area.y, &left_line, left_width);
        }

        // Reserve space for spinner/message, key hints get the rest (right-aligned)
        let reserved_left = if left_width > 0 { left_width + 2 } else { 0 }; // +2 for gap
        let max_hints_width = area.width.saturating_sub(reserved_left);

        // Render key hints responsively (right-aligned, removes from left when too wide)
        render_key_hints_responsive(
            area,
            buf,
            &self.hints,
            KeyHintBarStyle::minimal_footer(),
            Some(max_hints_width),
        );
    }
}

impl Default for GlobalFooter<'_> {
    fn default() -> Self {
        Self::new()
    }
}
