//! Default keybindings
//!
//! This module defines the default keybindings that are used
//! when no user configuration is present.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};

use super::keys::{KeyCombo, KeyContext, KeybindingConfig};
use crate::ui::action::Action;

/// Helper to insert a keybinding
fn bind(map: &mut HashMap<KeyCombo, Action>, key: &str, action: Action) {
    if let Ok(combo) = key.parse() {
        map.insert(combo, action);
    }
}

/// Create the default keybindings configuration
pub fn default_keybindings() -> KeybindingConfig {
    let mut config = KeybindingConfig::new();

    // ========== Global Keybindings ==========
    // These work in all modes unless overridden

    bind(&mut config.global, "C-q", Action::Quit);
    bind(&mut config.global, "C-t", Action::ToggleSidebar);
    bind(&mut config.global, "C-n", Action::NewProject);
    bind(&mut config.global, "C-p", Action::OpenCommandPalette);
    bind(&mut config.global, "C-M-p", Action::OpenPr); // Ctrl+Alt+P for PR
    bind(&mut config.global, "M-S-f", Action::ForkSession); // Alt+Shift+F for fork
    bind(&mut config.global, "C-z", Action::Suspend);

    // Note: Ctrl+C is handled specially in app.rs for double-press detection
    bind(&mut config.global, "C-g", Action::ToggleViewMode);
    bind(&mut config.global, "C-o", Action::ShowModelSelector);
    bind(&mut config.global, "M-i", Action::OpenSessionImport);

    // Readline shortcuts (work globally in input modes)
    bind(&mut config.global, "C-a", Action::MoveCursorStart);
    bind(&mut config.global, "C-e", Action::MoveCursorEnd);
    bind(&mut config.global, "C-f", Action::MoveCursorRight);
    bind(&mut config.global, "C-b", Action::MoveCursorLeft);
    bind(&mut config.global, "C-u", Action::DeleteToStart);
    bind(&mut config.global, "C-k", Action::DeleteToEnd);
    bind(&mut config.global, "C-w", Action::DeleteWordBack);
    bind(&mut config.global, "C-d", Action::Delete);
    bind(&mut config.global, "C-h", Action::Backspace);
    bind(&mut config.global, "C-j", Action::InsertNewline);

    // Close tab with Alt+Shift+W (Ctrl+Shift doesn't work reliably in terminals)
    bind(&mut config.global, "M-S-w", Action::CloseTab);

    // Tab/workspace switching with Alt+Tab and Alt+Shift+Tab
    config.global.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::ALT),
        Action::NextTab,
    );
    config.global.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::ALT),
        Action::PrevTab,
    );
    config.global.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::ALT | KeyModifiers::SHIFT),
        Action::PrevTab,
    );

    // Alt key shortcuts
    bind(&mut config.global, "M-b", Action::MoveWordLeft);
    bind(&mut config.global, "M-f", Action::MoveWordRight);
    bind(&mut config.global, "M-d", Action::DeleteWordForward);
    bind(&mut config.global, "M-<BS>", Action::DeleteWordBack);
    bind(&mut config.global, "M-p", Action::ToggleMetrics);
    bind(&mut config.global, "M-g", Action::DumpDebugState);
    bind(&mut config.global, "M-t", Action::ShowThemePicker);
    bind(&mut config.global, "M-n", Action::NewWorkspaceUnderCursor);
    bind(&mut config.global, "M-q", Action::OpenQueueEditor);
    bind(&mut config.global, "M-e", Action::EditPromptExternal);

    // Agent mode toggle (Build/Plan) - Ctrl+\
    //
    // Terminal compatibility notes:
    // - Most terminals send Ctrl+\ as Ctrl+4 (ASCII 0x1C, the file separator character)
    // - Ctrl+Backspace is often sent as Ctrl+H (same as regular backspace)
    // - Use `conduit debug-keys` to verify how your terminal reports key combinations
    // - Tested terminals: iTerm2 (macOS), Terminal.app (macOS), Ghostty (macOS)
    // - Users can override this binding in their config if their terminal differs
    bind(&mut config.global, "C-4", Action::ToggleAgentMode);

    // Alt+Shift for scrolling (M-S-j = Alt+Shift+J)
    bind(&mut config.global, "M-S-j", Action::ScrollDown(1));
    bind(&mut config.global, "M-S-k", Action::ScrollUp(1));
    bind(&mut config.global, "M-S-d", Action::ScrollPageDown);
    bind(&mut config.global, "M-S-b", Action::ScrollPageUp);

    // Copy workspace path to clipboard
    bind(&mut config.global, "M-S-c", Action::CopyWorkspacePath);
    // Copy selection to clipboard
    bind(&mut config.global, "M-c", Action::CopySelection);

    // Ctrl+Arrow for scrolling
    config.global.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::CONTROL),
        Action::ScrollUp(1),
    );
    config.global.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::CONTROL),
        Action::ScrollDown(1),
    );

    // Alt+1-9 for tab switching
    for i in 1..=9u8 {
        let key = format!("M-{}", i);
        bind(&mut config.global, &key, Action::SwitchToTab(i));
    }

    // ========== Chat Mode (Normal InputMode) ==========
    let chat = config.context.entry(KeyContext::Chat).or_default();

    // Enter submits (or expands in raw events)
    chat.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Submit,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::CONTROL),
        Action::SubmitSteer,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::SHIFT),
        Action::InsertNewline,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::ALT),
        Action::InsertNewline,
    );

    // Navigation
    chat.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Delete, KeyModifiers::NONE),
        Action::Delete,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Left, KeyModifiers::NONE),
        Action::MoveCursorLeft,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Right, KeyModifiers::NONE),
        Action::MoveCursorRight,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Home, KeyModifiers::NONE),
        Action::MoveCursorStart,
    );
    chat.insert(
        KeyCombo::new(KeyCode::End, KeyModifiers::NONE),
        Action::MoveCursorEnd,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::MoveCursorUp,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::MoveCursorDown,
    );
    chat.insert(
        KeyCombo::new(KeyCode::PageUp, KeyModifiers::NONE),
        Action::ScrollPageUp,
    );
    chat.insert(
        KeyCombo::new(KeyCode::PageDown, KeyModifiers::NONE),
        Action::ScrollPageDown,
    );
    chat.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::ScrollToBottom,
    );
    bind(chat, "M-<Up>", Action::ScrollPrevUserMessage);
    bind(chat, "M-<Down>", Action::ScrollNextUserMessage);

    // Tab toggles Plan/Build mode
    chat.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::NONE),
        Action::ToggleAgentMode,
    );
    // BackTab (Shift+Tab) also toggles Plan/Build mode - some terminals send SHIFT modifier, some don't
    chat.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::NONE),
        Action::ToggleAgentMode,
    );
    chat.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        Action::ToggleAgentMode,
    );

    // ========== Scrolling Mode ==========
    let scrolling = config.context.entry(KeyContext::Scrolling).or_default();

    scrolling.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::ScrollUp(1),
    );
    scrolling.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::ScrollDown(1),
    );
    bind(scrolling, "k", Action::ScrollUp(1));
    bind(scrolling, "j", Action::ScrollDown(1));
    scrolling.insert(
        KeyCombo::new(KeyCode::PageUp, KeyModifiers::NONE),
        Action::ScrollPageUp,
    );
    scrolling.insert(
        KeyCombo::new(KeyCode::PageDown, KeyModifiers::NONE),
        Action::ScrollPageDown,
    );
    scrolling.insert(
        KeyCombo::new(KeyCode::Home, KeyModifiers::NONE),
        Action::ScrollToTop,
    );
    scrolling.insert(
        KeyCombo::new(KeyCode::End, KeyModifiers::NONE),
        Action::ScrollToBottom,
    );
    bind(scrolling, "g", Action::ScrollToTop);
    bind(scrolling, "G", Action::ScrollToBottom);
    scrolling.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    bind(scrolling, "q", Action::Cancel);
    bind(scrolling, "i", Action::Cancel);

    // ========== Queue Editing Mode ==========
    let queue = config.context.entry(KeyContext::QueueEditing).or_default();

    queue.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::QueueEdit,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Delete, KeyModifiers::NONE),
        Action::QueueDelete,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::QueueDelete,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::CloseQueueEditor,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Char('k'), KeyModifiers::ALT),
        Action::QueueMoveUp,
    );
    queue.insert(
        KeyCombo::new(KeyCode::Char('j'), KeyModifiers::ALT),
        Action::QueueMoveDown,
    );

    // ========== Sidebar Navigation ==========
    let sidebar = config.context.entry(KeyContext::Sidebar).or_default();

    sidebar.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    sidebar.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    bind(sidebar, "k", Action::SelectPrev);
    bind(sidebar, "j", Action::SelectNext);
    sidebar.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::ExpandOrSelect,
    );
    sidebar.insert(
        KeyCombo::new(KeyCode::Right, KeyModifiers::NONE),
        Action::ExpandOrSelect,
    );
    bind(sidebar, "l", Action::ExpandOrSelect);
    sidebar.insert(
        KeyCombo::new(KeyCode::Left, KeyModifiers::NONE),
        Action::Collapse,
    );
    bind(sidebar, "h", Action::Collapse);
    sidebar.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::ExitSidebarMode,
    );
    // Tab toggles Plan/Build mode (Alt+Tab for workspace switching)
    sidebar.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::NONE),
        Action::ToggleAgentMode,
    );
    // BackTab (Shift+Tab) also toggles Plan/Build mode - some terminals send SHIFT modifier, some don't
    sidebar.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::NONE),
        Action::ToggleAgentMode,
    );
    sidebar.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        Action::ToggleAgentMode,
    );
    bind(sidebar, "r", Action::AddRepository);
    bind(sidebar, "s", Action::OpenSettings);
    bind(sidebar, "x", Action::ArchiveOrRemove);

    // ========== Dialog Context ==========
    let dialog = config.context.entry(KeyContext::Dialog).or_default();

    dialog.insert(
        KeyCombo::new(KeyCode::Left, KeyModifiers::NONE),
        Action::ConfirmToggle,
    );
    dialog.insert(
        KeyCombo::new(KeyCode::Right, KeyModifiers::NONE),
        Action::ConfirmToggle,
    );
    dialog.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::NONE),
        Action::ConfirmToggle,
    );
    // BackTab also toggles (prevents global PrevTab from leaking)
    dialog.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::NONE),
        Action::ConfirmToggle,
    );
    dialog.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        Action::ConfirmToggle,
    );
    dialog.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    dialog.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    bind(dialog, "y", Action::ConfirmYes);
    bind(dialog, "n", Action::ConfirmNo);
    bind(dialog, "d", Action::ToggleDetails);

    // ========== Project Picker ==========
    let picker = config.context.entry(KeyContext::ProjectPicker).or_default();

    picker.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    picker.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    picker.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    bind(picker, "C-j", Action::SelectNext);
    bind(picker, "C-k", Action::SelectPrev);
    bind(picker, "C-f", Action::SelectPageDown);
    bind(picker, "C-b", Action::SelectPageUp);
    bind(picker, "C-a", Action::AddRepository);
    picker.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    picker.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );

    // ========== Model Selector ==========
    let model = config.context.entry(KeyContext::ModelSelector).or_default();

    model.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    model.insert(
        KeyCombo::new(KeyCode::Delete, KeyModifiers::NONE),
        Action::Delete,
    );
    model.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    model.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    model.insert(
        KeyCombo::new(KeyCode::Left, KeyModifiers::NONE),
        Action::MoveCursorLeft,
    );
    model.insert(
        KeyCombo::new(KeyCode::Right, KeyModifiers::NONE),
        Action::MoveCursorRight,
    );
    model.insert(
        KeyCombo::new(KeyCode::Home, KeyModifiers::NONE),
        Action::MoveCursorStart,
    );
    model.insert(
        KeyCombo::new(KeyCode::End, KeyModifiers::NONE),
        Action::MoveCursorEnd,
    );
    bind(model, "k", Action::SelectPrev);
    bind(model, "j", Action::SelectNext);
    bind(model, "C-k", Action::SelectPrev);
    bind(model, "C-j", Action::SelectNext);
    bind(model, "M-d", Action::SetDefaultModel);
    model.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    model.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );

    // ========== Add Repository Dialog ==========
    let add_repo = config.context.entry(KeyContext::AddRepository).or_default();

    add_repo.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::Delete, KeyModifiers::NONE),
        Action::Delete,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::Left, KeyModifiers::NONE),
        Action::MoveCursorLeft,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::Right, KeyModifiers::NONE),
        Action::MoveCursorRight,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::Home, KeyModifiers::NONE),
        Action::MoveCursorStart,
    );
    add_repo.insert(
        KeyCombo::new(KeyCode::End, KeyModifiers::NONE),
        Action::MoveCursorEnd,
    );

    // ========== Base Directory Dialog ==========
    let base_dir = config.context.entry(KeyContext::BaseDir).or_default();

    base_dir.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::Delete, KeyModifiers::NONE),
        Action::Delete,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::Left, KeyModifiers::NONE),
        Action::MoveCursorLeft,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::Right, KeyModifiers::NONE),
        Action::MoveCursorRight,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::Home, KeyModifiers::NONE),
        Action::MoveCursorStart,
    );
    base_dir.insert(
        KeyCombo::new(KeyCode::End, KeyModifiers::NONE),
        Action::MoveCursorEnd,
    );

    // ========== Raw Events View ==========
    let raw = config.context.entry(KeyContext::RawEvents).or_default();

    // Event list navigation
    raw.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::RawEventsSelectPrev,
    );
    raw.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::RawEventsSelectNext,
    );
    bind(raw, "k", Action::RawEventsSelectPrev);
    bind(raw, "j", Action::RawEventsSelectNext);
    bind(raw, "l", Action::RawEventsToggleExpand);
    bind(raw, "h", Action::RawEventsCollapse);
    raw.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::RawEventsToggleExpand,
    );
    raw.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::NONE),
        Action::RawEventsToggleExpand,
    );
    raw.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::RawEventsCollapse,
    );

    // Detail panel toggle
    bind(raw, "e", Action::EventDetailToggle);

    // Detail panel scrolling (Ctrl variants)
    bind(raw, "C-j", Action::EventDetailScrollDown);
    bind(raw, "C-k", Action::EventDetailScrollUp);
    bind(raw, "C-f", Action::EventDetailPageDown);
    bind(raw, "C-b", Action::EventDetailPageUp);
    // Use vim-style g/G for top/bottom (C-g is reserved for ToggleViewMode)
    bind(raw, "g", Action::EventDetailScrollToTop);
    bind(raw, "G", Action::EventDetailScrollToBottom);

    // Copy selected event
    bind(raw, "c", Action::EventDetailCopy);

    // ========== Command Mode ==========
    let command = config.context.entry(KeyContext::Command).or_default();

    command.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::ExecuteCommand,
    );
    command.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    command.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    command.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::NONE),
        Action::CompleteCommand,
    );

    // ========== Help Dialog ==========
    let help = config.context.entry(KeyContext::HelpDialog).or_default();

    help.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    bind(help, "q", Action::Cancel);
    help.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::ScrollUp(1),
    );
    help.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::ScrollDown(1),
    );
    bind(help, "k", Action::ScrollUp(1));
    bind(help, "j", Action::ScrollDown(1));
    help.insert(
        KeyCombo::new(KeyCode::PageUp, KeyModifiers::NONE),
        Action::ScrollPageUp,
    );
    help.insert(
        KeyCombo::new(KeyCode::PageDown, KeyModifiers::NONE),
        Action::ScrollPageDown,
    );
    bind(help, "C-b", Action::ScrollPageUp);
    bind(help, "C-f", Action::ScrollPageDown);
    help.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );

    // ========== Session Import Picker ==========
    let session_import = config.context.entry(KeyContext::SessionImport).or_default();

    session_import.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    session_import.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    bind(session_import, "k", Action::SelectPrev);
    bind(session_import, "j", Action::SelectNext);
    bind(session_import, "C-j", Action::SelectNext);
    bind(session_import, "C-k", Action::SelectPrev);
    bind(session_import, "C-f", Action::SelectPageDown);
    bind(session_import, "C-b", Action::SelectPageUp);
    session_import.insert(
        KeyCombo::new(KeyCode::Tab, KeyModifiers::NONE),
        Action::CycleImportFilter,
    );
    // BackTab also cycles filter (prevents global PrevTab from leaking through)
    session_import.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::NONE),
        Action::CycleImportFilter,
    );
    session_import.insert(
        KeyCombo::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        Action::CycleImportFilter,
    );
    session_import.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::ImportSession,
    );
    session_import.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    session_import.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );

    // ========== Command Palette ==========
    let palette = config
        .context
        .entry(KeyContext::CommandPalette)
        .or_default();

    palette.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    palette.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    bind(palette, "C-k", Action::SelectPrev);
    bind(palette, "C-j", Action::SelectNext);
    bind(palette, "C-p", Action::SelectPrev);
    bind(palette, "C-n", Action::SelectNext);
    palette.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    palette.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );
    palette.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );

    // ========== Theme Picker ==========
    let theme_picker = config.context.entry(KeyContext::ThemePicker).or_default();

    theme_picker.insert(
        KeyCombo::new(KeyCode::Backspace, KeyModifiers::NONE),
        Action::Backspace,
    );
    theme_picker.insert(
        KeyCombo::new(KeyCode::Delete, KeyModifiers::NONE),
        Action::Delete,
    );
    theme_picker.insert(
        KeyCombo::new(KeyCode::Up, KeyModifiers::NONE),
        Action::SelectPrev,
    );
    theme_picker.insert(
        KeyCombo::new(KeyCode::Down, KeyModifiers::NONE),
        Action::SelectNext,
    );
    bind(theme_picker, "C-k", Action::SelectPrev);
    bind(theme_picker, "C-j", Action::SelectNext);
    theme_picker.insert(
        KeyCombo::new(KeyCode::Enter, KeyModifiers::NONE),
        Action::Confirm,
    );
    theme_picker.insert(
        KeyCombo::new(KeyCode::Esc, KeyModifiers::NONE),
        Action::Cancel,
    );

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrl_4_bound_to_toggle_agent_mode() {
        let config = default_keybindings();

        // Ctrl+\ shows as Ctrl+4 in terminals
        let key_combo: KeyCombo = "C-4".parse().expect("Should parse C-4");

        // Should be in global bindings
        let action = config.global.get(&key_combo);
        assert!(
            action.is_some(),
            "Ctrl+4 should be bound in global keybindings"
        );
        assert!(
            matches!(action, Some(Action::ToggleAgentMode)),
            "Ctrl+4 should be bound to ToggleAgentMode, got {:?}",
            action
        );
    }
}
