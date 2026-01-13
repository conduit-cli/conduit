use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_list_action(&mut self, action: Action) {
        match action {
            Action::SelectNext => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    let visible_count = self.state.sidebar_data.visible_nodes().len();
                    self.state
                        .sidebar_state
                        .tree_state
                        .select_next(visible_count);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.select_next();
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.select_next();
                }
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.select_next();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.select_next();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.select_next();
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.select_next();
                }
                InputMode::QueueEditing => {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.select_queue_next();
                    }
                }
                _ => {}
            },
            Action::SelectPrev => match self.state.input_mode {
                InputMode::SidebarNavigation => {
                    let visible_count = self.state.sidebar_data.visible_nodes().len();
                    self.state
                        .sidebar_state
                        .tree_state
                        .select_previous(visible_count);
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.select_previous();
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.select_prev();
                }
                InputMode::SelectingAgent => {
                    self.state.agent_selector_state.select_previous();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.select_prev();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.select_prev();
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.select_prev();
                }
                InputMode::QueueEditing => {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.select_queue_prev();
                    }
                }
                _ => {}
            },
            Action::SelectPageDown => {
                if self.state.input_mode == InputMode::PickingProject {
                    self.state.project_picker_state.page_down();
                } else if self.state.input_mode == InputMode::ImportingSession {
                    self.state.session_import_state.page_down();
                }
            }
            Action::SelectPageUp => {
                if self.state.input_mode == InputMode::PickingProject {
                    self.state.project_picker_state.page_up();
                } else if self.state.input_mode == InputMode::ImportingSession {
                    self.state.session_import_state.page_up();
                }
            }
            _ => {}
        }
    }
}
