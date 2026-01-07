//! Git operations module

mod pr;
mod status;
mod worktree;

pub use pr::{
    CheckState, CheckStatus, MergeReadiness, MergeableStatus, PrManager, PrPreflightResult,
    PrState, PrStatus, ReviewDecision,
};
pub use status::GitDiffStats;
pub use worktree::{WorktreeInfo, WorktreeManager};
