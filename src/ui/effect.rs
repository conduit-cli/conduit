use std::path::PathBuf;

use crate::agent::{AgentStartConfig, AgentType};
use crate::session::ExternalSession;
use uuid::Uuid;

/// Side effects that should be executed outside the reducer.
pub enum Effect {
    SaveSessionState,
    StartAgent {
        session_id: Uuid,
        agent_type: AgentType,
        config: AgentStartConfig,
    },
    PrPreflight {
        tab_index: usize,
        working_dir: PathBuf,
    },
    OpenPrInBrowser {
        working_dir: PathBuf,
    },
    DumpDebugState,
    CreateWorkspace {
        repo_id: Uuid,
    },
    ForkWorkspace {
        parent_workspace_id: Uuid,
        base_branch: String,
    },
    ArchiveWorkspace {
        workspace_id: Uuid,
    },
    RemoveProject {
        repo_id: Uuid,
    },
    CopyToClipboard(String),
    /// Discover external sessions (Claude Code and Codex CLI; Gemini not supported yet)
    DiscoverSessions,
    /// Import an external session
    ImportSession(ExternalSession),
    /// Generate session title and branch name from first message
    GenerateTitleAndBranch {
        /// Stable session ID for correlation (avoids stale tab_index after close/reorder)
        session_id: Uuid,
        user_message: String,
        working_dir: PathBuf,
        workspace_id: Option<Uuid>,
        current_branch: String,
    },
    /// Run a local shell command
    RunShellCommand {
        session_id: Uuid,
        message_index: usize,
        command: String,
        working_dir: Option<PathBuf>,
    },
}
