//! Actions that can be triggered by keybindings
//!
//! This module defines all the actions that can be bound to keys.
//! Each action represents a single, atomic operation in the UI.

use serde::{Deserialize, Serialize};

/// All mappable UI actions
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    // ========== Global Actions ==========
    /// Quit the application
    Quit,
    /// Toggle sidebar visibility
    ToggleSidebar,
    /// Open new project dialog
    NewProject,
    /// Open/create pull request
    OpenPr,
    /// Interrupt current agent processing
    InterruptAgent,
    /// Toggle between Chat and RawEvents view
    ToggleViewMode,
    /// Show model selector dialog
    ShowModelSelector,
    /// Toggle performance metrics display
    ToggleMetrics,
    /// Dump debug state to file
    DumpDebugState,

    // ========== Tab Management ==========
    /// Close current tab
    CloseTab,
    /// Switch to next tab
    NextTab,
    /// Switch to previous tab
    PrevTab,
    /// Switch to tab by number (1-9)
    SwitchToTab(u8),

    // ========== Chat Scrolling ==========
    /// Scroll chat up by N lines
    ScrollUp(u16),
    /// Scroll chat down by N lines
    ScrollDown(u16),
    /// Scroll chat up by a page
    ScrollPageUp,
    /// Scroll chat down by a page
    ScrollPageDown,
    /// Scroll to top of chat
    ScrollToTop,
    /// Scroll to bottom of chat
    ScrollToBottom,

    // ========== Input Box Editing ==========
    /// Insert a newline (for multi-line input)
    InsertNewline,
    /// Delete character before cursor
    Backspace,
    /// Delete character at cursor
    Delete,
    /// Delete word before cursor
    DeleteWordBack,
    /// Delete word after cursor
    DeleteWordForward,
    /// Delete from cursor to start of line
    DeleteToStart,
    /// Delete from cursor to end of line
    DeleteToEnd,
    /// Move cursor left one character
    MoveCursorLeft,
    /// Move cursor right one character
    MoveCursorRight,
    /// Move cursor to start of line
    MoveCursorStart,
    /// Move cursor to end of line
    MoveCursorEnd,
    /// Move cursor left one word
    MoveWordLeft,
    /// Move cursor right one word
    MoveWordRight,
    /// Move cursor up one line (multi-line input)
    MoveCursorUp,
    /// Move cursor down one line (multi-line input)
    MoveCursorDown,
    /// Navigate to previous command in history
    HistoryPrev,
    /// Navigate to next command in history
    HistoryNext,
    /// Submit the current input
    Submit,

    // ========== List/Tree Navigation ==========
    /// Select next item in list
    SelectNext,
    /// Select previous item in list
    SelectPrev,
    /// Move selection down by a page
    SelectPageDown,
    /// Move selection up by a page
    SelectPageUp,
    /// Confirm current selection
    Confirm,
    /// Cancel current dialog/mode
    Cancel,
    /// Expand node or select (for tree views)
    ExpandOrSelect,
    /// Collapse current node (for tree views)
    Collapse,
    /// Add a repository
    AddRepository,
    /// Open settings dialog
    OpenSettings,
    /// Archive workspace or remove project
    ArchiveOrRemove,

    // ========== Sidebar Navigation ==========
    /// Focus sidebar and enter sidebar mode
    EnterSidebarMode,
    /// Leave sidebar mode and return to normal
    ExitSidebarMode,

    // ========== Raw Events View ==========
    /// Select next event in raw events view
    RawEventsSelectNext,
    /// Select previous event in raw events view
    RawEventsSelectPrev,
    /// Toggle expand for selected event
    RawEventsToggleExpand,
    /// Collapse expanded event
    RawEventsCollapse,

    // ========== Event Detail Panel ==========
    /// Toggle event detail panel visibility
    EventDetailToggle,
    /// Scroll up in event detail panel
    EventDetailScrollUp,
    /// Scroll down in event detail panel
    EventDetailScrollDown,
    /// Page up in event detail panel
    EventDetailPageUp,
    /// Page down in event detail panel
    EventDetailPageDown,
    /// Jump to top of event detail panel
    EventDetailScrollToTop,
    /// Jump to bottom of event detail panel
    EventDetailScrollToBottom,
    /// Copy selected event JSON to clipboard
    EventDetailCopy,

    // ========== Confirmation Dialog ==========
    /// Confirm yes in dialog
    ConfirmYes,
    /// Confirm no in dialog
    ConfirmNo,
    /// Toggle selection in dialog
    ConfirmToggle,
    /// Toggle details visibility in error dialog
    ToggleDetails,

    // ========== Agent Selection ==========
    /// Confirm agent selection
    SelectAgent,
    /// Toggle between Build and Plan mode (Claude only)
    ToggleAgentMode,

    // ========== Session Import ==========
    /// Open session import picker
    OpenSessionImport,
    /// Import the selected session
    ImportSession,
    /// Cycle session import agent filter
    CycleImportFilter,

    // ========== Command Mode ==========
    /// Show help dialog
    ShowHelp,
    /// Execute command in command mode
    ExecuteCommand,
    /// Autocomplete command in command mode
    CompleteCommand,

    // ========== Command Palette ==========
    /// Open command palette
    OpenCommandPalette,
}

impl Action {
    /// Get a human-readable description of the action
    pub fn description(&self) -> &'static str {
        match self {
            // Global
            Action::Quit => "Quit application",
            Action::ToggleSidebar => "Toggle sidebar",
            Action::NewProject => "New project",
            Action::OpenPr => "Open/create PR",
            Action::InterruptAgent => "Interrupt agent",
            Action::ToggleViewMode => "Toggle view mode",
            Action::ShowModelSelector => "Select model",
            Action::ToggleMetrics => "Toggle metrics",
            Action::DumpDebugState => "Dump debug state",

            // Tab management
            Action::CloseTab => "Close tab",
            Action::NextTab => "Next tab",
            Action::PrevTab => "Previous tab",
            Action::SwitchToTab(_) => "Switch to tab",

            // Scrolling
            Action::ScrollUp(_) => "Scroll up",
            Action::ScrollDown(_) => "Scroll down",
            Action::ScrollPageUp => "Page up",
            Action::ScrollPageDown => "Page down",
            Action::ScrollToTop => "Scroll to top",
            Action::ScrollToBottom => "Scroll to bottom",

            // Input editing
            Action::InsertNewline => "Insert newline",
            Action::Backspace => "Backspace",
            Action::Delete => "Delete",
            Action::DeleteWordBack => "Delete word back",
            Action::DeleteWordForward => "Delete word forward",
            Action::DeleteToStart => "Delete to start",
            Action::DeleteToEnd => "Delete to end",
            Action::MoveCursorLeft => "Move left",
            Action::MoveCursorRight => "Move right",
            Action::MoveCursorStart => "Move to start",
            Action::MoveCursorEnd => "Move to end",
            Action::MoveWordLeft => "Move word left",
            Action::MoveWordRight => "Move word right",
            Action::MoveCursorUp => "Move up",
            Action::MoveCursorDown => "Move down",
            Action::HistoryPrev => "Previous in history",
            Action::HistoryNext => "Next in history",
            Action::Submit => "Submit",

            // List/Tree navigation
            Action::SelectNext => "Select next",
            Action::SelectPrev => "Select previous",
            Action::SelectPageDown => "Page down",
            Action::SelectPageUp => "Page up",
            Action::Confirm => "Confirm",
            Action::Cancel => "Cancel",
            Action::ExpandOrSelect => "Expand/select",
            Action::Collapse => "Collapse",
            Action::AddRepository => "Add repository",
            Action::OpenSettings => "Open settings",
            Action::ArchiveOrRemove => "Archive/remove",

            // Sidebar
            Action::EnterSidebarMode => "Enter sidebar",
            Action::ExitSidebarMode => "Exit sidebar",

            // Raw events
            Action::RawEventsSelectNext => "Select next event",
            Action::RawEventsSelectPrev => "Select previous event",
            Action::RawEventsToggleExpand => "Toggle expand",
            Action::RawEventsCollapse => "Collapse event",

            // Event detail panel
            Action::EventDetailToggle => "Toggle detail panel",
            Action::EventDetailScrollUp => "Scroll panel up",
            Action::EventDetailScrollDown => "Scroll panel down",
            Action::EventDetailPageUp => "Page panel up",
            Action::EventDetailPageDown => "Page panel down",
            Action::EventDetailScrollToTop => "Panel to top",
            Action::EventDetailScrollToBottom => "Panel to bottom",
            Action::EventDetailCopy => "Copy event JSON",

            // Confirmation
            Action::ConfirmYes => "Yes",
            Action::ConfirmNo => "No",
            Action::ConfirmToggle => "Toggle selection",
            Action::ToggleDetails => "Toggle details",

            // Agent
            Action::SelectAgent => "Select agent",
            Action::ToggleAgentMode => "Toggle Build/Plan mode",

            // Session import
            Action::OpenSessionImport => "Import session",
            Action::ImportSession => "Import selected",
            Action::CycleImportFilter => "Cycle filter",

            // Command mode
            Action::ShowHelp => "Show help",
            Action::ExecuteCommand => "Execute command",
            Action::CompleteCommand => "Autocomplete command",

            // Command palette
            Action::OpenCommandPalette => "Command palette",
        }
    }

    /// Returns true if this action opens a dialog/modal
    pub fn opens_dialog(&self) -> bool {
        matches!(
            self,
            Action::NewProject
                | Action::ShowModelSelector
                | Action::OpenSessionImport
                | Action::ShowHelp
                | Action::AddRepository
                | Action::OpenSettings
                | Action::OpenCommandPalette
        )
    }

    /// Returns true if this action should appear in the command palette
    pub fn show_in_palette(&self) -> bool {
        matches!(
            self,
            // Global
            Action::Quit
                | Action::ToggleSidebar
                | Action::NewProject
                | Action::OpenPr
                | Action::InterruptAgent
                | Action::ToggleViewMode
                | Action::ShowModelSelector
                | Action::ToggleMetrics
                | Action::DumpDebugState
                // Tab management
                | Action::CloseTab
                | Action::NextTab
                | Action::PrevTab
                // Scrolling (page-level)
                | Action::ScrollPageUp
                | Action::ScrollPageDown
                | Action::ScrollToTop
                | Action::ScrollToBottom
                // Sidebar
                | Action::EnterSidebarMode
                | Action::AddRepository
                | Action::OpenSettings
                | Action::ArchiveOrRemove
                // Agent/Session
                | Action::ToggleAgentMode
                | Action::OpenSessionImport
                | Action::ShowHelp
        )
    }

    /// Get palette description (with "..." suffix for dialogs)
    pub fn palette_description(&self) -> String {
        let desc = self.description();
        if self.opens_dialog() {
            format!("{}...", desc)
        } else {
            desc.to_string()
        }
    }
}
