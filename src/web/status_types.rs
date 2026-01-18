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
    pub merge_readiness: Option<String>,
    pub checks_total: Option<usize>,
    pub checks_passed: Option<usize>,
    pub checks_failed: Option<usize>,
    pub checks_pending: Option<usize>,
    pub checks_skipped: Option<usize>,
    pub mergeable: Option<String>,
    pub review_decision: Option<String>,
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
            merge_readiness: Some(
                match pr.merge_readiness {
                    crate::git::MergeReadiness::Ready => "ready",
                    crate::git::MergeReadiness::Blocked => "blocked",
                    crate::git::MergeReadiness::HasConflicts => "has_conflicts",
                    crate::git::MergeReadiness::Unknown => "unknown",
                }
                .to_string(),
            ),
            checks_total: Some(pr.checks.total),
            checks_passed: Some(pr.checks.passed),
            checks_failed: Some(pr.checks.failed),
            checks_pending: Some(pr.checks.pending),
            checks_skipped: Some(pr.checks.skipped),
            mergeable: Some(
                match pr.mergeable {
                    crate::git::MergeableStatus::Mergeable => "mergeable",
                    crate::git::MergeableStatus::Conflicting => "conflicting",
                    crate::git::MergeableStatus::Unknown => "unknown",
                }
                .to_string(),
            ),
            review_decision: Some(
                match pr.review_decision {
                    crate::git::ReviewDecision::Approved => "approved",
                    crate::git::ReviewDecision::ReviewRequired => "review_required",
                    crate::git::ReviewDecision::ChangesRequested => "changes_requested",
                    crate::git::ReviewDecision::None => "none",
                }
                .to_string(),
            ),
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
