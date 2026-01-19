use std::path::PathBuf;
use std::time::Duration;

use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::effect::Effect;
use crate::ui::events::InputMode;

impl App {
    /// Handle opening a file in a new tab
    pub(super) fn handle_open_file(&mut self, path: PathBuf, effects: &mut Vec<Effect>) {
        match self.state.tab_manager.open_file(path.clone()) {
            Ok(_) => {
                self.state.set_timed_footer_message(
                    format!("Opened: {}", path.display()),
                    Duration::from_secs(2),
                );
                self.state.input_mode = InputMode::Normal;
                self.state.sidebar_state.set_focused(false);
                effects.push(Effect::SaveSessionState);
            }
            Err(e) => {
                self.state
                    .error_dialog_state
                    .show("Failed to open file", format!("{}: {}", path.display(), e));
                self.state.input_mode = InputMode::ShowingError;
            }
        }
    }

    pub(super) fn handle_tab_action(&mut self, action: Action, effects: &mut Vec<Effect>) {
        match action {
            Action::CloseTab => {
                let active = self.state.tab_manager.active_index();
                self.stop_agent_for_tab(active);
                self.close_tab_at_index(active);
                if self.state.tab_manager.is_empty() {
                    self.state.stop_footer_spinner();
                    self.state.sidebar_state.visible = true;
                    self.state.input_mode = InputMode::SidebarNavigation;
                } else {
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
                effects.push(Effect::SaveSessionState);
            }
            Action::NextTab => {
                // Include sidebar in tab cycle when visible
                if self.state.input_mode == InputMode::SidebarNavigation {
                    // From sidebar, go to first tab
                    if !self.state.tab_manager.is_empty() {
                        self.state.tab_manager.switch_to(0);
                        self.state.sidebar_state.set_focused(false);
                        self.state.input_mode = InputMode::Normal;
                        self.sync_sidebar_to_active_tab();
                    }
                } else if self.state.sidebar_state.visible {
                    // Check if on last tab - if so, go to sidebar
                    let current = self.state.tab_manager.active_index();
                    let count = self.state.tab_manager.len();
                    if count > 0 && current == count - 1 {
                        // On last tab, go to sidebar
                        self.state.sidebar_state.set_focused(true);
                        self.state.input_mode = InputMode::SidebarNavigation;
                    } else {
                        self.state.tab_manager.next_tab();
                        self.sync_sidebar_to_active_tab();
                        self.sync_footer_spinner();
                    }
                } else {
                    self.state.tab_manager.next_tab();
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
            }
            Action::PrevTab => {
                // Include sidebar in tab cycle when visible
                if self.state.input_mode == InputMode::SidebarNavigation {
                    // From sidebar, go to last tab
                    let count = self.state.tab_manager.len();
                    if count > 0 {
                        self.state.tab_manager.switch_to(count - 1);
                        self.state.sidebar_state.set_focused(false);
                        self.state.input_mode = InputMode::Normal;
                        self.sync_sidebar_to_active_tab();
                        self.sync_footer_spinner();
                    }
                } else if self.state.sidebar_state.visible {
                    // Check if on first tab - if so, go to sidebar
                    let current = self.state.tab_manager.active_index();
                    if current == 0 {
                        // On first tab, go to sidebar
                        self.state.sidebar_state.set_focused(true);
                        self.state.input_mode = InputMode::SidebarNavigation;
                    } else {
                        self.state.tab_manager.prev_tab();
                        self.sync_sidebar_to_active_tab();
                        self.sync_footer_spinner();
                    }
                } else {
                    self.state.tab_manager.prev_tab();
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
            }
            Action::SwitchToTab(n) => {
                if n > 0 {
                    self.state.tab_manager.switch_to((n - 1) as usize);
                    self.sync_sidebar_to_active_tab();
                    self.sync_footer_spinner();
                }
            }
            _ => {}
        }
    }
}
