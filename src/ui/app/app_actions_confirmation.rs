use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::components::ConfirmationContext;
use crate::ui::effect::Effect;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_confirmation_action(
        &mut self,
        action: Action,
        effects: &mut Vec<Effect>,
    ) -> anyhow::Result<()> {
        match action {
            Action::ConfirmYes => {
                if self.state.input_mode == InputMode::Confirming {
                    if let Some(context) = self.state.confirmation_dialog_state.context.clone() {
                        match context {
                            ConfirmationContext::SelectWorkspaceMode { repo_id } => {
                                match self.apply_repo_workspace_mode(
                                    repo_id,
                                    crate::git::WorkspaceMode::Worktree,
                                ) {
                                    Ok(()) => {
                                        self.state.confirmation_dialog_state.hide();
                                        self.state.input_mode = InputMode::SidebarNavigation;
                                        if let Some(effect) = self.start_workspace_creation(repo_id)
                                        {
                                            effects.push(effect);
                                        }
                                    }
                                    Err(err) => {
                                        self.state.confirmation_dialog_state.hide();
                                        self.show_error("Unable to Set Workspace Mode", &err);
                                    }
                                }
                            }
                            ConfirmationContext::ArchiveWorkspace(id) => {
                                if let Some((workspace, settings, base_path)) =
                                    self.resolve_workspace_settings(id)
                                {
                                    if settings.archive_delete_branch
                                        && settings.archive_remote_prompt
                                    {
                                        let should_prompt = match base_path.as_ref() {
                                            Some(path) => match self
                                                .worktree_manager()
                                                .remote_branch_exists(path, &workspace.branch)
                                            {
                                                Ok(true) => true,
                                                Ok(false) => false,
                                                Err(err) => {
                                                    tracing::warn!(
                                                        error = %err,
                                                        workspace_id = %workspace.id,
                                                        branch = %workspace.branch,
                                                        "Failed to check remote branch existence"
                                                    );
                                                    false
                                                }
                                            },
                                            None => false,
                                        };
                                        if should_prompt {
                                            self.prompt_archive_remote_delete(&workspace);
                                            return Ok(());
                                        }
                                    }
                                } else {
                                    self.state.confirmation_dialog_state.hide();
                                    self.show_error(
                                        "Archive Failed",
                                        "Workspace or repository not found.",
                                    );
                                    return Ok(());
                                }
                                effects.push(self.execute_archive_workspace(id, false));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                            }
                            ConfirmationContext::ArchiveWorkspaceRemoteDelete { workspace_id } => {
                                effects.push(self.execute_archive_workspace(workspace_id, true));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                            }
                            ConfirmationContext::RemoveProject(id) => {
                                effects.push(self.execute_remove_project(id));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                            }
                            ConfirmationContext::CreatePullRequest {
                                tab_index,
                                working_dir,
                                preflight,
                            } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.submit_pr_workflow(
                                    tab_index,
                                    working_dir,
                                    preflight,
                                )?);
                            }
                            ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.push(Effect::OpenPrInBrowser { working_dir });
                            }
                            ConfirmationContext::SteerFallback { message_id } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.confirm_steer_fallback(message_id)?);
                            }
                            ConfirmationContext::ForkSession {
                                parent_workspace_id,
                                base_branch,
                            } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                if let Some(effect) =
                                    self.execute_fork_session(parent_workspace_id, base_branch)
                                {
                                    effects.push(effect);
                                }
                            }
                        }
                    }
                }
            }
            Action::ConfirmNo => {
                if self.state.input_mode == InputMode::Confirming {
                    if let Some(context) = self.state.confirmation_dialog_state.context.clone() {
                        match context {
                            ConfirmationContext::SelectWorkspaceMode { repo_id } => {
                                match self.apply_repo_workspace_mode(
                                    repo_id,
                                    crate::git::WorkspaceMode::Checkout,
                                ) {
                                    Ok(()) => {
                                        self.state.confirmation_dialog_state.hide();
                                        self.state.input_mode = InputMode::SidebarNavigation;
                                        if let Some(effect) = self.start_workspace_creation(repo_id)
                                        {
                                            effects.push(effect);
                                        }
                                    }
                                    Err(err) => {
                                        self.state.confirmation_dialog_state.hide();
                                        self.show_error("Unable to Set Workspace Mode", &err);
                                    }
                                }
                            }
                            ConfirmationContext::ArchiveWorkspaceRemoteDelete { workspace_id } => {
                                effects.push(self.execute_archive_workspace(workspace_id, false));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                            }
                            _ => {
                                self.state.input_mode = self.dismiss_confirmation_dialog();
                            }
                        }
                    } else {
                        self.state.input_mode = self.dismiss_confirmation_dialog();
                    }
                }
            }
            Action::ConfirmToggle => {
                if self.state.input_mode == InputMode::Confirming {
                    self.state.confirmation_dialog_state.toggle_selection();
                }
            }
            _ => {}
        }

        Ok(())
    }
}
