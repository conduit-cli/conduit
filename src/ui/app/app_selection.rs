use crate::ui::app::App;
use crate::ui::app_state::SelectionDragTarget;
use crate::ui::events::InputMode;
use crate::ui::session::AgentSession;

impl App {
    pub(super) fn handle_selection_start(&mut self, x: u16, y: u16) -> bool {
        if self.has_active_dialog() {
            return false;
        }
        if self.state.view_mode != crate::ui::events::ViewMode::Chat {
            return false;
        }

        let show_chat_scrollbar = self.config().ui.show_chat_scrollbar;
        let Some(session) = self.state.tab_manager.active_session_mut() else {
            return false;
        };

        if let Some(input_area) = self.state.input_area {
            if self.state.input_mode != InputMode::Command && Self::point_in_rect(x, y, input_area)
            {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                session.chat_view.clear_selection();
                if session.input_box.begin_selection(x, y, input_area) {
                    self.state.selection_drag = Some(SelectionDragTarget::Input);
                    return true;
                }
            }
        }

        if let Some(chat_area) = self.state.chat_area {
            if Self::point_in_rect(x, y, chat_area) {
                if self.state.input_mode == InputMode::SidebarNavigation {
                    self.state.input_mode = InputMode::Normal;
                    self.state.sidebar_state.set_focused(false);
                }
                session.input_box.clear_selection();
                if session
                    .chat_view
                    .begin_selection(x, y, chat_area, show_chat_scrollbar)
                {
                    self.state.selection_drag = Some(SelectionDragTarget::Chat);
                    return true;
                }
            }
        }

        false
    }

    pub(super) fn handle_selection_drag(&mut self, x: u16, y: u16) -> bool {
        let Some(target) = self.state.selection_drag else {
            return false;
        };

        let mut scrolled_lines = 0usize;
        let mut handled = false;
        let show_chat_scrollbar = self.config().ui.show_chat_scrollbar;

        {
            let Some(session) = self.state.tab_manager.active_session_mut() else {
                return false;
            };

            match target {
                SelectionDragTarget::Input => {
                    if let Some(input_area) = self.state.input_area {
                        session.input_box.update_selection(x, y, input_area);
                        handled = true;
                    }
                }
                SelectionDragTarget::Chat => {
                    if let Some(chat_area) = self.state.chat_area {
                        let top_edge = chat_area.y;
                        let bottom_edge_inclusive = chat_area
                            .y
                            .saturating_add(chat_area.height.saturating_sub(1));
                        let bottom_edge_exclusive = chat_area.y.saturating_add(chat_area.height);
                        let should_scroll_up = if Self::AUTO_SCROLL_ON_EDGE_INCLUSIVE {
                            y <= top_edge
                        } else {
                            y < top_edge
                        };
                        let should_scroll_down = if Self::AUTO_SCROLL_ON_EDGE_INCLUSIVE {
                            y >= bottom_edge_inclusive
                        } else {
                            y >= bottom_edge_exclusive
                        };

                        if should_scroll_up {
                            session.chat_view.scroll_up(1);
                            scrolled_lines = scrolled_lines.saturating_add(1);
                        } else if should_scroll_down {
                            session.chat_view.scroll_down(1);
                            scrolled_lines = scrolled_lines.saturating_add(1);
                        }
                        session.chat_view.update_selection(
                            x,
                            y,
                            chat_area,
                            session.is_processing,
                            show_chat_scrollbar,
                        );
                        handled = true;
                    }
                }
            }
        }

        if scrolled_lines > 0 {
            self.record_scroll(scrolled_lines);
        }

        handled
    }

    pub(super) fn handle_selection_end(&mut self) -> Option<Vec<crate::ui::effect::Effect>> {
        let target = self.state.selection_drag.take()?;
        let mut copied_text = None;
        let mut should_clear_selection = false;
        let auto_copy = self.config().selection.auto_copy_selection;
        let clear_after_copy = self.config().selection.clear_selection_after_copy;
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            let has_selection = match target {
                SelectionDragTarget::Input => session.input_box.finalize_selection(),
                SelectionDragTarget::Chat => session.chat_view.finalize_selection(),
            };

            if has_selection && auto_copy {
                copied_text = Self::selection_text_for_target(session, target);
                should_clear_selection = copied_text.is_some() && clear_after_copy;
            }
            if should_clear_selection {
                Self::clear_selection_for_target(session, target);
            }
        }

        let mut effects = Vec::new();
        if let Some(text) = copied_text {
            effects.push(crate::ui::effect::Effect::CopyToClipboard(text));
            self.state.set_timed_footer_message(
                "Copied selection".to_string(),
                std::time::Duration::from_secs(5),
            );
        }

        Some(effects)
    }

    pub(super) fn selection_text_for_target(
        session: &mut AgentSession,
        target: SelectionDragTarget,
    ) -> Option<String> {
        match target {
            SelectionDragTarget::Input => session.input_box.selected_text(),
            SelectionDragTarget::Chat => session.chat_view.copy_selection(),
        }
    }

    pub(super) fn clear_selection_for_target(
        session: &mut AgentSession,
        target: SelectionDragTarget,
    ) {
        match target {
            SelectionDragTarget::Input => session.input_box.clear_selection(),
            SelectionDragTarget::Chat => session.chat_view.clear_selection(),
        }
    }
}
