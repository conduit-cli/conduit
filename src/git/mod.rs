//! Git operations module

mod pr;
mod status;
mod workspace_mode;
mod workspace_repo;
mod worktree;

pub use pr::{
    CheckState, CheckStatus, MergeReadiness, MergeableStatus, PrManager, PrPreflightResult,
    PrState, PrStatus, ReviewDecision,
};
pub use status::GitDiffStats;
pub use workspace_mode::WorkspaceMode;
pub use workspace_repo::WorkspaceRepoManager;
pub use worktree::{WorktreeInfo, WorktreeManager};
