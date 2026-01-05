//! Shared path input state for dialogs.

use std::path::PathBuf;

use super::TextInputState;

/// Shared state for dialogs that capture a filesystem path.
#[derive(Debug, Clone)]
pub struct PathInputState {
    /// Text input state
    pub text: TextInputState,
    /// Whether the dialog is visible
    pub visible: bool,
    /// Validation error message
    pub error: Option<String>,
    /// Whether the path is valid
    pub is_valid: bool,
}

impl Default for PathInputState {
    fn default() -> Self {
        Self::new()
    }
}

impl PathInputState {
    pub fn new() -> Self {
        Self {
            text: TextInputState::new(),
            visible: false,
            error: None,
            is_valid: false,
        }
    }

    /// Show the dialog and reset validation state.
    pub fn show(&mut self) {
        self.visible = true;
        self.clear_validation();
    }

    /// Hide the dialog.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Get the current input value.
    pub fn input(&self) -> &str {
        self.text.value()
    }

    /// Check if dialog is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Clear validation state.
    pub fn clear_validation(&mut self) {
        self.error = None;
        self.is_valid = false;
    }

    /// Mark as valid and clear errors.
    pub fn set_valid(&mut self) {
        self.error = None;
        self.is_valid = true;
    }

    /// Mark as invalid with a specific error.
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.error = Some(msg.into());
        self.is_valid = false;
    }

    /// Mark as invalid without a specific error.
    pub fn set_invalid(&mut self) {
        self.error = None;
        self.is_valid = false;
    }

    /// Get the expanded path (handles ~).
    pub fn expanded_path(&self) -> PathBuf {
        let input = self.text.value();
        if input.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                return home.join(input[1..].trim_start_matches('/'));
            }
        }
        PathBuf::from(input)
    }

    // Delegated text input methods.
    pub fn insert_char(&mut self, c: char) {
        self.text.insert_char(c);
    }

    pub fn delete_char(&mut self) {
        self.text.delete_char();
    }

    pub fn delete_forward(&mut self) {
        self.text.delete_forward();
    }

    pub fn move_left(&mut self) {
        self.text.move_left();
    }

    pub fn move_right(&mut self) {
        self.text.move_right();
    }

    pub fn move_start(&mut self) {
        self.text.move_start();
    }

    pub fn move_end(&mut self) {
        self.text.move_end();
    }

    pub fn delete_to_start(&mut self) {
        self.text.delete_to_start();
    }

    pub fn delete_to_end(&mut self) {
        self.text.delete_to_end();
    }
}
