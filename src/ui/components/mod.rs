mod add_repo_dialog;
mod agent_selector;
mod base_dir_dialog;
mod chat_message;
mod chat_view;
mod command_palette;
mod confirmation_dialog;
mod dialog;
mod error_dialog;
pub mod file_path_detector;
mod file_viewer_view;
mod global_footer;
mod help_dialog;
mod inline_prompt;
mod input_box;
mod key_hints;
mod knight_rider_spinner;
mod logo_shine;
mod markdown;
mod missing_tool_dialog;
mod model_selector;
mod multi_select_dialog;
mod path_input;
mod project_picker;
mod provider_selector;
mod raw_events_types;
mod raw_events_view;
mod reasoning_selector;
mod scrollbar;
mod searchable_list;
mod session_header;
mod session_import_picker;
mod settings_menu;
mod sidebar;
mod slash_menu;
pub(crate) mod source_highlighter;
mod spinner;
mod status_bar;
mod tab_bar;
mod text_input;
pub mod theme;
mod theme_picker;
mod thinking_indicator;
mod tree_view;
mod turn_summary;
mod workspace_defaults_dialog;

pub use add_repo_dialog::{AddRepoDialog, AddRepoDialogState};
pub use agent_selector::{AgentSelector, AgentSelectorState};
pub use base_dir_dialog::{BaseDirDialog, BaseDirDialogState};
pub use chat_message::{ChatMessage, MessageRole};
pub use chat_view::{ChatView, HoveredFilePath};
pub use command_palette::{CommandPalette, CommandPaletteEntry, CommandPaletteState};
pub use confirmation_dialog::{
    ConfirmationContext, ConfirmationDialog, ConfirmationDialogState, ConfirmationType,
};
pub use dialog::{
    dialog_content_area, DialogFrame, InstructionBar, StatusLine, DIALOG_CONTENT_PADDING_X,
    DIALOG_CONTENT_PADDING_Y,
};
pub use error_dialog::{ErrorDialog, ErrorDialogState};
pub use file_viewer_view::FileViewerView;
pub use global_footer::{FooterContext, GlobalFooter};
pub use help_dialog::{HelpCategory, HelpDialog, HelpDialogState, KeybindingEntry};
pub use inline_prompt::{
    InlinePrompt, InlinePromptState, InlinePromptType, PromptAction, PromptAnswer, PromptResponse,
};
pub use input_box::InputBox;
pub use key_hints::{render_key_hints, render_key_hints_responsive, KeyHintBarStyle};
pub use knight_rider_spinner::KnightRiderSpinner;
pub use logo_shine::LogoShineAnimation;
pub use markdown::MarkdownRenderer;
pub use missing_tool_dialog::{
    MissingToolDialog, MissingToolDialogState, MissingToolResult, StartupToolDialog,
};
pub use model_selector::{
    DefaultModelSelection, ModelSelector, ModelSelectorItem, ModelSelectorState,
};
pub use multi_select_dialog::{MultiSelectDialog, MultiSelectDialogState, MultiSelectItem};
pub use path_input::PathInputState;
pub use project_picker::{ProjectEntry, ProjectPicker, ProjectPickerState};
pub use provider_selector::{ProviderSelector, ProviderSelectorState};
pub use raw_events_types::{
    EventDetailState, EventDirection, RawEventEntry, DETAIL_PANEL_BREAKPOINT,
};
pub use raw_events_view::{RawEventsClick, RawEventsScrollbarMetrics, RawEventsView};
pub use reasoning_selector::{ReasoningOption, ReasoningSelector, ReasoningSelectorState};
pub use scrollbar::{render_minimal_scrollbar, scrollbar_offset_from_point, ScrollbarMetrics};
pub use searchable_list::SearchableListState;
pub use session_header::SessionHeader;
pub use session_import_picker::{AgentFilter, SessionImportPicker, SessionImportPickerState};
pub use settings_menu::{SettingsMenu, SettingsMenuEntry, SettingsMenuEntryId, SettingsMenuState};
pub use sidebar::{Sidebar, SidebarState, SIDEBAR_HEADER_ROWS};
pub use slash_menu::{SlashMenu, SlashMenuEntry, SlashMenuState};
pub use spinner::Spinner;
pub use status_bar::StatusBar;
pub use tab_bar::{TabBar, TabBarHitTarget};
pub use text_input::TextInputState;
pub use theme_picker::{ThemePicker, ThemePickerItem, ThemePickerState};
// Theme system - new dynamic API (use these for new code)
pub use theme::{
    // Accent colors (functions)
    accent_error,
    accent_primary,
    accent_secondary,
    accent_success,
    accent_warning,
    // Agent colors (functions)
    agent_claude,
    agent_codex,
    agent_gemini,
    agent_opencode,
    // Background colors (functions)
    bg_base,
    bg_elevated,
    bg_highlight,
    bg_surface,
    bg_terminal,
    // Color utilities
    boost_brightness,
    // Border colors (functions)
    border_default,
    border_dimmed,
    border_focused,
    contrast_ratio,
    // Theme management
    current_theme,
    current_theme_name,
    darken,
    desaturate,
    dialog_bg,
    // Tool block colors (functions)
    diff_add,
    diff_remove,
    dim,
    ensure_contrast_bg,
    ensure_contrast_fg,
    // Legacy aliases (functions)
    footer_bg,
    init_theme,
    input_bg,
    interpolate,
    key_hint_bg,
    lighten,
    list_themes,
    load_theme_by_name,
    load_theme_from_path,
    markdown_code_bg,
    markdown_inline_code_bg,
    parse_hex_color,
    // PR state colors (functions)
    pr_closed_bg,
    pr_draft_bg,
    pr_merged_bg,
    pr_open_bg,
    pr_unknown_bg,
    refresh_themes,
    relative_luminance,
    saturate,
    selected_bg,
    selected_bg_dim,
    set_theme,
    shift_hue,
    // Logo shine colors (functions)
    shine_center,
    shine_edge,
    shine_mid,
    shine_peak,
    sidebar_bg,
    // Spinner colors (functions)
    spinner_active,
    spinner_inactive,
    spinner_trail_1,
    spinner_trail_2,
    spinner_trail_3,
    spinner_trail_4,
    spinner_trail_5,
    status_bar_bg,
    tab_bar_bg,
    // Text colors (functions)
    text_bright,
    text_faint,
    text_muted,
    text_primary,
    text_secondary,
    toggle_theme,
    tool_block_bg,
    tool_command,
    tool_comment,
    tool_output,
    // Theme types
    Theme,
    ThemeInfo,
    ThemeRegistry,
    ThemeSource,
};

pub use thinking_indicator::{ProcessingState, ThinkingIndicator};
pub use tree_view::{
    ActionType, NodeType, SidebarData, SidebarGitDisplay, TreeNode, TreeView, TreeViewState,
    SIDEBAR_GIT_DISPLAY,
};
pub use turn_summary::{FileChange, TurnSummary};
pub use workspace_defaults_dialog::{
    WorkspaceDefaultsDialog, WorkspaceDefaultsDialogState, WorkspaceDefaultsDraft,
};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Truncate a string to fit within a maximum display width, adding "..." if truncated.
/// Uses unicode display width to handle multi-byte and wide characters correctly.
pub(crate) fn truncate_to_width(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let current_width = UnicodeWidthStr::width(s);
    if current_width <= max_width {
        return s.to_string();
    }

    let ellipsis = "...";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis);
    if max_width <= ellipsis_width {
        return ellipsis.chars().take(max_width).collect();
    }

    let target_width = max_width - ellipsis_width;
    let mut result = String::new();
    let mut width = 0;

    for ch in s.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if width + ch_width > target_width {
            break;
        }
        result.push(ch);
        width += ch_width;
    }

    result.push_str(ellipsis);
    result
}
