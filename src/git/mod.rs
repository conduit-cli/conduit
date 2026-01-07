//! Git operations module

mod pr;
mod status;
mod worktree;

pub use pr::{PrManager, PrPreflightResult, PrState, PrStatus};
pub use status::GitDiffStats;
pub use worktree::{WorktreeInfo, WorktreeManager};
