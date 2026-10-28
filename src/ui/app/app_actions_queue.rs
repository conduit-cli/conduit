use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_queue_action(&mut self, action: Action) {
        match action {
            Action::OpenQueueEditor => {
                self.open_queue_editor();
            }
            Action::CloseQueueEditor => {
                self.close_queue_editor();
            }
            Action::QueueMoveUp => {
                if self.state.input_mode == InputMode::QueueEditing {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.move_queue_up();
                        crate::ui::app_queue::clamp_queue_selection(session);
                    }
                }
            }
            Action::QueueMoveDown => {
                if self.state.input_mode == InputMode::QueueEditing {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.move_queue_down();
                        crate::ui::app_queue::clamp_queue_selection(session);
                    }
                }
            }
            Action::QueueEdit => {
                if self.state.input_mode == InputMode::QueueEditing {
                    let mut message = None;
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        message = session.dequeue_selected();
                        crate::ui::app_queue::clamp_queue_selection(session);
                    }
                    if let Some(msg) = message {
                        self.close_queue_editor();
                        self.restore_queued_to_input(msg);
                    } else {
                        self.close_queue_editor();
                    }
                }
            }
            Action::QueueDelete if self.state.input_mode == InputMode::QueueEditing => {
                let mut should_close = false;
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if let Some(idx) = session.queue_selection {
                        session.remove_queue_at(idx);
                    }
                    crate::ui::app_queue::clamp_queue_selection(session);
                    if session.queued_messages.is_empty() {
                        should_close = true;
                    }
                }
                if should_close {
                    self.close_queue_editor();
                }
            }
            _ => {}
        }
    }
}
