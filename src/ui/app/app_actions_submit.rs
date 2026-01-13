use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::effect::Effect;

impl App {
    pub(super) fn handle_submit_related_action(
        &mut self,
        action: Action,
        effects: &mut Vec<Effect>,
    ) -> anyhow::Result<()> {
        match action {
            Action::Submit => {
                effects
                    .extend(self.handle_submit_action(crate::data::QueuedMessageMode::FollowUp)?);
            }
            Action::SubmitSteer => {
                effects.extend(self.handle_submit_action(crate::data::QueuedMessageMode::Steer)?);
            }
            _ => {}
        }

        Ok(())
    }
}
