//! Reusable confirmation dialog component

use std::path::PathBuf;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use uuid::Uuid;

use super::dialog::{DialogFrame, InstructionBar};
use crate::git::PrPreflightResult;

/// Confirmation type determines the dialog's appearance and urgency level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmationType {
    /// Informational - safe action (cyan border)
    #[default]
    Info,
    /// Warning - potentially risky action (yellow border)
    Warning,
    /// Danger - destructive or risky action (red border)
    Danger,
}

/// Context for what action the confirmation dialog is for
#[derive(Debug, Clone)]
pub enum ConfirmationContext {
    /// Archiving a single workspace
    ArchiveWorkspace(Uuid),
    /// Removing a project (archives all workspaces and deletes repository)
    RemoveProject(Uuid),
    /// Creating a pull request
    CreatePullRequest {
        tab_index: usize,
        working_dir: PathBuf,
        preflight: PrPreflightResult,
    },
    /// Opening an existing PR in browser
    OpenExistingPr {
        working_dir: PathBuf,
        pr_url: String,
    },
    /// Steering fallback confirmation (interrupt current run)
    SteerFallback { message_id: Uuid },
    /// Forking the current session
    ForkSession {
        parent_workspace_id: Uuid,
        /// The branch to fork from (computed at dialog show time for consistency)
        base_branch: String,
    },
}

impl ConfirmationType {
    /// Get the border color for this confirmation type
    pub fn border_color(&self) -> Color {
        match self {
            ConfirmationType::Info => Color::Cyan,
            ConfirmationType::Warning => Color::Yellow,
            ConfirmationType::Danger => Color::Red,
        }
    }

    /// Get the warning icon color
    pub fn warning_color(&self) -> Color {
        match self {
            ConfirmationType::Info => Color::Cyan,
            ConfirmationType::Warning => Color::Yellow,
            ConfirmationType::Danger => Color::Red,
        }
    }
}

/// State for the confirmation dialog
#[derive(Debug, Clone, Default)]
pub struct ConfirmationDialogState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// Whether the dialog is in loading state (showing spinner)
    pub loading: bool,
    /// Loading message to display
    pub loading_message: String,
    /// Current spinner frame for loading animation
    pub spinner_frame: usize,
    /// Dialog title
    pub title: String,
    /// Main message to display
    pub message: String,
    /// List of warning messages to display
    pub warnings: Vec<String>,
    /// Confirmation type (affects appearance)
    pub confirmation_type: ConfirmationType,
    /// Text for the confirm button
    pub confirm_text: String,
    /// Text for the cancel button
    pub cancel_text: String,
    /// Currently selected button (0 = Cancel, 1 = Confirm)
    pub selected: usize,
    /// Context for the action being confirmed
    pub context: Option<ConfirmationContext>,
}

impl ConfirmationDialogState {
    /// Create a new confirmation dialog state
    pub fn new() -> Self {
        Self {
            visible: false,
            loading: false,
            loading_message: String::new(),
            spinner_frame: 0,
            title: String::new(),
            message: String::new(),
            warnings: Vec::new(),
            confirmation_type: ConfirmationType::Info,
            confirm_text: "Confirm".to_string(),
            cancel_text: "Cancel".to_string(),
            selected: 0, // Default to Cancel for safety
            context: None,
        }
    }

    /// Show the dialog in loading state with a spinner
    pub fn show_loading(&mut self, title: impl Into<String>, loading_message: impl Into<String>) {
        self.visible = true;
        self.loading = true;
        self.loading_message = loading_message.into();
        self.spinner_frame = 0;
        self.title = title.into();
        self.message = String::new();
        self.warnings = Vec::new();
        self.confirmation_type = ConfirmationType::Info;
        self.context = None;
    }

    /// Advance the spinner animation
    pub fn tick(&mut self) {
        if self.loading {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
        }
    }

    /// Show the dialog with the given configuration
    pub fn show(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        warnings: Vec<String>,
        confirmation_type: ConfirmationType,
        confirm_text: impl Into<String>,
        context: Option<ConfirmationContext>,
    ) {
        self.visible = true;
        self.loading = false; // Clear loading state
        self.title = title.into();
        self.message = message.into();
        self.warnings = warnings;
        self.confirmation_type = confirmation_type;
        self.confirm_text = confirm_text.into();
        self.selected = 0; // Default to Cancel for safety
        self.context = context;
    }

    /// Hide the dialog and reset state
    pub fn hide(&mut self) {
        self.visible = false;
        self.loading = false;
        self.context = None;
    }

    /// Toggle selection between Cancel and Confirm
    pub fn toggle_selection(&mut self) {
        self.selected = if self.selected == 0 { 1 } else { 0 };
    }

    /// Select Cancel
    pub fn select_cancel(&mut self) {
        self.selected = 0;
    }

    /// Select Confirm
    pub fn select_confirm(&mut self) {
        self.selected = 1;
    }

    /// Check if Confirm is selected
    pub fn is_confirm_selected(&self) -> bool {
        self.selected == 1
    }

    /// Check if Cancel is selected
    pub fn is_cancel_selected(&self) -> bool {
        self.selected == 0
    }
}

/// Confirmation dialog widget
pub struct ConfirmationDialog<'a> {
    state: &'a ConfirmationDialogState,
}

impl<'a> ConfirmationDialog<'a> {
    pub fn new(state: &'a ConfirmationDialogState) -> Self {
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

    /// Calculate the required dialog height based on content
    fn calculate_height(&self, dialog_width: u16) -> u16 {
        // Layout:
        // - Top border (1)
        // - Empty line (1)
        // - Message lines
        // - Empty line after message (1)
        // - Warnings (if any)
        // - Empty line before buttons (1)
        // - Buttons (1)
        // - Empty line (1)
        // - Instructions (1)
        // - Bottom border (1)
        let message_lines = self.calculate_message_lines(dialog_width);
        let warnings_height = if self.state.warnings.is_empty() {
            0
        } else {
            self.state.warnings.len() as u16 + 1 // warnings + spacing before them
        };
        let base_height: u16 = 12; // borders + padding + buttons + instructions
        base_height + message_lines + warnings_height
    }
}

/// Spinner animation frames
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Widget for ConfirmationDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        // Loading state - show compact dialog with spinner
        if self.state.loading {
            let dialog_width: u16 = 50;
            let dialog_height: u16 = 9; // Compact: border + padding + spinner + padding + instructions + border

            // Render dialog frame
            let frame = DialogFrame::new(&self.state.title, dialog_width, dialog_height)
                .border_color(Color::Cyan);
            let inner = frame.render(area, buf);

            if inner.height < 3 {
                return;
            }

            // Render spinner and loading message centered
            let spinner_char = SPINNER_FRAMES[self.state.spinner_frame % SPINNER_FRAMES.len()];
            let loading_line = Line::from(vec![
                Span::styled(
                    format!("{} ", spinner_char),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    self.state.loading_message.as_str(),
                    Style::default().fg(Color::White),
                ),
            ]);

            let loading_para = Paragraph::new(loading_line).alignment(Alignment::Center);
            loading_para.render(
                Rect {
                    x: inner.x,
                    y: inner.y + inner.height / 2,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );

            // Render instruction bar at bottom (only Esc to cancel)
            let instructions_y = inner.y + inner.height.saturating_sub(1);
            let instructions = InstructionBar::new(vec![("Esc", "Cancel")]);
            instructions.render(
                Rect {
                    x: inner.x,
                    y: instructions_y,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );

            return;
        }

        // Normal confirmation dialog
        let dialog_width: u16 = 50;
        let dialog_height = self.calculate_height(dialog_width);

        // Render dialog frame
        let frame = DialogFrame::new(&self.state.title, dialog_width, dialog_height)
            .border_color(self.state.confirmation_type.border_color());
        let inner = frame.render(area, buf);

        if inner.height < 6 {
            return;
        }

        // Start with top padding
        let mut y_offset: u16 = 1;

        // Render message with wrapping
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

        // Render warnings
        if !self.state.warnings.is_empty() {
            // Spacing before warnings already included in y_offset

            let warning_color = self.state.confirmation_type.warning_color();
            for warning in &self.state.warnings {
                if inner.y + y_offset >= inner.y + inner.height.saturating_sub(2) {
                    break;
                }

                let warning_line = Line::from(vec![
                    Span::styled("  ⚠ ", Style::default().fg(warning_color)),
                    Span::styled(warning.as_str(), Style::default().fg(warning_color)),
                ]);
                let warning_para = Paragraph::new(warning_line);
                warning_para.render(
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
        }

        // Render buttons at bottom (with spacing before instructions)
        let buttons_y = inner.y + inner.height.saturating_sub(4);
        if buttons_y >= inner.y + y_offset {
            let cancel_style = if self.state.is_cancel_selected() {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let confirm_style = if self.state.is_confirm_selected() {
                Style::default()
                    .fg(Color::Black)
                    .bg(self.state.confirmation_type.border_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.state.confirmation_type.border_color())
            };

            let buttons_line = Line::from(vec![
                Span::styled(format!(" {} ", self.state.cancel_text), cancel_style),
                Span::raw("    "),
                Span::styled(format!(" {} ", self.state.confirm_text), confirm_style),
            ]);

            let buttons = Paragraph::new(buttons_line).alignment(Alignment::Center);
            buttons.render(
                Rect {
                    x: inner.x,
                    y: buttons_y,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
        }

        // Render instruction bar at bottom
        let instructions_y = inner.y + inner.height.saturating_sub(1);
        let instructions = InstructionBar::new(vec![
            ("←/→", "Select"),
            ("Enter", "Confirm"),
            ("Esc", "Cancel"),
            ("y/n", "Quick"),
        ]);
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
