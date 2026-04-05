use crate::agent::{AgentInput, AgentType, MessageDisplay};
use crate::ui::app::App;
use crate::ui::app_state::ModelPickerContext;
use crate::ui::components::ConfirmationContext;
use crate::ui::effect::Effect;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_confirm_action(
        &mut self,
        effects: &mut Vec<Effect>,
    ) -> anyhow::Result<()> {
        // Defensive normalization: if visibility and input mode diverge, confirm the
        // top-most visible modal instead of the stale input mode.
        if self.state.model_selector_state.is_visible()
            && self.state.input_mode != InputMode::SelectingModel
        {
            self.state.input_mode = InputMode::SelectingModel;
        } else if self.state.provider_selector_state.is_visible()
            && self.state.input_mode != InputMode::SelectingProviders
        {
            self.state.input_mode = InputMode::SelectingProviders;
        } else if self.state.settings_menu_state.is_visible()
            && self.state.input_mode != InputMode::SettingsMenu
        {
            self.state.input_mode = InputMode::SettingsMenu;
        } else if self.state.workspace_defaults_dialog_state.is_visible()
            && self.state.input_mode != InputMode::WorkspaceDefaults
        {
            self.state.input_mode = InputMode::WorkspaceDefaults;
        }

        match self.state.input_mode {
            InputMode::SidebarNavigation => {
                let selected = self.state.sidebar_state.tree_state.selected;
                if let Some(node) = self.state.sidebar_data.get_at(selected) {
                    use crate::ui::components::{ActionType, NodeType};
                    match node.node_type {
                        NodeType::Action(ActionType::NewWorkspace) => {
                            if let Some(parent_id) = node.parent_id {
                                if let Some(effect) = self.start_workspace_creation(parent_id) {
                                    effects.push(effect);
                                }
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
                if let Some(model) = self.state.model_selector_state.selected_model().cloned() {
                    let model_id = model.id.clone();
                    let agent_type = model.agent_type;
                    let display_name = model.display_name.clone();
                    let required_tool = Self::required_tool(agent_type);
                    if !self.tools().is_available(required_tool) {
                        self.show_missing_tool(
                            required_tool,
                            format!(
                                "{} is required to use this model.",
                                required_tool.display_name()
                            ),
                        );
                        return Ok(());
                    }
                    if self.state.model_picker_context
                        == ModelPickerContext::OnboardingDefaultSelection
                    {
                        if self.persist_default_model_selection(&model) {
                            self.state.model_selector_state.hide();
                            self.state.model_picker_context = ModelPickerContext::SessionSelection;
                            self.continue_new_project_flow();
                        }
                        return Ok(());
                    }

                    if self.state.model_picker_context
                        == ModelPickerContext::SettingsDefaultSelection
                    {
                        if self.persist_default_model_selection(&model) {
                            self.state.model_selector_state.hide();
                            self.state.model_picker_context = ModelPickerContext::SessionSelection;
                            self.reopen_settings_menu();
                        }
                        return Ok(());
                    }

                    if self.state.model_picker_context == ModelPickerContext::HandoffSelection {
                        self.state.model_selector_state.hide();
                        self.state.model_picker_context = ModelPickerContext::SessionSelection;
                        self.state.input_mode = InputMode::Normal;
                        match self.execute_handoff_session(agent_type, model_id.clone()) {
                            Ok(new_effects) => effects.extend(new_effects),
                            Err(err) => self.show_error("Handoff Failed", &err.to_string()),
                        }
                        return Ok(());
                    }

                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if Self::reject_cross_agent_switch(session, agent_type) {
                            return Ok(());
                        }
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
                self.state.model_picker_context = ModelPickerContext::SessionSelection;
                self.state.input_mode = InputMode::Normal;
            }
            InputMode::SelectingReasoning => {
                if let Some(option) = self.state.reasoning_selector_state.selected_option() {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if !App::reasoning_supported(session.agent_type) {
                            let display = MessageDisplay::Error {
                                content: "Reasoning effort is not supported for this agent."
                                    .to_string(),
                            };
                            session.chat_view.push(display.to_chat_message());
                            return Ok(());
                        }
                        if App::session_started(session) {
                            if session.agent_type == AgentType::Pi {
                                let Some(effort) = option.effort else {
                                    let display = MessageDisplay::Error {
                                        content: "Auto reasoning isn't available for active Pi sessions. Pick an explicit thinking level."
                                            .to_string(),
                                    };
                                    session.chat_view.push(display.to_chat_message());
                                    return Ok(());
                                };
                                let Some(input_tx) = session.agent_input_tx.clone() else {
                                    let display = MessageDisplay::Error {
                                        content: "Pi reasoning can't be changed because the live input channel is unavailable."
                                            .to_string(),
                                    };
                                    session.chat_view.push(display.to_chat_message());
                                    return Ok(());
                                };
                                tokio::spawn(async move {
                                    if let Err(err) = input_tx
                                        .send(AgentInput::PiSetThinkingLevel { level: effort })
                                        .await
                                    {
                                        tracing::warn!(
                                            "Failed to send Pi thinking level update: {}",
                                            err
                                        );
                                    }
                                });
                                session.set_reasoning_effort(Some(effort));
                            } else {
                                let display = MessageDisplay::Error {
                                    content: "Changing reasoning effort after a session has started is not supported. Start a new session/tab."
                                        .to_string(),
                                };
                                session.chat_view.push(display.to_chat_message());
                                return Ok(());
                            }
                        } else {
                            session.set_reasoning_effort(option.effort);
                        }
                        let msg = match option.effort {
                            Some(effort) => {
                                format!("Reasoning effort set to: {}", effort.display_name())
                            }
                            None => "Reasoning effort set to: Auto".to_string(),
                        };
                        let display = MessageDisplay::System { content: msg };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
                self.state.reasoning_selector_state.hide();
                self.state.input_mode = InputMode::Normal;
            }
            InputMode::SelectingTheme => {
                effects.extend(self.confirm_theme_picker()?);
            }
            InputMode::SelectingAgent => {
                let agent_type = self.state.agent_selector_state.selected_agent();
                self.state.agent_selector_state.hide();
                self.state.input_mode = InputMode::Normal;
                self.create_tab_with_agent(agent_type);
            }
            InputMode::SelectingProviders => {
                if !self.state.provider_selector_state.validate_non_empty() {
                    return Ok(());
                }
                let providers = self.state.provider_selector_state.selected_providers();
                if let Err(err) = crate::core::services::ConfigService::set_enabled_providers(
                    &mut self.core,
                    providers,
                ) {
                    self.state.provider_selector_state.dialog.validation_error =
                        Some(err.to_string());
                    return Ok(());
                }

                self.state.provider_selector_state.hide();
                self.state.set_timed_footer_message(
                    "Providers updated".to_string(),
                    std::time::Duration::from_secs(3),
                );

                if self.state.pending_new_project_target.is_some() {
                    self.continue_new_project_flow();
                } else if !self.return_to_settings_menu_if_needed() {
                    self.state.input_mode = InputMode::Normal;
                }
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
                    if let Some(dao) = self.app_state_dao() {
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
                    self.state.base_dir_dialog_state.hide();
                    match self.state.base_dir_dialog_context {
                        crate::ui::app_state::BaseDirDialogContext::ProjectDiscovery => {
                            let base_path = self.state.base_dir_dialog_state.expanded_path();
                            self.state.base_dir_dialog_context =
                                crate::ui::app_state::BaseDirDialogContext::ProjectDiscovery;
                            self.start_project_discovery(base_path);
                        }
                        crate::ui::app_state::BaseDirDialogContext::Settings => {
                            self.state.base_dir_dialog_context =
                                crate::ui::app_state::BaseDirDialogContext::ProjectDiscovery;
                            self.state.set_timed_footer_message(
                                "Projects directory updated".to_string(),
                                std::time::Duration::from_secs(3),
                            );
                            if !self.return_to_settings_menu_if_needed() {
                                self.state.input_mode = InputMode::Normal;
                            }
                        }
                    }
                }
            }
            InputMode::SettingsMenu => {
                self.open_selected_setting();
            }
            InputMode::WorkspaceDefaults => {
                if self
                    .state
                    .workspace_defaults_dialog_state
                    .activate_selected()
                {
                    let draft = self.state.workspace_defaults_dialog_state.draft;
                    if let Err(err) = crate::core::services::ConfigService::set_workspace_defaults(
                        &mut self.core,
                        draft.mode,
                        draft.archive_delete_branch,
                        draft.archive_remote_prompt,
                    ) {
                        self.show_error("Failed to Save", &err.to_string());
                        return Ok(());
                    }

                    self.state.workspace_defaults_dialog_state.hide();
                    self.state.set_timed_footer_message(
                        "Workspace defaults updated".to_string(),
                        std::time::Duration::from_secs(3),
                    );
                    if !self.return_to_settings_menu_if_needed() {
                        self.state.input_mode = InputMode::Normal;
                    }
                }
            }
            InputMode::Confirming => {
                if self.is_blocking_confirmation_loading_dialog() {
                    return Ok(());
                }
                if let Some(context) = self.state.confirmation_dialog_state.context.take() {
                    match context {
                        ConfirmationContext::SelectWorkspaceMode { repo_id } => {
                            let mode = if self.state.confirmation_dialog_state.is_confirm_selected()
                            {
                                crate::git::WorkspaceMode::Worktree
                            } else {
                                crate::git::WorkspaceMode::Checkout
                            };
                            match self.apply_repo_workspace_mode(repo_id, mode) {
                                Ok(()) => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.state.input_mode = InputMode::SidebarNavigation;
                                    if let Some(effect) = self.start_workspace_creation(repo_id) {
                                        effects.push(effect);
                                    }
                                }
                                Err(err) => {
                                    self.state.confirmation_dialog_state.hide();
                                    self.show_error("Unable to Set Workspace Mode", &err);
                                }
                            }
                            return Ok(());
                        }
                        ConfirmationContext::ArchiveWorkspaceRemoteDelete { workspace_id } => {
                            let delete_remote =
                                self.state.confirmation_dialog_state.is_confirm_selected();
                            effects
                                .push(self.execute_archive_workspace(workspace_id, delete_remote));
                            return Ok(());
                        }
                        ConfirmationContext::ArchiveWorkspace(id) => {
                            if self.state.confirmation_dialog_state.is_confirm_selected() {
                                effects.push(self.execute_archive_workspace_preflight(id));
                                return Ok(());
                            }
                        }
                        ConfirmationContext::ArchiveWorkspaceInProgress { .. } => {
                            return Ok(());
                        }
                        ConfirmationContext::ArchiveWorkspacePreflightInProgress { .. } => {
                            return Ok(());
                        }
                        ConfirmationContext::RemoveProject(id) => {
                            if self.state.confirmation_dialog_state.is_confirm_selected() {
                                effects.push(self.execute_remove_project(id));
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::SidebarNavigation;
                                return Ok(());
                            }
                        }
                        ConfirmationContext::RemoveProjectPreflightInProgress { .. } => {
                            return Ok(());
                        }
                        ConfirmationContext::CreatePullRequest {
                            tab_index,
                            working_dir,
                            preflight,
                        } => {
                            if self.state.confirmation_dialog_state.is_confirm_selected() {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.submit_pr_workflow(
                                    tab_index,
                                    working_dir,
                                    preflight,
                                )?);
                                return Ok(());
                            }
                        }
                        ConfirmationContext::OpenExistingPr { working_dir, .. } => {
                            if self.state.confirmation_dialog_state.is_confirm_selected() {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.push(Effect::OpenPrInBrowser { working_dir });
                                return Ok(());
                            }
                        }
                        ConfirmationContext::SteerFallback { message_id } => {
                            if self.state.confirmation_dialog_state.is_confirm_selected() {
                                self.state.confirmation_dialog_state.hide();
                                self.state.input_mode = InputMode::Normal;
                                effects.extend(self.confirm_steer_fallback(message_id)?);
                                return Ok(());
                            }
                        }
                        ConfirmationContext::ForkSession {
                            parent_workspace_id,
                            base_branch,
                        } => {
                            if self.state.confirmation_dialog_state.is_confirm_selected() {
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
                        ConfirmationContext::ForkSessionPreflightInProgress { .. } => {
                            return Ok(());
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
                            self.tools_mut().update_tool(tool, path.clone());
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
