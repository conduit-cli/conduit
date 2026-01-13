use crate::agent::MessageDisplay;
use crate::ui::app::App;
use crate::ui::components::ConfirmationContext;
use crate::ui::effect::Effect;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_confirm_action(
        &mut self,
        effects: &mut Vec<Effect>,
    ) -> anyhow::Result<()> {
        match self.state.input_mode {
            InputMode::SidebarNavigation => {
                let selected = self.state.sidebar_state.tree_state.selected;
                if let Some(node) = self.state.sidebar_data.get_at(selected) {
                    use crate::ui::components::{ActionType, NodeType};
                    match node.node_type {
                        NodeType::Action(ActionType::NewWorkspace) => {
                            if let Some(parent_id) = node.parent_id {
                                effects.push(self.start_workspace_creation(parent_id));
                            }
                        }
                        NodeType::Workspace => {
                            self.open_workspace(node.id);
                            self.state.input_mode = InputMode::Normal;
                            self.state.sidebar_state.set_focused(false);
                        }
                        NodeType::Repository => {
                            self.state.sidebar_data.toggle_at(selected);
                        }
                    }
                }
            }
            InputMode::SelectingModel => {
                if let Some(model) = self.state.model_selector_state.selected_model() {
                    let model_id = model.id.clone();
                    let agent_type = model.agent_type;
                    let display_name = model.display_name.clone();
                    let required_tool = Self::required_tool(agent_type);
                    if !self.tools.is_available(required_tool) {
                        self.show_missing_tool(
                            required_tool,
                            format!(
                                "{} is required to use this model.",
                                required_tool.display_name()
                            ),
                        );
                        return Ok(());
                    }
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        let agent_changed =
                            session.set_agent_and_model(agent_type, Some(model_id.clone()));
                        let msg = if agent_changed {
                            format!("Switched to {} with model: {}", agent_type, display_name)
                        } else {
                            format!("Model changed to: {}", display_name)
                        };
                        let display = MessageDisplay::System { content: msg };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
                self.state.model_selector_state.hide();
                self.state.input_mode = InputMode::Normal;
            }
            InputMode::SelectingTheme => {
                effects.extend(self.confirm_theme_picker()?);
            }
            InputMode::SelectingAgent => {
                let agent_type = self.state.agent_selector_state.selected_agent();
                self.state.agent_selector_state.hide();
                self.create_tab_with_agent(agent_type);
            }
            InputMode::PickingProject => {
                if let Some(project) = self.state.project_picker_state.selected_project() {
                    let repo_id = self.add_project_to_sidebar(project.path.clone());
                    self.state.project_picker_state.hide();
                    if let Some(id) = repo_id {
                        self.state.sidebar_data.expand_repo(id);
                        if let Some(repo_index) = self.state.sidebar_data.find_repo_index(id) {
                            self.state.sidebar_state.tree_state.selected = repo_index + 1;
                        }
                        self.state.sidebar_state.show();
                        self.state.sidebar_state.set_focused(true);
                        self.state.show_first_time_splash = false;
                        self.state.input_mode = InputMode::SidebarNavigation;
                    } else {
                        self.state.input_mode = InputMode::Normal;
                    }
                }
            }
            InputMode::AddingRepository => {
                if self.state.add_repo_dialog_state.is_valid() {
                    let repo_id = self.add_repository();
                    self.state.add_repo_dialog_state.hide();
                    if let Some(id) = repo_id {
                        self.state.sidebar_data.expand_repo(id);
                        if let Some(repo_index) = self.state.sidebar_data.find_repo_index(id) {
                            self.state.sidebar_state.tree_state.selected = repo_index + 1;
                        }
                        self.state.sidebar_state.show();
                        self.state.sidebar_state.set_focused(true);
                        self.state.show_first_time_splash = false;
                        self.state.input_mode = InputMode::SidebarNavigation;
                    } else {
                        self.state.input_mode = InputMode::Normal;
                    }
                }
            }
            InputMode::SettingBaseDir => {
                if self.state.base_dir_dialog_state.is_valid() {
                    if let Some(dao) = &self.app_state_dao {
                        if let Err(e) = dao.set(
                            "projects_base_dir",
                            self.state.base_dir_dialog_state.input(),
                        ) {
                            self.state.base_dir_dialog_state.hide();
                            self.show_error(
                                "Failed to Save",
                                &format!("Could not save projects directory: {}", e),
                            );
                            return Ok(());
                        }
                    }
                    let base_path = self.state.base_dir_dialog_state.expanded_path();
                    self.state.base_dir_dialog_state.hide();
                    self.state.close_overlays();
                    self.state.project_picker_state.show(base_path);
                    self.state.input_mode = InputMode::PickingProject;
                }
            }
            InputMode::Confirming => {
                if self.state.confirmation_dialog_state.is_confirm_selected() {
                    if let Some(context) = self.state.confirmation_dialog_state.context.clone() {
                        match context {
                            ConfirmationContext::ArchiveWorkspace(id) => {
                                effects.push(self.execute_archive_workspace(id));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                                return Ok(());
                            }
                            ConfirmationContext::RemoveProject(id) => {
                                effects.push(self.execute_remove_project(id));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                                return Ok(());
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
                                return Ok(());
                            }
                            ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.push(Effect::OpenPrInBrowser { working_dir });
                                return Ok(());
                            }
                            ConfirmationContext::SteerFallback { message_id } => {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.confirm_steer_fallback(message_id)?);
                                return Ok(());
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
                                return Ok(());
                            }
                        }
                    }
                }
                // Cancel selected - dismiss the confirmation dialog
                self.state.input_mode = self.dismiss_confirmation_dialog();
            }
            InputMode::ShowingError => {
                self.state.error_dialog_state.hide();
                self.state.input_mode = InputMode::Normal;
            }
            InputMode::MissingTool => {
                // Validate and save the path
                if let Some(result) = self.state.missing_tool_dialog_state.validate() {
                    use crate::ui::components::MissingToolResult;
                    match result {
                        MissingToolResult::PathProvided(path) => {
                            let tool = self.state.missing_tool_dialog_state.tool;
                            // Update ToolAvailability
                            self.tools.update_tool(tool, path.clone());
                            // Save to config
                            if let Err(e) = crate::config::save_tool_path(tool, &path) {
                                tracing::warn!("Failed to save tool path to config: {}", e);
                            }
                            self.refresh_runners();
                            self.state.missing_tool_dialog_state.hide();
                            self.state.input_mode = InputMode::Normal;
                        }
                        MissingToolResult::Skipped | MissingToolResult::Quit => {
                            self.state.missing_tool_dialog_state.hide();
                            self.state.input_mode = InputMode::Normal;
                        }
                    }
                }
                // If validation failed, error is set in state and we stay in dialog
            }
            _ => {}
        }

        Ok(())
    }
}
