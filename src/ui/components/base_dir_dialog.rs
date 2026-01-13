//! Set base projects directory dialog component

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    widgets::{Block, Borders, Paragraph, Widget},
};
use std::path::PathBuf;

use super::{DialogFrame, InstructionBar, PathInputState, StatusLine};

/// State for the base directory dialog
#[derive(Debug, Clone)]
pub struct BaseDirDialogState {
    /// Shared path input state
    pub path: PathInputState,
}

impl Default for BaseDirDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseDirDialogState {
    pub fn new() -> Self {
        Self {
            path: PathInputState::new(),
        }
    }

    /// Show the dialog with optional initial value
    pub fn show(&mut self) {
        self.path.show();
        // Default to ~/code if empty
        if self.path.text.is_empty() {
            self.path.text.set("~/code");
        }
        self.validate();
    }

    /// Show with a specific initial path
    pub fn show_with_path(&mut self, path: &str) {
        self.path.show();
        self.path.text.set(path);
        self.validate();
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.path.hide();
    }

    /// Get the current input value
    pub fn input(&self) -> &str {
        self.path.input()
    }

    // Delegate text input methods
    pub fn insert_char(&mut self, c: char) {
        self.path.insert_char(c);
        self.validate();
    }

    pub fn delete_char(&mut self) {
        self.path.delete_char();
        self.validate();
    }

    pub fn delete_forward(&mut self) {
        self.path.delete_forward();
        self.validate();
    }

    pub fn move_left(&mut self) {
        self.path.move_left();
    }

    pub fn move_right(&mut self) {
        self.path.move_right();
    }

    pub fn move_start(&mut self) {
        self.path.move_start();
    }

    pub fn move_end(&mut self) {
        self.path.move_end();
    }

    pub fn delete_to_start(&mut self) {
        self.path.delete_to_start();
        self.validate();
    }

    pub fn delete_to_end(&mut self) {
        self.path.delete_to_end();
        self.validate();
    }

    /// Validate the current input path
    pub fn validate(&mut self) {
        let input = self.path.input();

        // Check if path is empty
        if input.is_empty() {
            self.path.set_error("Path cannot be empty");
            return;
        }

        let expanded_path = self.path.expanded_path();

        // Check if path exists
        if !expanded_path.exists() {
            self.path.set_error("Directory does not exist");
            return;
        }

        // Check if it's a directory
        if !expanded_path.is_dir() {
            self.path.set_error("Path is not a directory");
            return;
        }

        self.path.set_valid();
    }

    /// Get the expanded path
    pub fn expanded_path(&self) -> PathBuf {
        self.path.expanded_path()
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.path.is_visible()
    }

    /// Validation error message
    pub fn error(&self) -> Option<&str> {
        self.path.error.as_deref()
    }

    /// Whether the path is valid
    pub fn is_valid(&self) -> bool {
        self.path.is_valid
    }
}

/// Base directory dialog widget
pub struct BaseDirDialog;

impl BaseDirDialog {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &BaseDirDialogState) {
        if !state.is_visible() {
            return;
        }

        // Render dialog frame
        let frame = DialogFrame::new("Set Projects Directory", 56, 13);
        let inner = frame.render(area, buf);

        // Layout inside dialog
        let chunks = Layout::vertical([
            Constraint::Length(1), // Label
            Constraint::Length(1), // Spacing
            Constraint::Length(3), // Input field
            Constraint::Length(1), // Status/error
            Constraint::Length(1), // Help text
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        // Render label
        let label = Paragraph::new("Where do you keep your projects?")
            .style(Style::default().fg(Color::White));
        label.render(chunks[0], buf);

        // Render input field with border
        let input_style = if state.is_valid() {
            Style::default().fg(Color::Green)
        } else if state.error().is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::White)
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(input_style);

        let input_inner = input_block.inner(chunks[2]);
        input_block.render(chunks[2], buf);

        // Render text input with cursor
        state
            .path
            .text
            .render(input_inner, buf, Style::default().fg(Color::White));

        // Render status/error
        let status = StatusLine::from_result(state.error(), state.is_valid(), "Directory found");
        status.render(chunks[3], buf);

        // Help text
        let help = Paragraph::new("This directory will be scanned for git projects.")
            .style(Style::default().fg(Color::DarkGray));
        help.render(chunks[4], buf);

        // Render instructions
        let instructions = InstructionBar::new(vec![("Enter", "confirm"), ("Esc", "cancel")]);
        instructions.render(chunks[6], buf);
    }
}

impl Default for BaseDirDialog {
    fn default() -> Self {
        Self::new()
    }
}
