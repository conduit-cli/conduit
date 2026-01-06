use ratatui::{buffer::Buffer, layout::Rect};

use crate::ui::components::{render_key_hints, KeyHintBarStyle};
use crate::ui::events::ViewMode;

/// Global footer showing keyboard shortcuts in minimal style
pub struct GlobalFooter {
    hints: Vec<(&'static str, &'static str)>,
    view_mode: ViewMode,
}

impl GlobalFooter {
    pub fn new() -> Self {
        Self {
            hints: vec![
                ("tab", "next tab"),
                ("C-o", "model"),
                ("C-t", "sidebar"),
                ("C-n", "new project"),
                ("M-S-w", "close"),
                ("C-c", "stop"),
                ("C-q", "quit"),
            ],
            view_mode: ViewMode::Chat,
        }
    }

    pub fn with_view_mode(mut self, view_mode: ViewMode) -> Self {
        self.view_mode = view_mode;
        self.update_hints();
        self
    }

    fn update_hints(&mut self) {
        self.hints = match self.view_mode {
            ViewMode::Chat => vec![
                ("tab", "next tab"),
                ("C-o", "model"),
                ("C-t", "sidebar"),
                ("C-n", "new project"),
                ("M-S-w", "close"),
                ("C-c", "stop"),
                ("C-q", "quit"),
            ],
            ViewMode::RawEvents => vec![
                ("j/k", "nav"),
                ("e", "detail"),
                ("C-j/k", "panel"),
                ("c", "copy"),
                ("C-g", "chat"),
            ],
        };
    }

    pub fn with_hints(hints: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            hints,
            view_mode: ViewMode::Chat,
        }
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
