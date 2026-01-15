use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::app_state::SelectionDragTarget;
use crate::ui::effect::Effect;
use crate::ui::events::{InputMode, ViewMode};
use std::time::Duration;

impl App {
    pub(super) fn handle_global_action(&mut self, action: Action, effects: &mut Vec<Effect>) {
        match action {
            Action::Quit => {
                self.state.should_quit = true;
                effects.push(Effect::SaveSessionState);
            }
            Action::NewProject => {
                self.open_project_picker_or_base_dir();
            }
            Action::NewWorkspaceUnderCursor => {
                use crate::ui::components::{ActionType, NodeType};

                let sidebar_focused = self.state.sidebar_state.focused;
                let repo_id_from_sidebar = if sidebar_focused {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    self.state
                        .sidebar_data
                        .get_at(selected)
                        .and_then(|node| match node.node_type {
                            NodeType::Repository => Some(node.id),
                            NodeType::Workspace => node.parent_id,
                            NodeType::Action(ActionType::NewWorkspace) => node.parent_id,
                        })
                } else {
                    None
                };

                let repo_id_from_tab = if sidebar_focused {
                    None
                } else {
                    let session = self.state.tab_manager.active_session();
                    let workspace_id = session.and_then(|s| s.workspace_id);
                    match (workspace_id, self.workspace_dao().as_ref()) {
                        (Some(workspace_id), Some(workspace_dao)) => {
                            match workspace_dao.get_by_id(workspace_id) {
                                Ok(Some(workspace)) => Some(workspace.repository_id),
                                Ok(None) => {
                                    tracing::error!(
                                        workspace_id = %workspace_id,
                                        "Workspace not found for active tab"
                                    );
                                    None
                                }
                                Err(err) => {
                                    tracing::error!(
                                        workspace_id = %workspace_id,
                                        error = %err,
                                        "Failed to load workspace for active tab"
                                    );
                                    None
                                }
                            }
                        }
                        _ => None,
                    }
                };

                let repo_id = if sidebar_focused {
                    repo_id_from_sidebar
                } else {
                    repo_id_from_tab
                };

                if let Some(repo_id) = repo_id {
                    effects.push(self.start_workspace_creation(repo_id));
                } else {
                    self.state.set_timed_footer_message(
                        "No project selected to create a workspace".to_string(),
                        Duration::from_secs(5),
                    );
                }
            }
            Action::ForkSession => {
                self.initiate_fork_session();
            }
            Action::InterruptAgent => {
                self.interrupt_agent();
            }
            Action::ToggleViewMode => {
                self.state.view_mode = match self.state.view_mode {
                    ViewMode::Chat => ViewMode::RawEvents,
                    ViewMode::RawEvents => ViewMode::Chat,
                };
            }
            Action::ShowModelSelector => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    let model = session.model.clone();
                    self.state.close_overlays();
                    let defaults = self.model_selector_defaults();
                    self.state.model_selector_state.show(model, defaults);
                    self.state.input_mode = InputMode::SelectingModel;
                }
            }
            Action::ShowThemePicker => {
                self.state.close_overlays();
                let theme_path = self.config().theme_path.clone();
                self.state.theme_picker_state.show(theme_path.as_deref());
                self.state.input_mode = InputMode::SelectingTheme;
            }
            Action::OpenSessionImport => {
                self.state.close_overlays();
                self.state.session_import_state.show();
                self.state.input_mode = InputMode::ImportingSession;
                // Trigger session discovery
                effects.push(Effect::DiscoverSessions);
            }
            Action::ImportSession => {
                if self.state.input_mode == InputMode::ImportingSession {
                    if let Some(session) =
                        self.state.session_import_state.selected_session().cloned()
                    {
                        self.state.session_import_state.hide();
                        self.state.input_mode = InputMode::Normal;
                        effects.push(Effect::ImportSession(session));
                    }
                }
            }
            Action::CycleImportFilter => {
                if self.state.input_mode == InputMode::ImportingSession {
                    self.state.session_import_state.cycle_filter();
                }
            }
            Action::ToggleMetrics => {
                self.state.show_metrics = !self.state.show_metrics;
                // Uncomment to test spinner animation smoothness with Alt+P:
                // if self.state.show_metrics {
                //     self.state
                //         .start_footer_spinner(Some("Testing spinner...".to_string()));
                // } else {
                //     self.state.stop_footer_spinner();
                // }
            }
            Action::ToggleAgentMode => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    // Only toggle when agent supports plan mode
                    if session.capabilities.supports_plan_mode {
                        session.agent_mode = session.agent_mode.toggle();
                        session.update_status();
                    }
                }
            }
            Action::DumpDebugState => {
                effects.push(Effect::DumpDebugState);
            }
            Action::CopyWorkspacePath => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    if let Some(working_dir) = &session.working_dir {
                        let path_str = working_dir.display().to_string();
                        effects.push(Effect::CopyToClipboard(path_str.clone()));
                        self.state.set_timed_footer_message(
                            format!("Copied: {}", path_str),
                            Duration::from_secs(10),
                        );
                    }
                }
            }
            Action::CopySelection => {
                let mut copied = false;
                let clear_after_copy = self.config().selection.clear_selection_after_copy;
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.input_box.has_selection() {
                        if let Some(text) = session.input_box.selected_text() {
                            copied = true;
                            effects.push(Effect::CopyToClipboard(text));
                            if clear_after_copy {
                                Self::clear_selection_for_target(
                                    session,
                                    SelectionDragTarget::Input,
                                );
                            }
                        }
                    } else if session.chat_view.has_selection() {
                        if let Some(text) = session.chat_view.copy_selection() {
                            copied = true;
                            effects.push(Effect::CopyToClipboard(text));
                            if clear_after_copy {
                                Self::clear_selection_for_target(
                                    session,
                                    SelectionDragTarget::Chat,
                                );
                            }
                        }
                    }
                }

                if copied {
                    self.state.set_timed_footer_message(
                        "Copied selection".to_string(),
                        Duration::from_secs(5),
                    );
                } else {
                    self.state.set_timed_footer_message(
                        "No selection to copy".to_string(),
                        Duration::from_secs(3),
                    );
                }
            }
            _ => {}
        }
    }
}
