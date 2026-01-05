use ratatui::{buffer::Buffer, layout::Rect};

use crate::ui::components::{render_key_hints, KeyHintBarStyle, FOOTER_BG, KEY_HINT_BG};
use crate::ui::events::ViewMode;

/// Global footer showing keyboard shortcuts in neovim style
pub struct GlobalFooter {
    hints: Vec<(&'static str, &'static str)>,
    view_mode: ViewMode,
}

impl GlobalFooter {
    pub fn new() -> Self {
        Self {
            hints: vec![
                ("Tab", "Switch"),
                ("C-t", "Sidebar"),
                ("C-n", "Project"),
                ("M-S-w", "Close"),
                ("C-c", "Stop"),
                ("C-q", "Quit"),
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
                ("Tab", "Switch"),
                ("C-t", "Sidebar"),
                ("C-n", "Project"),
                ("M-S-w", "Close"),
                ("C-c", "Stop"),
                ("C-q", "Quit"),
            ],
            ViewMode::RawEvents => vec![
                ("j/k", "Nav"),
                ("e", "Detail"),
                ("C-j/k", "Panel"),
                ("c", "Copy"),
                ("C-g", "Chat"),
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
        render_key_hints(
            area,
            buf,
            &self.hints,
            KeyHintBarStyle::footer_bar(KEY_HINT_BG, FOOTER_BG),
        );
    }
}

impl Default for GlobalFooter {
    fn default() -> Self {
        Self::new()
    }
}
