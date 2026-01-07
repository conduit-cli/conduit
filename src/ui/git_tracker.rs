//! Background git and PR status tracker
//!
//! Polls git status and PR information in the background without blocking the UI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::git::{GitDiffStats, PrManager, PrStatus};

/// Configuration for the background tracker
pub struct GitTrackerConfig {
    /// How often to poll for git status (default: 2 seconds)
    pub git_status_poll_interval: Duration,
    /// How often to poll for PR updates (default: 60 seconds)
    pub pr_poll_interval: Duration,
}

impl Default for GitTrackerConfig {
    fn default() -> Self {
        Self {
            git_status_poll_interval: Duration::from_secs(2),
            pr_poll_interval: Duration::from_secs(60),
        }
    }
}

/// State tracked per workspace
#[derive(Debug, Clone, Default)]
struct WorkspaceGitState {
    pr_status: Option<PrStatus>,
    diff_stats: GitDiffStats,
    branch_name: Option<String>,
    #[allow(dead_code)]
    last_pr_check: Option<Instant>,
    #[allow(dead_code)]
    last_git_check: Option<Instant>,
}

/// Updates sent from background tracker to UI
#[derive(Debug, Clone)]
pub enum GitTrackerUpdate {
    /// PR status changed for a workspace (None means PR was removed/closed)
    PrStatusChanged {
        workspace_id: Uuid,
        status: Option<PrStatus>,
    },
    /// Git diff stats changed for a workspace
    GitStatsChanged {
        workspace_id: Uuid,
        stats: GitDiffStats,
    },
    /// Branch name changed (None means detached HEAD)
    BranchChanged {
        workspace_id: Uuid,
        branch: Option<String>,
    },
}

/// Commands to the background tracker
#[derive(Debug)]
pub enum GitTrackerCommand {
    /// Register a workspace to track
    TrackWorkspace {
        workspace_id: Uuid,
        working_dir: PathBuf,
    },
    /// Stop tracking a workspace
    UntrackWorkspace { workspace_id: Uuid },
    /// Force immediate refresh for a workspace
    RefreshNow { workspace_id: Uuid },
    /// Shutdown the tracker
    Shutdown,
}

/// Handle to control the background tracker
#[derive(Clone)]
pub struct GitTrackerHandle {
    cmd_tx: mpsc::UnboundedSender<GitTrackerCommand>,
}

impl GitTrackerHandle {
    /// Track a workspace for git/PR status updates
    pub fn track_workspace(&self, workspace_id: Uuid, working_dir: PathBuf) {
        let _ = self.cmd_tx.send(GitTrackerCommand::TrackWorkspace {
            workspace_id,
            working_dir,
        });
    }

    /// Stop tracking a workspace
    pub fn untrack_workspace(&self, workspace_id: Uuid) {
        let _ = self
            .cmd_tx
            .send(GitTrackerCommand::UntrackWorkspace { workspace_id });
    }

    /// Force an immediate refresh for a workspace
    #[allow(dead_code)]
    pub fn refresh_now(&self, workspace_id: Uuid) {
        let _ = self
            .cmd_tx
            .send(GitTrackerCommand::RefreshNow { workspace_id });
    }

    /// Shutdown the tracker
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(GitTrackerCommand::Shutdown);
    }
}

/// Background git and PR status tracker
struct GitTracker {
    config: GitTrackerConfig,
    /// Tracked workspaces: workspace_id -> (working_dir, state)
    workspaces: HashMap<Uuid, (PathBuf, WorkspaceGitState)>,
    /// Receive commands from UI
    cmd_rx: mpsc::UnboundedReceiver<GitTrackerCommand>,
    /// Send updates to UI
    update_tx: mpsc::UnboundedSender<GitTrackerUpdate>,
}

impl GitTracker {
    /// Spawn the background tracker and return a handle to control it
    pub fn spawn(
        config: GitTrackerConfig,
        update_tx: mpsc::UnboundedSender<GitTrackerUpdate>,
    ) -> GitTrackerHandle {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let tracker = Self {
            config,
            workspaces: HashMap::new(),
            cmd_rx,
            update_tx,
        };

        tokio::spawn(tracker.run());

        GitTrackerHandle { cmd_tx }
    }

    /// Main loop for the background tracker
    async fn run(mut self) {
        let mut git_interval = tokio::time::interval(self.config.git_status_poll_interval);
        let mut pr_interval = tokio::time::interval(self.config.pr_poll_interval);

        // Skip the first immediate tick
        git_interval.tick().await;
        pr_interval.tick().await;

        loop {
            tokio::select! {
                // Handle commands
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        GitTrackerCommand::Shutdown => break,
                        GitTrackerCommand::TrackWorkspace { workspace_id, working_dir } => {
                            self.workspaces.insert(workspace_id, (working_dir.clone(), WorkspaceGitState::default()));
                            // Immediate check on registration
                            self.check_workspace(workspace_id, &working_dir).await;
                        }
                        GitTrackerCommand::UntrackWorkspace { workspace_id } => {
                            self.workspaces.remove(&workspace_id);
                        }
                        GitTrackerCommand::RefreshNow { workspace_id } => {
                            if let Some((working_dir, _)) = self.workspaces.get(&workspace_id).cloned() {
                                self.check_workspace(workspace_id, &working_dir).await;
                            }
                        }
                    }
                }
                // Git status polling (fast)
                _ = git_interval.tick() => {
                    self.poll_git_status().await;
                }
                // PR polling (slow)
                _ = pr_interval.tick() => {
                    self.poll_pr_status().await;
                }
            }
        }
    }

    /// Check git status for all tracked workspaces
    ///
    /// TODO: Consider parallel polling with join_all for many workspaces
    async fn poll_git_status(&mut self) {
        let workspace_ids: Vec<_> = self.workspaces.keys().copied().collect();

        for workspace_id in workspace_ids {
            if let Some((working_dir, state)) = self.workspaces.get_mut(&workspace_id) {
                let dir = working_dir.clone();

                // Get git diff stats using spawn_blocking
                let new_stats =
                    tokio::task::spawn_blocking(move || GitDiffStats::from_working_dir(&dir))
                        .await
                        .unwrap_or_default();

                // Only send update if stats changed
                if new_stats != state.diff_stats {
                    state.diff_stats = new_stats.clone();
                    state.last_git_check = Some(Instant::now());
                    let _ = self.update_tx.send(GitTrackerUpdate::GitStatsChanged {
                        workspace_id,
                        stats: new_stats,
                    });
                }

                // Also check branch name
                let dir = working_dir.clone();
                let new_branch =
                    tokio::task::spawn_blocking(move || PrManager::get_current_branch(&dir))
                        .await
                        .ok()
                        .flatten();

                if new_branch != state.branch_name {
                    state.branch_name = new_branch.clone();
                    // Always send update (including None for detached HEAD)
                    let _ = self.update_tx.send(GitTrackerUpdate::BranchChanged {
                        workspace_id,
                        branch: new_branch,
                    });
                }
            }
        }
    }

    /// Check PR status for all tracked workspaces
    async fn poll_pr_status(&mut self) {
        let workspace_ids: Vec<_> = self.workspaces.keys().copied().collect();

        for workspace_id in workspace_ids {
            if let Some((working_dir, state)) = self.workspaces.get_mut(&workspace_id) {
                let dir = working_dir.clone();

                // Get PR status using spawn_blocking
                let new_pr_status =
                    tokio::task::spawn_blocking(move || PrManager::get_existing_pr(&dir))
                        .await
                        .ok()
                        .flatten();

                // Compare with current state
                let pr_changed = match (&state.pr_status, &new_pr_status) {
                    (None, Some(_)) => true,
                    (Some(_), None) => true,
                    (Some(old), Some(new)) => {
                        old.number != new.number
                            || old.state != new.state
                            || old.exists != new.exists
                    }
                    (None, None) => false,
                };

                if pr_changed {
                    state.pr_status = new_pr_status.clone();
                    state.last_pr_check = Some(Instant::now());
                    // Always send update (including None to clear PR badge)
                    let _ = self.update_tx.send(GitTrackerUpdate::PrStatusChanged {
                        workspace_id,
                        status: new_pr_status,
                    });
                }
            }
        }
    }

    /// Check a single workspace immediately (both git and PR)
    async fn check_workspace(&mut self, workspace_id: Uuid, working_dir: &Path) {
        let dir = working_dir.to_path_buf();

        // Get git diff stats
        let new_stats = tokio::task::spawn_blocking({
            let dir = dir.clone();
            move || GitDiffStats::from_working_dir(&dir)
        })
        .await
        .unwrap_or_default();

        // Get branch name
        let new_branch = tokio::task::spawn_blocking({
            let dir = dir.clone();
            move || PrManager::get_current_branch(&dir)
        })
        .await
        .ok()
        .flatten();

        // Get PR status
        let new_pr_status = tokio::task::spawn_blocking(move || PrManager::get_existing_pr(&dir))
            .await
            .ok()
            .flatten();

        // Update state and send updates
        if let Some((_, state)) = self.workspaces.get_mut(&workspace_id) {
            state.diff_stats = new_stats.clone();
            state.branch_name = new_branch.clone();
            state.pr_status = new_pr_status.clone();
            state.last_git_check = Some(Instant::now());
            state.last_pr_check = Some(Instant::now());
        }

        // Send all updates
        let _ = self.update_tx.send(GitTrackerUpdate::GitStatsChanged {
            workspace_id,
            stats: new_stats,
        });

        // Always send branch update (including None for detached HEAD)
        let _ = self.update_tx.send(GitTrackerUpdate::BranchChanged {
            workspace_id,
            branch: new_branch,
        });

        // Always send PR update (including None to clear)
        let _ = self.update_tx.send(GitTrackerUpdate::PrStatusChanged {
            workspace_id,
            status: new_pr_status,
        });
    }
}

/// Spawn the git tracker and return a handle
pub fn spawn_git_tracker(update_tx: mpsc::UnboundedSender<GitTrackerUpdate>) -> GitTrackerHandle {
    GitTracker::spawn(GitTrackerConfig::default(), update_tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tracker_spawn_shutdown() {
        let (update_tx, mut update_rx) = mpsc::unbounded_channel();
        let handle = spawn_git_tracker(update_tx);

        // Shutdown should work
        handle.shutdown();

        // Give it a moment to shut down
        tokio::time::sleep(Duration::from_millis(50)).await;

        // No updates should be pending (or channel should be dropped)
        assert!(update_rx.try_recv().is_err());
    }
}
