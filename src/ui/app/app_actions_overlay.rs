use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_overlay_action(&mut self, action: Action) {
        match action {
            Action::ToggleDetails => {
                if self.state.input_mode == InputMode::ShowingError {
                    self.state.error_dialog_state.toggle_details();
                }
            }
            Action::SelectAgent => {
                if self.state.input_mode == InputMode::SelectingAgent {
                    let agent_type = self.state.agent_selector_state.selected_agent();
                    self.state.agent_selector_state.hide();
                    self.create_tab_with_agent(agent_type);
                }
            }
            Action::ShowHelp => {
                self.state.close_overlays();
                let keybindings = self.config().keybindings.clone();
                self.state.help_dialog_state.show(&keybindings);
                self.state.input_mode = InputMode::ShowingHelp;
            }
            Action::OpenCommandPalette => {
                self.state.close_overlays();
                let keybindings = self.config().keybindings.clone();
                let supports_plan_mode = self
                    .state
                    .tab_manager
                    .active_session()
                    .is_some_and(|s| s.capabilities.supports_plan_mode);
                self.state
                    .command_palette_state
                    .show(&keybindings, supports_plan_mode);
                self.state.input_mode = InputMode::CommandPalette;
            }
            _ => {}
        }
    }
}
