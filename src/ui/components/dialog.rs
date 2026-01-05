//! Reusable dialog frame and instruction bar components

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use super::{render_key_hints, KeyHintBarStyle, ACCENT_PRIMARY};
// Re-export Widget for use in render methods
pub use ratatui::widgets::Widget as WidgetTrait;

/// A centered dialog frame with title and border
pub struct DialogFrame<'a> {
    title: &'a str,
    width: u16,
    height: u16,
    border_color: Color,
}

impl<'a> DialogFrame<'a> {
    pub fn new(title: &'a str, width: u16, height: u16) -> Self {
        Self {
            title,
            width,
            height,
            border_color: ACCENT_PRIMARY,
        }
    }

    pub fn border_color(mut self, color: Color) -> Self {
        self.border_color = color;
        self
    }

    /// Render the dialog frame and return the inner area for content
    pub fn render(&self, area: Rect, buf: &mut Buffer) -> Rect {
        // Calculate dialog size (capped to screen size)
        let dialog_width = self.width.min(area.width.saturating_sub(4));
        let dialog_height = self.height.min(area.height.saturating_sub(2));

        // Center the dialog
        let x = (area.width.saturating_sub(dialog_width)) / 2;
        let y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width,
            height: dialog_height,
        };

        // Clear the dialog area
        Clear.render(dialog_area, buf);

        // Render dialog border with rounded corners
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(self.border_color));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Add horizontal padding (1 char on each side)
        Rect {
            x: inner.x.saturating_add(1),
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height,
        }
    }
}

/// An instruction bar showing keyboard shortcuts
pub struct InstructionBar<'a> {
    instructions: Vec<(&'a str, &'a str)>,
}

impl<'a> InstructionBar<'a> {
    pub fn new(instructions: Vec<(&'a str, &'a str)>) -> Self {
        Self { instructions }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        render_key_hints(
            area,
            buf,
            &self.instructions,
            KeyHintBarStyle::instruction_bar(),
        );
    }
}

/// A status line showing validation state (error, success, or empty)
pub struct StatusLine<'a> {
    error: Option<&'a str>,
    success: Option<&'a str>,
}

impl<'a> StatusLine<'a> {
    pub fn new() -> Self {
        Self {
            error: None,
            success: None,
        }
    }

    pub fn error(mut self, msg: &'a str) -> Self {
        self.error = Some(msg);
        self.success = None;
        self
    }

    pub fn success(mut self, msg: &'a str) -> Self {
        self.success = Some(msg);
        self.error = None;
        self
    }

    pub fn from_result(error: Option<&'a str>, is_valid: bool, success_msg: &'a str) -> Self {
        if let Some(err) = error {
            Self {
                error: Some(err),
                success: None,
            }
        } else if is_valid {
            Self {
                error: None,
                success: Some(success_msg),
            }
        } else {
            Self {
                error: None,
                success: None,
            }
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let line = if let Some(error) = self.error {
            Line::from(vec![
                Span::styled("✗ ", Style::default().fg(Color::Red)),
                Span::styled(error, Style::default().fg(Color::Red)),
            ])
        } else if let Some(success) = self.success {
            Line::from(vec![
                Span::styled("✓ ", Style::default().fg(Color::Green)),
                Span::styled(success, Style::default().fg(Color::Green)),
            ])
        } else {
            Line::default()
        };

        Paragraph::new(line).render(area, buf);
    }
}

impl<'a> Default for StatusLine<'a> {
    fn default() -> Self {
        Self::new()
    }
}
