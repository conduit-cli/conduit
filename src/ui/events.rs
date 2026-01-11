use std::path::PathBuf;

use crate::agent::{AgentEvent, AgentType};
use crate::git::PrPreflightResult;
use crate::ui::git_tracker::GitTrackerUpdate;
use uuid::Uuid;

/// Application-level events
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Terminal input event
    Input(crossterm::event::Event),

    /// Agent event from a session
    Agent { tab_index: usize, event: AgentEvent },

    /// Agent event stream ended (process exited)
    AgentStreamEnded { tab_index: usize },

    /// Agent subprocess started with given PID
    AgentStarted { tab_index: usize, pid: u32 },

    /// User submitted a prompt
    PromptSubmit { tab_index: usize, prompt: String },

    /// Request to create a new tab
    NewTab(AgentType),

    /// Request to close a tab
    CloseTab(usize),

    /// Request to switch to a tab
    SwitchTab(usize),

    /// Agent selection dialog requested
    ShowAgentSelector,

    /// Agent selected from dialog
    AgentSelected(AgentType),

    /// Request to interrupt current agent
    InterruptAgent(usize),

    /// Toggle sidebar visibility
    ToggleSidebar,

    /// Open a workspace (creates/switches to tab)
    OpenWorkspace(Uuid),

    /// Refresh sidebar data from database
    RefreshSidebar,

    /// Tick event for animations/updates
    Tick,

    /// Request to quit the application
    Quit,

    /// Error occurred
    Error(String),

    /// PR preflight check completed
    PrPreflightCompleted {
        tab_index: usize,
        working_dir: PathBuf,
        result: PrPreflightResult,
    },

    /// Open PR in browser completed
    OpenPrCompleted { result: Result<(), String> },

    /// Debug export completed
    DebugDumped { result: Result<String, String> },

    /// Workspace creation completed
    WorkspaceCreated {
        result: Result<WorkspaceCreated, String>,
    },
    /// Fork workspace creation completed
    ForkWorkspaceCreated {
        result: Result<ForkWorkspaceCreated, String>,
    },

    /// Workspace archive completed
    WorkspaceArchived {
        result: Result<WorkspaceArchived, String>,
    },

    /// Project removal completed
    ProjectRemoved { result: RemoveProjectResult },

    /// Cached sessions loaded (fast path from disk cache)
    SessionsCacheLoaded {
        sessions: Vec<crate::session::ExternalSession>,
    },

    /// Single session updated during background refresh
    SessionUpdated {
        session: crate::session::ExternalSession,
    },

    /// Session removed (file no longer exists)
    SessionRemoved { file_path: PathBuf },

    /// Background session discovery complete
    SessionDiscoveryComplete,

    /// Git tracker update (PR status, git stats, branch changes)
    GitTracker(GitTrackerUpdate),

    /// Title/branch generation completed
    TitleGenerated {
        /// Stable session ID for correlation (avoids stale tab_index after close/reorder)
        session_id: Uuid,
        result: Result<TitleGeneratedResult, String>,
    },
}

/// Result of successful title/branch generation
#[derive(Debug, Clone)]
pub struct TitleGeneratedResult {
    /// AI-generated session title
    pub title: String,
    /// New branch name (None if rename failed/skipped)
    pub new_branch: Option<String>,
    /// Associated workspace ID
    pub workspace_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceCreated {
    pub repo_id: Uuid,
    pub workspace_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ForkWorkspaceCreated {
    pub repo_id: Uuid,
    pub workspace_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct WorkspaceArchived {
    pub workspace_id: Uuid,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RemoveProjectResult {
    pub repo_id: Uuid,
    pub workspace_ids: Vec<Uuid>,
    pub errors: Vec<String>,
}

/// Input mode for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal mode - input focused
    #[default]
    Normal,
    /// Selecting agent for new tab
    SelectingAgent,
    /// Scrolling through chat history
    Scrolling,
    /// Navigating sidebar
    SidebarNavigation,
    /// Adding a repository (custom path)
    AddingRepository,
    /// Selecting model for current session
    SelectingModel,
    /// Selecting theme
    SelectingTheme,
    /// Setting base projects directory
    SettingBaseDir,
    /// Picking a project from the list
    PickingProject,
    /// Showing a confirmation dialog
    Confirming,
    /// Removing a project (showing spinner)
    RemovingProject,
    /// Showing an error dialog
    ShowingError,
    /// Command mode (typing :command)
    Command,
    /// Showing help dialog
    ShowingHelp,
    /// Importing a session from external agent
    ImportingSession,
    /// Command palette is open
    CommandPalette,
    /// Missing tool dialog is open
    MissingTool,
    /// Editing queued messages inline
    QueueEditing,
}

/// View mode for the main content area
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Standard chat view
    #[default]
    Chat,
    /// Raw events debug view
    RawEvents,
}
