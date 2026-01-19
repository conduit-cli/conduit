//! Data models for repositories and workspaces

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::agent::AgentType;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QueuedMessageMode {
    Steer,
    FollowUp,
}

impl QueuedMessageMode {
    pub fn label(&self) -> &'static str {
        match self {
            QueuedMessageMode::Steer => "Steering",
            QueuedMessageMode::FollowUp => "Queued",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedImageAttachment {
    pub path: PathBuf,
    pub placeholder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedMessage {
    pub id: Uuid,
    pub mode: QueuedMessageMode,
    pub text: String,
    pub images: Vec<QueuedImageAttachment>,
    pub created_at: DateTime<Utc>,
}

/// Represents a git repository that can have multiple workspaces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    /// Unique identifier
    pub id: Uuid,
    /// Display name for the repository
    pub name: String,
    /// Path to the base repository (original checkout)
    pub base_path: Option<PathBuf>,
    /// Remote repository URL (for future cloning support)
    pub repository_url: Option<String>,
    /// When the repository was added
    pub created_at: DateTime<Utc>,
    /// Last time the repository was modified
    pub updated_at: DateTime<Utc>,
}

impl Repository {
    /// Create a new repository from a local path
    pub fn from_local_path(name: impl Into<String>, base_path: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            base_path: Some(base_path),
            repository_url: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a new repository from a remote URL (for future use)
    pub fn from_url(name: impl Into<String>, url: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            base_path: None,
            repository_url: Some(url.into()),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Represents a workspace (git worktree) within a repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// Unique identifier
    pub id: Uuid,
    /// Parent repository ID
    pub repository_id: Uuid,
    /// Display name for the workspace
    pub name: String,
    /// Git branch this workspace is on
    pub branch: String,
    /// Path to the worktree directory
    pub path: PathBuf,
    /// When the workspace was created
    pub created_at: DateTime<Utc>,
    /// Last time the workspace was accessed
    pub last_accessed: DateTime<Utc>,
    /// Whether this is the default/main workspace
    pub is_default: bool,
    /// When the workspace was archived (None = active)
    pub archived_at: Option<DateTime<Utc>>,
    /// Commit SHA at the time of archive (if recorded)
    pub archived_commit_sha: Option<String>,
}

impl Workspace {
    /// Create a new workspace
    pub fn new(
        repository_id: Uuid,
        name: impl Into<String>,
        branch: impl Into<String>,
        path: PathBuf,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            repository_id,
            name: name.into(),
            branch: branch.into(),
            path,
            created_at: now,
            last_accessed: now,
            is_default: false,
            archived_at: None,
            archived_commit_sha: None,
        }
    }

    /// Create a default workspace (for the main branch)
    pub fn new_default(
        repository_id: Uuid,
        name: impl Into<String>,
        branch: impl Into<String>,
        path: PathBuf,
    ) -> Self {
        let mut ws = Self::new(repository_id, name, branch, path);
        ws.is_default = true;
        ws
    }

    /// Update the last accessed timestamp
    pub fn touch(&mut self) {
        self.last_accessed = Utc::now();
    }

    /// Check if this workspace is archived
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

/// Represents a saved session tab for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTab {
    /// Unique identifier
    pub id: Uuid,
    /// Tab index (ordering)
    pub tab_index: i32,
    /// Whether this session tab is currently open
    pub is_open: bool,
    /// Associated workspace ID (optional)
    pub workspace_id: Option<Uuid>,
    /// Agent type (Claude, Codex, or Gemini)
    pub agent_type: AgentType,
    /// Agent mode (Build or Plan)
    pub agent_mode: Option<String>,
    /// Agent session ID (for resume and history loading)
    pub agent_session_id: Option<String>,
    /// Selected model
    pub model: Option<String>,
    /// PR number if a PR exists for this session's branch
    pub pr_number: Option<i32>,
    /// When the tab was created
    pub created_at: DateTime<Utc>,
    /// Pending user message that hasn't been confirmed by agent yet
    pub pending_user_message: Option<String>,
    /// Queued messages waiting to be delivered
    pub queued_messages: Vec<QueuedMessage>,
    /// Input history for arrow-up restoration
    pub input_history: Vec<String>,
    /// Fork seed ID (if this tab was created via fork)
    pub fork_seed_id: Option<Uuid>,
    /// AI-generated session title/description
    pub title: Option<String>,
}

impl SessionTab {
    /// Create a new session tab
    pub fn new(
        tab_index: i32,
        agent_type: AgentType,
        workspace_id: Option<Uuid>,
        agent_session_id: Option<String>,
        model: Option<String>,
        pr_number: Option<i32>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tab_index,
            is_open: true,
            workspace_id,
            agent_type,
            agent_mode: None,
            agent_session_id,
            model,
            pr_number,
            created_at: Utc::now(),
            pending_user_message: None,
            queued_messages: Vec::new(),
            input_history: Vec::new(),
            fork_seed_id: None,
            title: None,
        }
    }
}

/// Metadata for a forked session seed prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkSeed {
    /// Unique identifier
    pub id: Uuid,
    /// Agent type (Claude, Codex, or Gemini)
    pub agent_type: AgentType,
    /// Parent agent session ID
    pub parent_session_id: Option<String>,
    /// Parent workspace ID
    pub parent_workspace_id: Option<Uuid>,
    /// When the fork seed was created
    pub created_at: DateTime<Utc>,
    /// Hash of the seed prompt (no raw transcript stored)
    pub seed_prompt_hash: String,
    /// Optional path or pointer to a stored seed prompt (if configured)
    pub seed_prompt_path: Option<String>,
    /// Estimated tokens for the seed prompt
    pub token_estimate: i64,
    /// Context window size for the model at fork time
    pub context_window: i64,
    /// Whether the first assistant reply should be suppressed
    pub seed_ack_filtered: bool,
}

impl ForkSeed {
    pub fn new(
        agent_type: AgentType,
        parent_session_id: Option<String>,
        parent_workspace_id: Option<Uuid>,
        seed_prompt_hash: String,
        seed_prompt_path: Option<String>,
        token_estimate: i64,
        context_window: i64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            agent_type,
            parent_session_id,
            parent_workspace_id,
            created_at: Utc::now(),
            seed_prompt_hash,
            seed_prompt_path,
            token_estimate,
            context_window,
            seed_ack_filtered: true,
        }
    }
}
