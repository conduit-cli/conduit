use crate::ui::app::App;
use crate::ui::effect::Effect;
use crate::ui::events::InputMode;

impl App {
    /// Handle Ctrl+P: Open existing PR or create new one
    pub(super) fn handle_pr_action(&mut self) -> Option<Effect> {
        let tab_index = self.state.tab_manager.active_index();
        let session = self.state.tab_manager.active_session()?;

        let working_dir = session.working_dir.as_ref()?.clone();

        // Show loading dialog immediately
        self.state.close_overlays();
        self.state
            .confirmation_dialog_state
            .show_loading("Create Pull Request", "Checking repository status...");
        self.state.input_mode = InputMode::Confirming;

        Some(Effect::PrPreflight {
            tab_index,
            working_dir,
        })
    }
}
