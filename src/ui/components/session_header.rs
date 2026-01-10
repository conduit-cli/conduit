//! Session header component displaying the AI-generated session title
//!
//! This component renders a fixed header below the tab bar showing
//! the session title/description. Shows "New session" in muted text
//! when no title has been generated yet.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};

use super::{bg_elevated, text_muted, text_secondary};

/// Session header component
pub struct SessionHeader<'a> {
    /// The session title (None = new session)
    title: Option<&'a str>,
}

impl<'a> SessionHeader<'a> {
    /// Create a new session header
    pub fn new(title: Option<&'a str>) -> Self {
        Self { title }
    }
}

impl Widget for SessionHeader<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        // Fill background
        let bg_style = Style::default().bg(bg_elevated());
        for x in area.x..area.x + area.width {
            buf[(x, area.y)].set_style(bg_style);
        }

        // Display text
        let text = self.title.unwrap_or("New session");
        let max_len = area.width.saturating_sub(4) as usize;
        let display = if text.len() > max_len {
            format!("{}…", &text[..max_len.saturating_sub(1)])
        } else {
            text.to_string()
        };

        // Style: secondary color if we have a title, muted if placeholder
        let text_color = if self.title.is_some() {
            text_secondary()
        } else {
            text_muted()
        };

        let line = Line::from(vec![
            Span::styled("  ", bg_style),
            Span::styled(display, bg_style.fg(text_color)),
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
