//! Error dialog component for displaying errors to users

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::dialog::{DialogFrame, InstructionBar};

/// State for the error dialog
#[derive(Debug, Clone, Default)]
pub struct ErrorDialogState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// Dialog title
    pub title: String,
    /// Main error message
    pub message: String,
    /// Optional technical details
    pub details: Option<String>,
    /// Whether details section is expanded
    pub details_expanded: bool,
}

impl ErrorDialogState {
    /// Create a new error dialog state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the dialog with a simple error message
    pub fn show(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.visible = true;
        self.title = title.into();
        self.message = message.into();
        self.details = None;
        self.details_expanded = false;
    }

    /// Show the dialog with technical details
    pub fn show_with_details(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        details: impl Into<String>,
    ) {
        self.show(title, message);
        self.details = Some(details.into());
    }

    /// Hide the dialog and reset state
    pub fn hide(&mut self) {
        self.visible = false;
        self.details_expanded = false;
    }

    /// Toggle details visibility
    pub fn toggle_details(&mut self) {
        if self.details.is_some() {
            self.details_expanded = !self.details_expanded;
        }
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check if details are available
    pub fn has_details(&self) -> bool {
        self.details.is_some()
    }
}

/// Error dialog widget
pub struct ErrorDialog<'a> {
    state: &'a ErrorDialogState,
}

impl<'a> ErrorDialog<'a> {
    pub fn new(state: &'a ErrorDialogState) -> Self {
        Self { state }
    }

    /// Calculate message height based on text length and available width
    fn calculate_message_lines(&self, width: u16) -> u16 {
        if self.state.message.is_empty() {
            return 0;
        }
        // Account for padding (2 chars on each side from DialogFrame)
        let available_width = width.saturating_sub(6) as usize;
        if available_width == 0 {
            return 1;
        }
        let msg_len = self.state.message.len();
        msg_len.div_ceil(available_width).max(1) as u16
    }

    /// Calculate details height if expanded
    fn calculate_details_lines(&self, width: u16) -> u16 {
        if !self.state.details_expanded {
            return 0;
        }
        if let Some(ref details) = self.state.details {
            let available_width = width.saturating_sub(6) as usize;
            if available_width == 0 {
                return 1;
            }
            // Count lines in details (split by newlines)
            let mut total_lines = 0u16;
            for line in details.lines() {
                let line_len = line.len();
                let wrapped_lines = line_len.div_ceil(available_width).max(1);
                total_lines += wrapped_lines as u16;
            }
            total_lines.max(1)
        } else {
            0
        }
    }

    /// Calculate the required dialog height based on content
    fn calculate_height(&self, dialog_width: u16) -> u16 {
        // Layout:
        // - Top border (1)
        // - Empty line (1)
        // - Message lines
        // - Empty line after message (1)
        // - Details toggle line (1) if details available
        // - Details content (if expanded)
        // - Empty line before button (1)
        // - OK button (1)
        // - Empty line (1)
        // - Instructions (1)
        // - Bottom border (1)
        let message_lines = self.calculate_message_lines(dialog_width);
        let details_toggle_height = if self.state.details.is_some() { 1 } else { 0 };
        let details_content_height = self.calculate_details_lines(dialog_width);
        let base_height: u16 = 12; // borders + padding + button + instructions
        base_height + message_lines + details_toggle_height + details_content_height
    }
}

impl Widget for ErrorDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        let dialog_width: u16 = 50;
        let dialog_height = self.calculate_height(dialog_width);

        // Render dialog frame with red border
        let frame = DialogFrame::new(&self.state.title, dialog_width, dialog_height)
            .border_color(Color::Red);
        let inner = frame.render(area, buf);

        if inner.height < 6 {
            return;
        }

        // Start with top padding
        let mut y_offset: u16 = 1;

        // Render error icon and message
        let message_lines = self.calculate_message_lines(dialog_width);
        let message = Paragraph::new(self.state.message.as_str())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        if inner.y + y_offset < inner.y + inner.height {
            message.render(
                Rect {
                    x: inner.x,
                    y: inner.y + y_offset,
                    width: inner.width,
                    height: message_lines,
                },
                buf,
            );
        }
        y_offset += message_lines;

        // Add spacing after message
        y_offset += 1;

        // Render details toggle if details available
        if let Some(ref details) = self.state.details {
            let toggle_text = if self.state.details_expanded {
                "▼ Details"
            } else {
                "▶ Details (press 'd' to show)"
            };
            let toggle_line = Line::from(Span::styled(
                toggle_text,
                Style::default().fg(Color::DarkGray),
            ));
            let toggle_para = Paragraph::new(toggle_line).alignment(Alignment::Center);
            if inner.y + y_offset < inner.y + inner.height.saturating_sub(4) {
                toggle_para.render(
                    Rect {
                        x: inner.x,
                        y: inner.y + y_offset,
                        width: inner.width,
                        height: 1,
                    },
                    buf,
                );
            }
            y_offset += 1;

            // Render details content if expanded
            if self.state.details_expanded {
                let details_lines = self.calculate_details_lines(dialog_width);
                let details_para = Paragraph::new(details.as_str())
                    .style(Style::default().fg(Color::DarkGray))
                    .wrap(Wrap { trim: true });
                if inner.y + y_offset < inner.y + inner.height.saturating_sub(4) {
                    details_para.render(
                        Rect {
                            x: inner.x + 2,
                            y: inner.y + y_offset,
                            width: inner.width.saturating_sub(4),
                            height: details_lines,
                        },
                        buf,
                    );
                }
                y_offset += details_lines;
            }
        }

        // Render OK button at bottom
        let button_y = inner.y + inner.height.saturating_sub(4);
        if button_y >= inner.y + y_offset {
            let button_style = Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD);

            let button_line = Line::from(Span::styled("  OK  ", button_style));
            let button = Paragraph::new(button_line).alignment(Alignment::Center);
            button.render(
                Rect {
                    x: inner.x,
                    y: button_y,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
        }

        // Render instruction bar at bottom
        let instructions_y = inner.y + inner.height.saturating_sub(1);
        let instructions = if self.state.details.is_some() {
            InstructionBar::new(vec![("Enter/Esc", "Dismiss"), ("d", "Details")])
        } else {
            InstructionBar::new(vec![("Enter/Esc", "Dismiss")])
        };
        instructions.render(
            Rect {
                x: inner.x,
                y: instructions_y,
                width: inner.width,
                height: 1,
            },
            buf,
        );
    }
}
