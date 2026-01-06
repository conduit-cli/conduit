use ratatui::{buffer::Buffer, layout::Rect};

use crate::ui::components::{render_key_hints, KeyHintBarStyle};
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
pub struct GlobalFooter {
    hints: Vec<(&'static str, &'static str)>,
}

impl GlobalFooter {
    pub fn new() -> Self {
        Self {
            hints: Self::chat_hints(),
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
        }
    }

    /// Create footer from app state
    pub fn from_state(view_mode: ViewMode, input_mode: InputMode, has_tabs: bool) -> Self {
        let context = FooterContext::from_state(view_mode, input_mode, has_tabs);
        Self::for_context(context)
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

    // Keep the old API for backwards compatibility
    pub fn with_view_mode(self, view_mode: ViewMode) -> Self {
        let context = match view_mode {
            ViewMode::Chat => FooterContext::Chat,
            ViewMode::RawEvents => FooterContext::RawEvents,
        };
        Self::for_context(context)
    }

    pub fn with_hints(hints: Vec<(&'static str, &'static str)>) -> Self {
        Self { hints }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        render_key_hints(area, buf, &self.hints, KeyHintBarStyle::minimal_footer());
    }
}

impl Default for GlobalFooter {
    fn default() -> Self {
        Self::new()
    }
}
