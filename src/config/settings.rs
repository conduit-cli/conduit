use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::agent::AgentType;
use crate::ui::action::Action;
use crate::util::paths::config_path;

use super::default_keys::default_keybindings;
use super::keys::{parse_key_notation, KeyContext, KeybindingConfig};

/// Example configuration file contents (bundled with the binary)
pub const EXAMPLE_CONFIG: &str = include_str!("config.toml.example");

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Default agent type for new sessions
    pub default_agent: AgentType,
    /// Working directory for agent operations
    pub working_dir: PathBuf,
    /// Maximum number of tabs allowed
    pub max_tabs: usize,
    /// Show token usage in status bar
    pub show_token_usage: bool,
    /// Show estimated cost in status bar
    pub show_cost: bool,
    /// Default allowed tools for Claude
    pub claude_allowed_tools: Vec<String>,
    /// Claude model pricing (input tokens per $1M)
    pub claude_input_cost_per_million: f64,
    /// Claude model pricing (output tokens per $1M)
    pub claude_output_cost_per_million: f64,
    /// Keybinding configuration
    pub keybindings: KeybindingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_agent: AgentType::Claude,
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            max_tabs: 10,
            show_token_usage: true,
            show_cost: true,
            claude_allowed_tools: vec![
                "Read".into(),
                "Edit".into(),
                "Write".into(),
                "Bash".into(),
                "Glob".into(),
                "Grep".into(),
            ],
            // Claude Sonnet 3.5 pricing
            claude_input_cost_per_million: 3.0,
            claude_output_cost_per_million: 15.0,
            keybindings: default_keybindings(),
        }
    }
}

/// TOML representation of keybinding configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlKeybindings {
    /// Global keybindings (apply to all contexts)
    #[serde(flatten)]
    pub global: HashMap<String, String>,

    /// Chat mode keybindings
    pub chat: Option<HashMap<String, String>>,

    /// Scrolling mode keybindings
    pub scrolling: Option<HashMap<String, String>>,

    /// Sidebar keybindings
    pub sidebar: Option<HashMap<String, String>>,

    /// Dialog keybindings
    pub dialog: Option<HashMap<String, String>>,

    /// Project picker keybindings
    pub project_picker: Option<HashMap<String, String>>,

    /// Model selector keybindings
    pub model_selector: Option<HashMap<String, String>>,

    /// Add repository dialog keybindings
    pub add_repository: Option<HashMap<String, String>>,

    /// Base directory dialog keybindings
    pub base_dir: Option<HashMap<String, String>>,

    /// Raw events view keybindings
    pub raw_events: Option<HashMap<String, String>>,
}

/// TOML representation of the config file
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlConfig {
    /// Keybinding configuration
    pub keys: Option<TomlKeybindings>,
}

impl TomlKeybindings {
    /// Convert TOML keybindings to KeybindingConfig
    fn to_keybinding_config(&self) -> KeybindingConfig {
        let mut config = KeybindingConfig::new();

        // Parse global bindings
        for (action_name, key_str) in &self.global {
            // Skip context sections (they're handled separately)
            if matches!(
                action_name.as_str(),
                "chat"
                    | "scrolling"
                    | "sidebar"
                    | "dialog"
                    | "project_picker"
                    | "model_selector"
                    | "add_repository"
                    | "base_dir"
                    | "raw_events"
            ) {
                continue;
            }

            if let (Ok(combo), Some(action)) =
                (parse_key_notation(key_str), parse_action(action_name))
            {
                config.global.insert(combo, action);
            }
        }

        // Parse context-specific bindings
        if let Some(chat) = &self.chat {
            parse_context_bindings(&mut config, KeyContext::Chat, chat);
        }
        if let Some(scrolling) = &self.scrolling {
            parse_context_bindings(&mut config, KeyContext::Scrolling, scrolling);
        }
        if let Some(sidebar) = &self.sidebar {
            parse_context_bindings(&mut config, KeyContext::Sidebar, sidebar);
        }
        if let Some(dialog) = &self.dialog {
            parse_context_bindings(&mut config, KeyContext::Dialog, dialog);
        }
        if let Some(picker) = &self.project_picker {
            parse_context_bindings(&mut config, KeyContext::ProjectPicker, picker);
        }
        if let Some(model) = &self.model_selector {
            parse_context_bindings(&mut config, KeyContext::ModelSelector, model);
        }
        if let Some(add_repo) = &self.add_repository {
            parse_context_bindings(&mut config, KeyContext::AddRepository, add_repo);
        }
        if let Some(base_dir) = &self.base_dir {
            parse_context_bindings(&mut config, KeyContext::BaseDir, base_dir);
        }
        if let Some(raw) = &self.raw_events {
            parse_context_bindings(&mut config, KeyContext::RawEvents, raw);
        }

        config
    }
}

/// Parse context-specific keybindings
fn parse_context_bindings(
    config: &mut KeybindingConfig,
    context: KeyContext,
    bindings: &HashMap<String, String>,
) {
    let context_map = config.context.entry(context).or_default();
    for (action_name, key_str) in bindings {
        if let (Ok(combo), Some(action)) = (parse_key_notation(key_str), parse_action(action_name))
        {
            context_map.insert(combo, action);
        }
    }
}

/// Parse an action name string into an Action
pub fn parse_action(name: &str) -> Option<Action> {
    match name {
        // Global
        "quit" => Some(Action::Quit),
        "toggle_sidebar" => Some(Action::ToggleSidebar),
        "new_project" => Some(Action::NewProject),
        "open_pr" => Some(Action::OpenPr),
        "interrupt_agent" => Some(Action::InterruptAgent),
        "toggle_view_mode" => Some(Action::ToggleViewMode),
        "show_model_selector" => Some(Action::ShowModelSelector),
        "toggle_metrics" => Some(Action::ToggleMetrics),
        "dump_debug_state" => Some(Action::DumpDebugState),
        "copy_selection" => Some(Action::CopySelection),

        // Tab management
        "close_tab" => Some(Action::CloseTab),
        "next_tab" => Some(Action::NextTab),
        "prev_tab" => Some(Action::PrevTab),

        // Scrolling
        "scroll_up" => Some(Action::ScrollUp(1)),
        "scroll_down" => Some(Action::ScrollDown(1)),
        "scroll_page_up" => Some(Action::ScrollPageUp),
        "scroll_page_down" => Some(Action::ScrollPageDown),
        "scroll_to_top" => Some(Action::ScrollToTop),
        "scroll_to_bottom" => Some(Action::ScrollToBottom),

        // Input editing
        "insert_newline" => Some(Action::InsertNewline),
        "backspace" => Some(Action::Backspace),
        "delete" => Some(Action::Delete),
        "delete_word_back" => Some(Action::DeleteWordBack),
        "delete_word_forward" => Some(Action::DeleteWordForward),
        "delete_to_start" => Some(Action::DeleteToStart),
        "delete_to_end" => Some(Action::DeleteToEnd),
        "move_cursor_left" => Some(Action::MoveCursorLeft),
        "move_cursor_right" => Some(Action::MoveCursorRight),
        "move_cursor_start" => Some(Action::MoveCursorStart),
        "move_cursor_end" => Some(Action::MoveCursorEnd),
        "move_word_left" => Some(Action::MoveWordLeft),
        "move_word_right" => Some(Action::MoveWordRight),
        "move_cursor_up" => Some(Action::MoveCursorUp),
        "move_cursor_down" => Some(Action::MoveCursorDown),
        "history_prev" => Some(Action::HistoryPrev),
        "history_next" => Some(Action::HistoryNext),
        "submit" => Some(Action::Submit),

        // Navigation
        "select_next" => Some(Action::SelectNext),
        "select_prev" => Some(Action::SelectPrev),
        "select_page_down" => Some(Action::SelectPageDown),
        "select_page_up" => Some(Action::SelectPageUp),
        "confirm" => Some(Action::Confirm),
        "cancel" => Some(Action::Cancel),
        "expand_or_select" => Some(Action::ExpandOrSelect),
        "collapse" => Some(Action::Collapse),
        "add_repository" => Some(Action::AddRepository),
        "open_settings" => Some(Action::OpenSettings),
        "archive_or_remove" => Some(Action::ArchiveOrRemove),

        // Sidebar
        "enter_sidebar_mode" => Some(Action::EnterSidebarMode),
        "exit_sidebar_mode" => Some(Action::ExitSidebarMode),

        // Raw events
        "raw_events_select_next" => Some(Action::RawEventsSelectNext),
        "raw_events_select_prev" => Some(Action::RawEventsSelectPrev),
        "raw_events_toggle_expand" => Some(Action::RawEventsToggleExpand),
        "raw_events_collapse" => Some(Action::RawEventsCollapse),

        // Dialog
        "confirm_yes" => Some(Action::ConfirmYes),
        "confirm_no" => Some(Action::ConfirmNo),
        "confirm_toggle" => Some(Action::ConfirmToggle),
        "toggle_details" => Some(Action::ToggleDetails),

        // Agent
        "select_agent" => Some(Action::SelectAgent),

        // Session import
        "open_session_import" | "import" => Some(Action::OpenSessionImport),
        "import_session" => Some(Action::ImportSession),
        "cycle_import_filter" => Some(Action::CycleImportFilter),

        // Command mode
        "show_help" => Some(Action::ShowHelp),
        "execute_command" => Some(Action::ExecuteCommand),
        "complete_command" => Some(Action::CompleteCommand),

        // Command palette
        "open_command_palette" => Some(Action::OpenCommandPalette),

        _ => None,
    }
}

/// All available command names for autocomplete
pub const COMMAND_NAMES: &[&str] = &[
    // Global
    "quit",
    "toggle_sidebar",
    "new_project",
    "open_pr",
    "interrupt_agent",
    "toggle_view_mode",
    "show_model_selector",
    "toggle_metrics",
    "dump_debug_state",
    "copy_selection",
    // Tab management
    "close_tab",
    "next_tab",
    "prev_tab",
    // Scrolling
    "scroll_up",
    "scroll_down",
    "scroll_page_up",
    "scroll_page_down",
    "scroll_to_top",
    "scroll_to_bottom",
    // Input editing
    "insert_newline",
    "backspace",
    "delete",
    "delete_word_back",
    "delete_word_forward",
    "delete_to_start",
    "delete_to_end",
    "move_cursor_left",
    "move_cursor_right",
    "move_cursor_start",
    "move_cursor_end",
    "move_word_left",
    "move_word_right",
    "move_cursor_up",
    "move_cursor_down",
    "history_prev",
    "history_next",
    "submit",
    // Navigation
    "select_next",
    "select_prev",
    "select_page_down",
    "select_page_up",
    "confirm",
    "cancel",
    "expand_or_select",
    "collapse",
    "add_repository",
    "open_settings",
    "archive_or_remove",
    // Sidebar
    "enter_sidebar_mode",
    "exit_sidebar_mode",
    // Raw events
    "raw_events_select_next",
    "raw_events_select_prev",
    "raw_events_toggle_expand",
    "raw_events_collapse",
    // Dialog
    "confirm_yes",
    "confirm_no",
    "confirm_toggle",
    "toggle_details",
    // Agent
    "select_agent",
    // Session import
    "open_session_import",
    "import",
    "import_session",
    "cycle_import_filter",
    // Command mode
    "show_help",
    // Command palette
    "open_command_palette",
    // Aliases
    "help",
    "h",
    "q",
];

impl Config {
    /// Load configuration from file, merging with defaults
    pub fn load() -> Self {
        let mut config = Config::default();

        let config_file = config_path();

        // Create example config on first run
        if !config_file.exists() {
            Self::create_default_config(&config_file);
        }

        // Try to load user config
        if config_file.exists() {
            if let Ok(contents) = fs::read_to_string(&config_file) {
                if let Ok(toml_config) = toml::from_str::<TomlConfig>(&contents) {
                    // Merge user keybindings on top of defaults
                    if let Some(keys) = toml_config.keys {
                        let user_bindings = keys.to_keybinding_config();
                        config.keybindings.merge(user_bindings);
                    }
                }
            }
        }

        config
    }

    /// Create the default config file from the bundled example
    fn create_default_config(path: &PathBuf) {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create config directory: {}", e);
                    return;
                }
            }
        }

        // Write the example config
        if let Err(e) = fs::write(path, EXAMPLE_CONFIG) {
            eprintln!("Failed to write default config: {}", e);
        }
    }

    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = dir;
        self
    }

    pub fn with_default_agent(mut self, agent: AgentType) -> Self {
        self.default_agent = agent;
        self
    }

    /// Calculate cost for given token usage
    pub fn calculate_cost(&self, input_tokens: i64, output_tokens: i64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.claude_input_cost_per_million;
        let output_cost =
            (output_tokens as f64 / 1_000_000.0) * self.claude_output_cost_per_million;
        input_cost + output_cost
    }
}
