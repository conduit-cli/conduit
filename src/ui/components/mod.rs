mod add_repo_dialog;
mod agent_selector;
mod base_dir_dialog;
mod chat_message;
mod chat_view;
mod command_palette;
mod confirmation_dialog;
mod dialog;
mod error_dialog;
mod global_footer;
mod help_dialog;
mod input_box;
mod key_hints;
mod knight_rider_spinner;
mod logo_shine;
mod markdown;
mod model_selector;
mod path_input;
mod project_picker;
mod raw_events_types;
mod raw_events_view;
mod scrollbar;
mod searchable_list;
mod session_import_picker;
mod sidebar;
mod spinner;
mod status_bar;
mod tab_bar;
mod text_input;
mod theme;
mod thinking_indicator;
mod tree_view;
mod turn_summary;

pub use add_repo_dialog::{AddRepoDialog, AddRepoDialogState};
pub use agent_selector::{AgentSelector, AgentSelectorState};
pub use base_dir_dialog::{BaseDirDialog, BaseDirDialogState};
pub use chat_message::{ChatMessage, MessageRole};
pub use chat_view::ChatView;
pub use command_palette::{CommandPalette, CommandPaletteEntry, CommandPaletteState};
pub use confirmation_dialog::{
    ConfirmationContext, ConfirmationDialog, ConfirmationDialogState, ConfirmationType,
};
pub use dialog::{DialogFrame, InstructionBar, StatusLine};
pub use error_dialog::{ErrorDialog, ErrorDialogState};
pub use global_footer::{FooterContext, GlobalFooter};
pub use help_dialog::{HelpCategory, HelpDialog, HelpDialogState, KeybindingEntry};
pub use input_box::InputBox;
pub use key_hints::{render_key_hints, render_key_hints_responsive, KeyHintBarStyle};
pub use knight_rider_spinner::KnightRiderSpinner;
pub use logo_shine::LogoShineAnimation;
pub use markdown::MarkdownRenderer;
pub use model_selector::{ModelSelector, ModelSelectorItem, ModelSelectorState};
pub use path_input::PathInputState;
pub use project_picker::{ProjectEntry, ProjectPicker, ProjectPickerState};
pub use raw_events_types::{
    EventDetailState, EventDirection, RawEventEntry, DETAIL_PANEL_BREAKPOINT,
};
pub use raw_events_view::{RawEventsClick, RawEventsScrollbarMetrics, RawEventsView};
pub use scrollbar::{render_minimal_scrollbar, scrollbar_offset_from_point, ScrollbarMetrics};
pub use searchable_list::SearchableListState;
pub use session_import_picker::{AgentFilter, SessionImportPicker, SessionImportPickerState};
pub use sidebar::{Sidebar, SidebarState};
pub use spinner::Spinner;
pub use status_bar::StatusBar;
pub use tab_bar::TabBar;
pub use text_input::TextInputState;
pub use theme::{
    ACCENT_ERROR,
    ACCENT_PRIMARY,
    ACCENT_SECONDARY,
    ACCENT_SUCCESS,
    ACCENT_WARNING,
    AGENT_CLAUDE,
    AGENT_CODEX,
    // New modern palette
    BG_BASE,
    BG_ELEVATED,
    BG_HIGHLIGHT,
    BG_SURFACE,
    BG_TERMINAL,
    BORDER_DEFAULT,
    BORDER_DIMMED,
    BORDER_FOCUSED,
    // Legacy aliases (backward compatibility)
    FOOTER_BG,
    INPUT_BG,
    KEY_HINT_BG,
    // PR state colors
    PR_CLOSED_BG,
    PR_DRAFT_BG,
    PR_MERGED_BG,
    PR_OPEN_BG,
    PR_UNKNOWN_BG,
    SELECTED_BG,
    SELECTED_BG_DIM,
    // Logo shine colors
    SHINE_CENTER,
    SHINE_EDGE,
    SHINE_MID,
    SHINE_PEAK,
    // Knight Rider spinner colors
    SPINNER_ACTIVE,
    SPINNER_INACTIVE,
    SPINNER_TRAIL_1,
    SPINNER_TRAIL_2,
    SPINNER_TRAIL_3,
    SPINNER_TRAIL_4,
    SPINNER_TRAIL_5,
    STATUS_BAR_BG,
    TAB_BAR_BG,
    TEXT_BRIGHT,
    TEXT_FAINT,
    TEXT_MUTED,
    TEXT_PRIMARY,
    TEXT_SECONDARY,
};
pub use thinking_indicator::{ProcessingState, ThinkingIndicator};
pub use tree_view::{
    ActionType, NodeType, SidebarData, SidebarGitDisplay, TreeNode, TreeView, TreeViewState,
    SIDEBAR_GIT_DISPLAY,
};
pub use turn_summary::{FileChange, TurnSummary};
