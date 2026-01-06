mod add_repo_dialog;
mod agent_selector;
mod base_dir_dialog;
mod chat_message;
mod chat_view;
mod confirmation_dialog;
mod dialog;
mod error_dialog;
mod global_footer;
mod help_dialog;
mod input_box;
mod key_hints;
mod markdown;
mod model_selector;
mod path_input;
mod searchable_list;
mod project_picker;
mod raw_events_types;
mod raw_events_view;
mod scrollbar;
mod session_import_picker;
mod sidebar;
mod spinner;
mod status_bar;
mod tab_bar;
mod theme;
mod text_input;
mod thinking_indicator;
mod tree_view;
mod turn_summary;

pub use add_repo_dialog::{AddRepoDialog, AddRepoDialogState};
pub use agent_selector::{AgentSelector, AgentSelectorState};
pub use base_dir_dialog::{BaseDirDialog, BaseDirDialogState};
pub use confirmation_dialog::{
    ConfirmationContext, ConfirmationDialog, ConfirmationDialogState, ConfirmationType,
};
pub use dialog::{DialogFrame, InstructionBar, StatusLine};
pub use error_dialog::{ErrorDialog, ErrorDialogState};
pub use help_dialog::{HelpCategory, HelpDialog, HelpDialogState, KeybindingEntry};
pub use project_picker::{ProjectEntry, ProjectPicker, ProjectPickerState};
pub use session_import_picker::{AgentFilter, SessionImportPicker, SessionImportPickerState};
pub use scrollbar::{render_minimal_scrollbar, scrollbar_offset_from_point, ScrollbarMetrics};
pub use text_input::TextInputState;
pub use chat_message::{ChatMessage, MessageRole};
pub use chat_view::ChatView;
pub use global_footer::GlobalFooter;
pub use input_box::InputBox;
pub use key_hints::{render_key_hints, KeyHintBarStyle};
pub use markdown::MarkdownRenderer;
pub use model_selector::{ModelSelector, ModelSelectorItem, ModelSelectorState};
pub use path_input::PathInputState;
pub use searchable_list::SearchableListState;
pub use raw_events_types::{EventDetailState, EventDirection, RawEventEntry, DETAIL_PANEL_BREAKPOINT};
pub use raw_events_view::{RawEventsClick, RawEventsScrollbarMetrics, RawEventsView};
pub use sidebar::{Sidebar, SidebarState};
pub use spinner::Spinner;
pub use status_bar::StatusBar;
pub use tab_bar::TabBar;
pub use theme::{
    // New modern palette
    BG_BASE, BG_ELEVATED, BG_HIGHLIGHT, BG_SURFACE, BG_TERMINAL,
    TEXT_BRIGHT, TEXT_FAINT, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
    ACCENT_ERROR, ACCENT_PRIMARY, ACCENT_SECONDARY, ACCENT_SUCCESS, ACCENT_WARNING,
    AGENT_CLAUDE, AGENT_CODEX,
    BORDER_DEFAULT, BORDER_DIMMED, BORDER_FOCUSED,
    // Legacy aliases (backward compatibility)
    FOOTER_BG, INPUT_BG, KEY_HINT_BG, SELECTED_BG, SELECTED_BG_DIM, STATUS_BAR_BG, TAB_BAR_BG,
};
pub use thinking_indicator::{ProcessingState, ThinkingIndicator};
pub use tree_view::{ActionType, NodeType, SidebarData, TreeNode, TreeView, TreeViewState};
pub use turn_summary::{FileChange, TurnSummary};
