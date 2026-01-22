use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_sidebar_action(
        &mut self,
        action: Action,
        effects: &mut Vec<crate::ui::effect::Effect>,
    ) {
        match action {
            Action::ToggleSidebar => {
                self.state.sidebar_state.toggle();
                if self.state.sidebar_state.visible {
                    self.state.sidebar_state.set_focused(true);
                    self.state.input_mode = InputMode::SidebarNavigation;
                    // Focus on the current tab's workspace if it has one
                    if let Some(session) = self.state.tab_manager.active_session() {
                        if let Some(workspace_id) = session.workspace_id {
                            if let Some(index) =
                                self.state.sidebar_data.focus_workspace(workspace_id)
                            {
                                self.state.sidebar_state.tree_state.selected = index;
                            }
                        }
                    }
                } else {
                    self.state.sidebar_state.set_focused(false);
                    self.state.input_mode = InputMode::Normal;
                }
            }
            Action::EnterSidebarMode => {
                self.state.sidebar_state.show();
                self.state.sidebar_state.set_focused(true);
                self.state.input_mode = InputMode::SidebarNavigation;
            }
            Action::ExitSidebarMode => {
                self.state.sidebar_state.set_focused(false);
                self.state.input_mode = InputMode::Normal;
            }
            Action::ExpandOrSelect => {
                // Same as Confirm for sidebar
                if self.state.input_mode == InputMode::SidebarNavigation {
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
            }
            Action::Collapse => {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    let selected = self.state.sidebar_state.tree_state.selected;
                    if let Some(node) = self.state.sidebar_data.get_at(selected) {
                        if !node.is_leaf() && node.expanded {
                            self.state.sidebar_data.toggle_at(selected);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
