//! Shared status response types for workspace git/PR info.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::git::{CheckState, GitDiffStats, PrState, PrStatus};

/// Response for git diff statistics.
#[derive(Debug, Serialize, Clone, Default)]
pub struct GitDiffStatsResponse {
    pub additions: usize,
    pub deletions: usize,
    pub files_changed: usize,
}

impl From<GitDiffStats> for GitDiffStatsResponse {
    fn from(stats: GitDiffStats) -> Self {
        Self {
            additions: stats.additions,
            deletions: stats.deletions,
            files_changed: stats.files_changed,
        }
    }
}

/// Response for PR status.
#[derive(Debug, Serialize, Clone)]
pub struct PrStatusResponse {
    pub number: u32,
    pub state: String,
    pub checks_passing: bool,
    pub url: Option<String>,
}

impl PrStatusResponse {
    pub fn from_pr_status(pr: &PrStatus) -> Option<Self> {
        if !pr.exists {
            return None;
        }

        let number = pr.number?;
        let state = match pr.state {
            PrState::Open => "open".to_string(),
            PrState::Merged => "merged".to_string(),
            PrState::Closed => "closed".to_string(),
            PrState::Draft => "draft".to_string(),
            PrState::Unknown => "unknown".to_string(),
        };

        Some(Self {
            number,
            state,
            checks_passing: matches!(pr.checks.state(), CheckState::Passing),
            url: pr.url.clone(),
        })
    }
}

/// Response for workspace git/PR status.
#[derive(Debug, Serialize, Clone, Default)]
pub struct WorkspaceStatusResponse {
    pub git_stats: Option<GitDiffStatsResponse>,
    pub pr_status: Option<PrStatusResponse>,
    pub updated_at: Option<DateTime<Utc>>,
}
