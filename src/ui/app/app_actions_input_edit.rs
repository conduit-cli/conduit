use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::events::InputMode;

impl App {
    pub(super) fn handle_input_edit_action(&mut self, action: Action) {
        match action {
            Action::InsertNewline => {
                // Don't insert newlines in help dialog, command mode, or sidebar navigation
                if self.state.input_mode != InputMode::ShowingHelp
                    && self.state.input_mode != InputMode::Command
                    && self.state.input_mode != InputMode::SidebarNavigation
                {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.input_box.insert_newline();
                    }
                }
            }
            Action::Backspace => match self.state.input_mode {
                InputMode::Command => {
                    if self.state.command_buffer.is_empty() {
                        // Exit command mode if buffer is empty
                        self.state.input_mode = InputMode::Normal;
                    } else {
                        self.state.command_buffer.pop();
                    }
                }
                InputMode::ShowingHelp => {
                    self.state.help_dialog_state.delete_char();
                }
                InputMode::ImportingSession => {
                    self.state.session_import_state.delete_char();
                }
                InputMode::PickingProject => {
                    self.state.project_picker_state.delete_char();
                }
                InputMode::CommandPalette => {
                    self.state.command_palette_state.delete_char();
                }
                InputMode::SettingBaseDir => {
                    self.state.base_dir_dialog_state.delete_char();
                }
                InputMode::MissingTool => {
                    self.state.missing_tool_dialog_state.backspace();
                }
                InputMode::SelectingTheme => {
                    self.state.theme_picker_state.backspace();
                }
                InputMode::SelectingModel => {
                    self.state.model_selector_state.delete_char();
                }
                _ => {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        session.input_box.backspace();
                    }
                }
            },
            Action::Delete => {
                if self.state.input_mode == InputMode::MissingTool {
                    self.state.missing_tool_dialog_state.delete();
                } else if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.delete();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.delete_forward();
                } else if self.state.input_mode == InputMode::SettingBaseDir {
                    self.state.base_dir_dialog_state.delete_forward();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete();
                }
            }
            Action::DeleteWordBack => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete_word_back();
                }
            }
            Action::DeleteWordForward => {
                // TODO: implement delete_word_forward in InputBox
            }
            Action::DeleteToStart => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete_to_start();
                }
            }
            Action::DeleteToEnd => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.delete_to_end();
                }
            }
            Action::MoveCursorLeft => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_left();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_left();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_left();
                }
            }
            Action::MoveCursorRight => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_right();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_right();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_right();
                }
            }
            Action::MoveCursorStart => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_to_start();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_start();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_start();
                }
            }
            Action::MoveCursorEnd => {
                if self.state.input_mode == InputMode::SelectingTheme {
                    self.state.theme_picker_state.move_to_end();
                } else if self.state.input_mode == InputMode::SelectingModel {
                    self.state.model_selector_state.move_cursor_end();
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_end();
                }
            }
            Action::MoveWordLeft => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_word_left();
                }
            }
            Action::MoveWordRight => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.move_word_right();
                }
            }
            Action::MoveCursorUp => {
                let mut dequeued = None;
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.move_up() && session.input_box.is_cursor_on_first_line() {
                        if session.input_box.is_empty() && !session.queued_messages.is_empty() {
                            dequeued = session.dequeue_last();
                        } else {
                            session.input_box.history_prev();
                        }
                    }
                }
                if let Some(message) = dequeued {
                    self.restore_queued_to_input(message);
                }
            }
            Action::MoveCursorDown => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if !session.input_box.move_down() && session.input_box.is_cursor_on_last_line()
                    {
                        session.input_box.history_next();
                    }
                }
            }
            Action::HistoryPrev => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.history_prev();
                }
            }
            Action::HistoryNext => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.input_box.history_next();
                }
            }
            _ => {}
        }
    }
}
