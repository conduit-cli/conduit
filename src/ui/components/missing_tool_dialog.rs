//! Missing tool dialog component
//!
//! This dialog is shown when a required external tool is not found.
//! It displays installation instructions and allows the user to provide
//! a custom path to the tool.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::util::tools::{Tool, ToolAvailability};

use super::dialog::{DialogFrame, InstructionBar, StatusLine};
use super::{accent_error, accent_warning, text_muted, text_primary, text_secondary};

/// Result of the missing tool dialog interaction
#[derive(Debug, Clone)]
pub enum MissingToolResult {
    /// User provided a valid path
    PathProvided(std::path::PathBuf),
    /// User chose to skip (only for optional tools)
    Skipped,
    /// User chose to quit the application
    Quit,
}

/// State for the missing tool dialog
#[derive(Debug, Clone)]
pub struct MissingToolDialogState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// The tool that is missing
    pub tool: Tool,
    /// Whether this is a required tool (no skip option)
    pub is_required: bool,
    /// Context message (why this tool is needed)
    pub context_message: Option<String>,
    /// Current input text
    pub input: String,
    /// Cursor position in input
    pub cursor: usize,
    /// Validation error message
    pub error: Option<String>,
    /// Whether validation is in progress
    pub validating: bool,
}

impl Default for MissingToolDialogState {
    fn default() -> Self {
        Self {
            visible: false,
            tool: Tool::Git,
            is_required: true,
            context_message: None,
            input: String::new(),
            cursor: 0,
            error: None,
            validating: false,
        }
    }
}

impl MissingToolDialogState {
    /// Show the dialog for a missing tool
    pub fn show(&mut self, tool: Tool) {
        self.visible = true;
        self.tool = tool;
        self.is_required =
            tool.is_required() || (tool.is_agent() && !self.has_any_other_agent(tool));
        self.context_message = None;
        self.input.clear();
        self.cursor = 0;
        self.error = None;
        self.validating = false;
    }

    /// Show the dialog with a context message (e.g., "To use PR features...")
    pub fn show_with_context(&mut self, tool: Tool, context: impl Into<String>) {
        self.show(tool);
        self.context_message = Some(context.into());
        // Context dialogs are usually for optional features
        self.is_required = false;
    }

    /// Show the dialog for missing agent (blocks until at least one agent is configured)
    pub fn show_for_agent(&mut self, preferred_tool: Tool) {
        self.show(preferred_tool);
        self.is_required = true; // Must have at least one agent
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
        self.error = None;
        self.validating = false;
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Insert a character at cursor position
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.error = None; // Clear error on input change
    }

    /// Delete character before cursor
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev_char_start = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.remove(prev_char_start);
            self.cursor = prev_char_start;
            self.error = None;
        }
    }

    /// Delete character at cursor
    pub fn delete(&mut self) {
        if self.cursor < self.input.len() {
            self.input.remove(self.cursor);
            self.error = None;
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor = self.input[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.input.len());
        }
    }

    /// Move cursor to start
    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end
    pub fn move_to_end(&mut self) {
        self.cursor = self.input.len();
    }

    /// Clear the input
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.error = None;
    }

    /// Validate the current input and return the result
    pub fn validate(&mut self) -> Option<MissingToolResult> {
        if self.input.trim().is_empty() {
            self.error = Some("Please enter a path".to_string());
            return None;
        }

        match ToolAvailability::validate_path(&self.input) {
            Ok(path) => {
                self.error = None;
                Some(MissingToolResult::PathProvided(path))
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    /// Set an error message
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    /// Check if there's another agent available (for agent requirement logic)
    fn has_any_other_agent(&self, _current_tool: Tool) -> bool {
        // This would need access to ToolAvailability to check
        // For now, return false to be safe (require agent if checking)
        false
    }
}

/// Missing tool dialog widget
pub struct MissingToolDialog<'a> {
    state: &'a MissingToolDialogState,
}

impl<'a> MissingToolDialog<'a> {
    pub fn new(state: &'a MissingToolDialogState) -> Self {
        Self { state }
    }
}

impl Widget for MissingToolDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        let dialog_width: u16 = 60;
        let dialog_height: u16 = 20;

        // Choose border color based on severity
        let border_color = if self.state.is_required {
            accent_error()
        } else {
            accent_warning()
        };

        // Render dialog frame
        let title = format!("Missing Tool: {}", self.state.tool.display_name());
        let frame =
            DialogFrame::new(&title, dialog_width, dialog_height).border_color(border_color);
        let inner = frame.render(area, buf);

        if inner.height < 10 {
            return;
        }

        let mut y_offset: u16 = 1;

        // Render description
        let description = if let Some(ref context) = self.state.context_message {
            context.clone()
        } else {
            self.state.tool.description().to_string()
        };

        let desc_para = Paragraph::new(description.as_str())
            .style(Style::default().fg(text_primary()))
            .wrap(Wrap { trim: true });
        let desc_height = 2u16;
        desc_para.render(
            Rect {
                x: inner.x,
                y: inner.y + y_offset,
                width: inner.width,
                height: desc_height,
            },
            buf,
        );
        y_offset += desc_height + 1;

        // Render installation instructions header
        let install_header = Line::from(Span::styled(
            "Install:",
            Style::default()
                .fg(text_secondary())
                .add_modifier(Modifier::BOLD),
        ));
        Paragraph::new(install_header).render(
            Rect {
                x: inner.x,
                y: inner.y + y_offset,
                width: inner.width,
                height: 1,
            },
            buf,
        );
        y_offset += 1;

        // Render installation commands
        for line in self.state.tool.install_instructions().lines() {
            let install_line = Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(text_muted()),
            ));
            Paragraph::new(install_line).render(
                Rect {
                    x: inner.x,
                    y: inner.y + y_offset,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
            y_offset += 1;
        }

        y_offset += 1;

        // Render "Or enter path" prompt
        let path_header = Line::from(Span::styled(
            format!("Or enter the path to {}:", self.state.tool.binary_name()),
            Style::default().fg(text_secondary()),
        ));
        Paragraph::new(path_header).render(
            Rect {
                x: inner.x,
                y: inner.y + y_offset,
                width: inner.width,
                height: 1,
            },
            buf,
        );
        y_offset += 1;

        // Render input box with border
        // Input box border
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.state.error.is_some() {
                accent_error()
            } else {
                text_muted()
            }));

        // We need 3 lines for bordered input
        let bordered_input_area = Rect {
            x: inner.x,
            y: inner.y + y_offset,
            width: inner.width,
            height: 3,
        };

        let input_inner = input_block.inner(bordered_input_area);
        input_block.render(bordered_input_area, buf);

        // Render input text with cursor
        let display_text = if self.state.input.is_empty() {
            format!("/path/to/{}", self.state.tool.binary_name())
        } else {
            self.state.input.clone()
        };

        let input_style = if self.state.input.is_empty() {
            Style::default().fg(text_muted())
        } else {
            Style::default().fg(text_primary())
        };

        Paragraph::new(display_text.as_str())
            .style(input_style)
            .render(input_inner, buf);

        // Draw cursor
        if !self.state.input.is_empty() || self.state.cursor == 0 {
            let cursor_x = input_inner.x + self.state.cursor as u16;
            if cursor_x < input_inner.x + input_inner.width {
                buf.set_style(
                    Rect {
                        x: cursor_x,
                        y: input_inner.y,
                        width: 1,
                        height: 1,
                    },
                    Style::default().add_modifier(Modifier::REVERSED),
                );
            }
        }

        y_offset += 3;

        // Render status line (error or success)
        let status_area = Rect {
            x: inner.x,
            y: inner.y + y_offset,
            width: inner.width,
            height: 1,
        };

        if let Some(ref error) = self.state.error {
            StatusLine::new().error(error).render(status_area, buf);
        }

        // Render instruction bar at bottom
        let instructions_y = inner.y + inner.height.saturating_sub(1);
        let instructions = if self.state.is_required {
            InstructionBar::new(vec![("Enter", "Validate path"), ("Esc/q", "Quit")])
        } else {
            InstructionBar::new(vec![
                ("Enter", "Validate path"),
                ("Esc", "Skip"),
                ("q", "Quit"),
            ])
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

/// A simplified startup dialog that can run before the main app
/// This is used for blocking checks (git, at least one agent)
pub struct StartupToolDialog {
    state: MissingToolDialogState,
}

impl StartupToolDialog {
    pub fn new(tool: Tool) -> Self {
        let mut state = MissingToolDialogState::default();
        state.show(tool);
        Self { state }
    }

    pub fn state(&self) -> &MissingToolDialogState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut MissingToolDialogState {
        &mut self.state
    }
}
