use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_dialog_action(&mut self, action: Action) {
        match action {
            Action::Cancel => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.hide(true); // Cancelled - restore original
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::AddingRepository => {
                    self.state.add_repo_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::SettingBaseDir => {
                    self.state.base_dir_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::Confirming => {
                    self.state.input_mode = self.dismiss_confirmation_dialog();
                }
                InputMode::ShowingError => {
                    self.state.error_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::MissingTool => {
                    self.state.missing_tool_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::Scrolling => {
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::Command => {
                    self.state.command_buffer.clear();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::ShowingHelp => {
                    self.state.help_dialog_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.hide();
                    self.state.input_mode = InputMode::Normal;
                }
                InputMode::QueueEditing => {
                    self.close_queue_editor();
                }
                _ => {}
            },
            Action::AddRepository => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    self.state.close_overlays();
                    self.state.add_repo_dialog_state.show();
                    self.state.input_mode = InputMode::AddingRepository;
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.hide();
                    self.state.close_overlays();
                    self.state.add_repo_dialog_state.show();
                    self.state.input_mode = InputMode::AddingRepository;
                }
                _ => {}
            },
            Action::OpenSettings => {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    self.state.close_overlays();
                    if let Some(dao) = &self.app_state_dao {
                        if let Ok(Some(current_dir)) = dao.get("projects_base_dir") {
                            self.state
                                .base_dir_dialog_state
                                .show_with_path(&current_dir);
                        } else {
                            self.state.base_dir_dialog_state.show();
                        }
                    } else {
                        self.state.base_dir_dialog_state.show();
                    }
                    self.state.input_mode = InputMode::SettingBaseDir;
                }
            }
            Action::ArchiveOrRemove => {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    if let Some(node) = self.state.sidebar_data.get_at(selected) {
                        use crate::ui::components::NodeType;
                        match node.node_type {
                            NodeType::Workspace => {
                                self.initiate_archive_workspace(node.id);
                            }
                            NodeType::Repository => {
                                self.initiate_remove_project(node.id);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
