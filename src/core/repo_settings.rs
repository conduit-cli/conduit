use crate::config::Config;
use crate::data::Repository;
use crate::git::WorkspaceMode;

#[derive(Debug, Clone, Copy)]
pub struct RepoWorkspaceSettings {
    pub mode: WorkspaceMode,
    pub archive_delete_branch: bool,
    pub archive_remote_prompt: bool,
}

pub fn resolve_repo_workspace_settings(
    config: &Config,
    repo: &Repository,
) -> RepoWorkspaceSettings {
    RepoWorkspaceSettings {
        mode: repo.workspace_mode_or(config.workspaces.default_mode),
        archive_delete_branch: repo
            .archive_delete_branch_or(config.workspaces.archive_delete_branch),
        archive_remote_prompt: repo
            .archive_remote_prompt_or(config.workspaces.archive_remote_prompt),
    }
}
