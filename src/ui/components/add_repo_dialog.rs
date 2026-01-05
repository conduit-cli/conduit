//! Add repository dialog component

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    widgets::{Block, Borders, Paragraph, Widget},
};
use std::path::PathBuf;

use super::{DialogFrame, InstructionBar, PathInputState, StatusLine};

/// State for the add repository dialog
#[derive(Debug, Clone)]
pub struct AddRepoDialogState {
    /// Shared path input state (includes visibility and validation)
    pub path: PathInputState,
    /// Extracted repository name
    pub repo_name: Option<String>,
}

impl Default for AddRepoDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl AddRepoDialogState {
    pub fn new() -> Self {
        Self {
            path: PathInputState::new(),
            repo_name: None,
        }
    }

    /// Show the dialog
    pub fn show(&mut self) {
        self.path.show();
        self.path.text.clear();
        self.repo_name = None;
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.path.hide();
    }

    /// Get the current input value
    pub fn input(&self) -> &str {
        self.path.input()
    }

    // Delegate text input methods with validation
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

    /// Validate the current input path
    pub fn validate(&mut self) {
        let input = self.path.input();

        // Check if path is empty
        if input.is_empty() {
            self.path.set_invalid();
            self.repo_name = None;
            return;
        }

        // Expand ~ to home directory
        let expanded_path = self.path.expanded_path();

        // Check if path exists
        if !expanded_path.exists() {
            self.path.set_error("Path does not exist");
            self.repo_name = None;
            return;
        }

        // Check if it's a directory
        if !expanded_path.is_dir() {
            self.path.set_error("Path is not a directory");
            self.repo_name = None;
            return;
        }

        // Check for .git directory
        let git_dir = expanded_path.join(".git");
        if !git_dir.exists() {
            self.path
                .set_error("Not a git repository (no .git directory)");
            self.repo_name = None;
            return;
        }

        // Extract repository name from path
        self.repo_name = expanded_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());

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

/// Add repository dialog widget
pub struct AddRepoDialog;

impl AddRepoDialog {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &AddRepoDialogState) {
        if !state.is_visible() {
            return;
        }

        // Render dialog frame
        let frame = DialogFrame::new("Add Custom Project", 60, 11);
        let inner = frame.render(area, buf);

        // Layout inside dialog
        let chunks = Layout::vertical([
            Constraint::Length(1), // Label
            Constraint::Length(1), // Spacing
            Constraint::Length(3), // Input field (with border)
            Constraint::Length(1), // Status/error
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        // Render label
        let label = Paragraph::new("Enter local repository path:")
            .style(Style::default().fg(Color::White));
        label.render(chunks[0], buf);

        // Render input field
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

        // Render input text with cursor and placeholder
        state.path.text.render_with_placeholder(
            input_inner,
            buf,
            Style::default().fg(Color::White),
            "~/path/to/repo",
            Style::default().fg(Color::DarkGray),
        );

        // Render status/error using StatusLine component
        let success_msg = format!(
            "Valid repository: {}",
            state.repo_name.as_deref().unwrap_or("repository")
        );
        let status = StatusLine::from_result(state.error(), state.is_valid(), &success_msg);
        status.render(chunks[3], buf);

        // Render instructions
        let instructions = InstructionBar::new(vec![("Enter", "add"), ("Esc", "cancel")]);
        instructions.render(chunks[5], buf);
    }
}

impl Default for AddRepoDialog {
    fn default() -> Self {
        Self::new()
    }
}
