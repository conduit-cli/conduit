use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table};

use crate::agent::{AgentType, ModelRegistry};
use crate::ui::action::Action;
use crate::util::paths::config_path;
use crate::util::tools::{Tool, ToolPaths};

use super::default_keys::default_keybindings;
use super::keys::{parse_key_notation, KeyContext, KeybindingConfig};

/// Example configuration file contents (bundled with the binary)
pub const EXAMPLE_CONFIG: &str = include_str!("config.toml.example");

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Default agent type for new sessions
    pub default_agent: AgentType,
    /// Default model ID for the default agent
    pub default_model: Option<String>,
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
    /// Configured paths for external tools (git, gh, claude, codex, gemini)
    pub tool_paths: ToolPaths,
    /// Theme name from config (None = use default)
    pub theme_name: Option<String>,
    /// Custom theme path from config (takes precedence over name)
    pub theme_path: Option<PathBuf>,
    /// Queue configuration
    pub queue: QueueConfig,
    /// Steering configuration
    pub steer: SteerConfig,
    /// Selection and clipboard configuration
    pub selection: SelectionConfig,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QueueDelivery {
    Separate,
    Concat,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, Copy)]
pub struct QueueConfig {
    pub delivery: QueueDelivery,
    pub mode: QueueMode,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlQueueConfig {
    pub delivery: Option<QueueDelivery>,
    pub mode: Option<QueueMode>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SteerBehavior {
    Hard,
    Soft,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SteerFallback {
    Queue,
    Interrupt,
    Prompt,
}

#[derive(Debug, Clone, Copy)]
pub struct SteerConfig {
    pub behavior: SteerBehavior,
    pub fallback: SteerFallback,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlSteerConfig {
    pub behavior: Option<SteerBehavior>,
    pub fallback: Option<SteerFallback>,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectionConfig {
    pub auto_copy_selection: bool,
    pub clear_selection_after_copy: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlSelectionConfig {
    pub auto_copy_selection: Option<bool>,
    pub clear_selection_after_copy: Option<bool>,
}

/// TOML representation of default model
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlDefaultModelConfig {
    pub agent: Option<String>,
    pub model: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_agent: AgentType::Claude,
            default_model: None,
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
            tool_paths: ToolPaths::default(),
            theme_name: None,
            theme_path: None,
            queue: QueueConfig {
                delivery: QueueDelivery::Separate,
                mode: QueueMode::OneAtATime,
            },
            steer: SteerConfig {
                behavior: SteerBehavior::Hard,
                fallback: SteerFallback::Queue,
            },
            selection: SelectionConfig {
                auto_copy_selection: true,
                clear_selection_after_copy: true,
            },
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

    /// Queue editor keybindings
    pub queue: Option<HashMap<String, String>>,
}

/// TOML representation of theme configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlThemeConfig {
    /// Theme name (built-in or VS Code theme label/extension ID)
    pub name: Option<String>,
    /// Direct path to VS Code theme JSON file
    pub path: Option<PathBuf>,
}

/// TOML representation of the config file
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TomlConfig {
    /// Default model configuration
    pub model: Option<TomlDefaultModelConfig>,
    /// Keybinding configuration
    pub keys: Option<TomlKeybindings>,
    /// Tool path configuration
    pub tools: Option<ToolPaths>,
    /// Theme configuration
    pub theme: Option<TomlThemeConfig>,
    /// Queue configuration
    pub queue: Option<TomlQueueConfig>,
    /// Steering configuration
    pub steer: Option<TomlSteerConfig>,
    /// Selection configuration
    pub selection: Option<TomlSelectionConfig>,
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
                    | "queue"
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
        if let Some(queue) = &self.queue {
            parse_context_bindings(&mut config, KeyContext::QueueEditing, queue);
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
        "fork_session" => Some(Action::ForkSession),
        "interrupt_agent" => Some(Action::InterruptAgent),
        "toggle_view_mode" => Some(Action::ToggleViewMode),
        "show_model_selector" => Some(Action::ShowModelSelector),
        "show_theme_picker" => Some(Action::ShowThemePicker),
        "toggle_metrics" => Some(Action::ToggleMetrics),
        "dump_debug_state" => Some(Action::DumpDebugState),
        "suspend" => Some(Action::Suspend),
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
        "prev_user_message" => Some(Action::ScrollPrevUserMessage),
        "next_user_message" => Some(Action::ScrollNextUserMessage),

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
        "submit_steer" => Some(Action::SubmitSteer),
        "open_queue_editor" => Some(Action::OpenQueueEditor),
        "close_queue_editor" => Some(Action::CloseQueueEditor),
        "queue_move_up" => Some(Action::QueueMoveUp),
        "queue_move_down" => Some(Action::QueueMoveDown),
        "queue_edit" => Some(Action::QueueEdit),
        "queue_delete" => Some(Action::QueueDelete),
        "edit_prompt_external" => Some(Action::EditPromptExternal),

        // Navigation
        "select_next" => Some(Action::SelectNext),
        "select_prev" => Some(Action::SelectPrev),
        "select_page_down" => Some(Action::SelectPageDown),
        "select_page_up" => Some(Action::SelectPageUp),
        "confirm" => Some(Action::Confirm),
        "cancel" => Some(Action::Cancel),
        "set_default_model" => Some(Action::SetDefaultModel),
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
    "fork_session",
    "interrupt_agent",
    "toggle_view_mode",
    "show_model_selector",
    "show_theme_picker",
    "toggle_metrics",
    "dump_debug_state",
    "suspend",
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
    "prev_user_message",
    "next_user_message",
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
    "submit_steer",
    "open_queue_editor",
    "close_queue_editor",
    "queue_move_up",
    "queue_move_down",
    "queue_edit",
    "queue_delete",
    "edit_prompt_external",
    // Navigation
    "select_next",
    "select_prev",
    "select_page_down",
    "select_page_up",
    "confirm",
    "cancel",
    "set_default_model",
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
                    // Load default model (agent + model pair)
                    if let Some(model_cfg) = toml_config.model {
                        if let (Some(agent), Some(model_id)) =
                            (model_cfg.agent.as_deref(), model_cfg.model.as_deref())
                        {
                            let agent_type = AgentType::parse(agent);
                            if let Some(model) = ModelRegistry::find_model(agent_type, model_id) {
                                config.default_agent = agent_type;
                                config.default_model = Some(model.id);
                            }
                        }
                    }

                    // Merge user keybindings on top of defaults
                    if let Some(keys) = toml_config.keys {
                        let user_bindings = keys.to_keybinding_config();
                        config.keybindings.merge(user_bindings);
                    }

                    // Load tool paths if configured
                    if let Some(tools) = toml_config.tools {
                        config.tool_paths = tools;
                    }

                    // Load theme configuration
                    if let Some(theme) = toml_config.theme {
                        config.theme_path = theme.path;
                        config.theme_name = theme.name;
                    }

                    // Load queue configuration
                    if let Some(queue) = toml_config.queue {
                        if let Some(delivery) = queue.delivery {
                            config.queue.delivery = delivery;
                        }
                        if let Some(mode) = queue.mode {
                            config.queue.mode = mode;
                        }
                    }

                    // Load steering configuration
                    if let Some(steer) = toml_config.steer {
                        if let Some(behavior) = steer.behavior {
                            config.steer.behavior = behavior;
                        }
                        if let Some(fallback) = steer.fallback {
                            config.steer.fallback = fallback;
                        }
                    }

                    // Load selection configuration
                    if let Some(selection) = toml_config.selection {
                        if let Some(auto_copy_selection) = selection.auto_copy_selection {
                            config.selection.auto_copy_selection = auto_copy_selection;
                        }
                        if let Some(clear_selection_after_copy) =
                            selection.clear_selection_after_copy
                        {
                            config.selection.clear_selection_after_copy =
                                clear_selection_after_copy;
                        }
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

    /// Get the default model ID for an agent (config override with fallback)
    pub fn default_model_for(&self, agent_type: AgentType) -> String {
        if agent_type == self.default_agent {
            if let Some(id) = self.default_model.as_deref() {
                if let Some(model) = ModelRegistry::find_model(agent_type, id) {
                    return model.id;
                }
            }
        }

        ModelRegistry::default_model(agent_type)
    }

    /// Update the default model for an agent in memory
    pub fn set_default_model(&mut self, agent_type: AgentType, model_id: String) {
        self.default_agent = agent_type;
        self.default_model = Some(model_id);
    }

    /// Calculate cost for given token usage
    pub fn calculate_cost(&self, input_tokens: i64, output_tokens: i64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.claude_input_cost_per_million;
        let output_cost =
            (output_tokens as f64 / 1_000_000.0) * self.claude_output_cost_per_million;
        input_cost + output_cost
    }
}

/// Save a tool path to the config file
///
/// This function reads the existing config.toml, adds or updates the tool path
/// in the [tools] section, and writes it back while preserving all other content.
pub fn save_tool_path(tool: Tool, path: &Path) -> std::io::Result<()> {
    let config_file = config_path();

    // Read existing config or start with empty document
    let contents = if config_file.exists() {
        fs::read_to_string(&config_file)?
    } else {
        String::new()
    };

    // Parse as TOML document
    let mut doc: DocumentMut = contents
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Ensure [tools] section exists
    if !doc.contains_key("tools") {
        doc["tools"] = Item::Table(Table::new());
    }

    // Set the tool path
    let path_str = path.to_string_lossy().to_string();
    doc["tools"][tool.binary_name()] = toml_edit::value(path_str);

    // Ensure parent directory exists
    if let Some(parent) = config_file.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // Write back to file
    fs::write(&config_file, doc.to_string())?;

    Ok(())
}

/// Save the selected theme to the config file.
///
/// This updates the [theme] section, setting either "name" or "path"
/// and clearing the other to avoid ambiguity.
pub fn save_theme_config(name: Option<&str>, path: Option<&Path>) -> std::io::Result<()> {
    let config_file = config_path();

    // Read existing config or start with empty document
    let contents = if config_file.exists() {
        fs::read_to_string(&config_file)?
    } else {
        String::new()
    };

    // Parse as TOML document
    let mut doc: DocumentMut = contents
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    if name.is_none() && path.is_none() {
        if doc.contains_key("theme") {
            doc.remove("theme");
        }
    } else {
        // Ensure [theme] section exists
        if !doc.contains_key("theme") {
            doc["theme"] = Item::Table(Table::new());
        }

        if let Some(path) = path {
            let path_str = path.to_string_lossy().to_string();
            doc["theme"]["path"] = toml_edit::value(path_str);
            if let Item::Table(table) = &mut doc["theme"] {
                table.remove("name");
            }
        } else if let Some(name) = name {
            doc["theme"]["name"] = toml_edit::value(name);
            if let Item::Table(table) = &mut doc["theme"] {
                table.remove("path");
            }
        }
    }

    // Ensure parent directory exists
    if let Some(parent) = config_file.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&config_file, doc.to_string())?;

    Ok(())
}

/// Save the default model for an agent type to the config file.
///
/// This updates the [model] section, setting "agent" and "model".
pub fn save_default_model(agent_type: AgentType, model_id: &str) -> std::io::Result<()> {
    let config_file = config_path();

    // Read existing config or start with empty document
    let contents = if config_file.exists() {
        fs::read_to_string(&config_file)?
    } else {
        String::new()
    };

    // Parse as TOML document
    let mut doc: DocumentMut = contents
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Ensure [model] section exists
    if !doc.contains_key("model") {
        doc["model"] = Item::Table(Table::new());
    }

    doc["model"]["agent"] = toml_edit::value(agent_type.as_str());
    doc["model"]["model"] = toml_edit::value(model_id);

    // Ensure parent directory exists
    if let Some(parent) = config_file.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(&config_file, doc.to_string())?;

    Ok(())
}
