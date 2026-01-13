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
                            ConfirmationContext::ArchiveWorkspace(id) => {
                                effects.push(self.execute_archive_workspace(id));
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
                    self.state.input_mode = self.dismiss_confirmation_dialog();
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
