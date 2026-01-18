//! Background workspace status manager for the web UI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::Config;
use crate::data::Workspace;
use crate::git::{GitDiffStats, PrManager};
use crate::web::status_types::{GitDiffStatsResponse, PrStatusResponse, WorkspaceStatusResponse};

#[derive(Debug, Clone)]
pub struct StatusManagerConfig {
    pub initial_scan: bool,
    pub concurrency: usize,
    pub selected_refresh_interval: Duration,
    pub pr_refresh_interval: Duration,
}

impl StatusManagerConfig {
    pub fn from_config(config: &Config) -> Self {
        let web_status = &config.web_status;
        Self {
            initial_scan: web_status.initial_scan,
            concurrency: web_status.status_scan_concurrency.max(1),
            selected_refresh_interval: Duration::from_millis(
                web_status.selected_refresh_interval_ms,
            ),
            pr_refresh_interval: Duration::from_millis(web_status.pr_refresh_interval_ms),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RefreshPlan {
    force_git: bool,
    force_pr: bool,
}

impl RefreshPlan {
    fn initial() -> Self {
        Self {
            force_git: true,
            force_pr: true,
        }
    }

    fn active_tick() -> Self {
        Self {
            force_git: true,
            force_pr: false,
        }
    }

    fn force_all() -> Self {
        Self {
            force_git: true,
            force_pr: true,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct WorkspaceEntry {
    path: PathBuf,
    status: WorkspaceStatusResponse,
    last_git_at: Option<Instant>,
    last_pr_at: Option<Instant>,
    refresh_generation: u64,
    in_flight: Option<CancellationToken>,
}

struct StatusManagerInner {
    config: StatusManagerConfig,
    workspaces: Mutex<HashMap<Uuid, WorkspaceEntry>>,
    active_workspace: Mutex<Option<Uuid>>,
    semaphore: Arc<Semaphore>,
    initial_scan_started: AtomicBool,
}

#[derive(Clone)]
pub struct StatusManager {
    inner: Arc<StatusManagerInner>,
}

impl StatusManager {
    pub fn new(config: StatusManagerConfig) -> Self {
        let inner = Arc::new(StatusManagerInner {
            config: config.clone(),
            workspaces: Mutex::new(HashMap::new()),
            active_workspace: Mutex::new(None),
            semaphore: Arc::new(Semaphore::new(config.concurrency)),
            initial_scan_started: AtomicBool::new(false),
        });

        Self::spawn_active_refresh_loop(inner.clone());

        Self { inner }
    }

    fn spawn_active_refresh_loop(inner: Arc<StatusManagerInner>) {
        let interval = inner.config.selected_refresh_interval;
        if interval == Duration::from_millis(0) {
            return;
        }

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let active = *inner.active_workspace.lock();
                if let Some(workspace_id) = active {
                    Self::schedule_refresh(inner.clone(), workspace_id, RefreshPlan::active_tick());
                }
            }
        });
    }

    pub fn kick_initial_scan(&self, workspaces: Vec<Workspace>) {
        if !self.inner.config.initial_scan {
            return;
        }

        if self.inner.initial_scan_started.swap(true, Ordering::SeqCst) {
            return;
        }

        for workspace in workspaces {
            self.register_workspace(workspace.id, workspace.path);
            Self::schedule_refresh(self.inner.clone(), workspace.id, RefreshPlan::initial());
        }
    }

    pub fn register_workspace(&self, workspace_id: Uuid, path: PathBuf) {
        let mut workspaces = self.inner.workspaces.lock();
        if let Some(entry) = workspaces.get_mut(&workspace_id) {
            entry.path = path;
            return;
        }

        workspaces.insert(
            workspace_id,
            WorkspaceEntry {
                path,
                status: WorkspaceStatusResponse::default(),
                last_git_at: None,
                last_pr_at: None,
                refresh_generation: 0,
                in_flight: None,
            },
        );
    }

    pub fn remove_workspace(&self, workspace_id: Uuid) {
        let mut workspaces = self.inner.workspaces.lock();
        if let Some(entry) = workspaces.remove(&workspace_id) {
            if let Some(token) = entry.in_flight {
                token.cancel();
            }
        }
    }

    pub fn set_active_workspace(&self, workspace_id: Option<Uuid>) {
        *self.inner.active_workspace.lock() = workspace_id;
        if let Some(workspace_id) = workspace_id {
            Self::schedule_refresh(self.inner.clone(), workspace_id, RefreshPlan::force_all());
        }
    }

    pub fn refresh_workspace(&self, workspace_id: Uuid) {
        Self::schedule_refresh(self.inner.clone(), workspace_id, RefreshPlan::force_all());
    }

    pub fn get_status(&self, workspace_id: Uuid) -> Option<WorkspaceStatusResponse> {
        let workspaces = self.inner.workspaces.lock();
        workspaces
            .get(&workspace_id)
            .map(|entry| entry.status.clone())
    }

    fn schedule_refresh(inner: Arc<StatusManagerInner>, workspace_id: Uuid, plan: RefreshPlan) {
        let now = Instant::now();
        let (path, token, generation, do_git, do_pr) = {
            let mut workspaces = inner.workspaces.lock();
            let entry = match workspaces.get_mut(&workspace_id) {
                Some(entry) => entry,
                None => return,
            };

            let git_due = entry
                .last_git_at
                .map(|ts| now.duration_since(ts) >= inner.config.selected_refresh_interval)
                .unwrap_or(true);
            let pr_due = entry
                .last_pr_at
                .map(|ts| now.duration_since(ts) >= inner.config.pr_refresh_interval)
                .unwrap_or(true);

            let do_git = plan.force_git || git_due;
            let do_pr = plan.force_pr || pr_due;
            if !do_git && !do_pr {
                return;
            }

            if let Some(token) = entry.in_flight.take() {
                token.cancel();
            }

            entry.refresh_generation = entry.refresh_generation.saturating_add(1);
            let generation = entry.refresh_generation;
            let token = CancellationToken::new();
            entry.in_flight = Some(token.clone());

            (entry.path.clone(), token, generation, do_git, do_pr)
        };

        tokio::spawn(async move {
            let permit = inner.semaphore.acquire().await;
            let _permit = match permit {
                Ok(permit) => permit,
                Err(err) => {
                    tracing::warn!(error = %err, "Status refresh semaphore closed");
                    return;
                }
            };

            if token.is_cancelled() {
                return;
            }

            let git_stats = if do_git {
                let path = path.clone();
                match tokio::task::spawn_blocking(move || GitDiffStats::from_working_dir(&path))
                    .await
                {
                    Ok(stats) => {
                        if stats.has_changes() {
                            Some(GitDiffStatsResponse::from(stats))
                        } else {
                            None
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "Git status task failed");
                        None
                    }
                }
            } else {
                None
            };

            if token.is_cancelled() {
                return;
            }

            let pr_status = if do_pr {
                let path = path.clone();
                match tokio::task::spawn_blocking(move || {
                    let gh_status = PrManager::gh_status();
                    if !gh_status.installed || !gh_status.authenticated {
                        return None;
                    }

                    PrManager::get_existing_pr(&path)
                        .and_then(|pr| PrStatusResponse::from_pr_status(&pr))
                })
                .await
                {
                    Ok(status) => status,
                    Err(err) => {
                        tracing::warn!(error = %err, "PR status task failed");
                        None
                    }
                }
            } else {
                None
            };

            if token.is_cancelled() {
                return;
            }

            let mut workspaces = inner.workspaces.lock();
            let entry = match workspaces.get_mut(&workspace_id) {
                Some(entry) => entry,
                None => return,
            };

            if entry.refresh_generation != generation {
                return;
            }

            let now = Instant::now();
            if do_git {
                entry.status.git_stats = git_stats;
                entry.last_git_at = Some(now);
            }
            if do_pr {
                entry.status.pr_status = pr_status;
                entry.last_pr_at = Some(now);
            }
            entry.status.updated_at = Some(Utc::now());
            entry.in_flight = None;
        });
    }
}
