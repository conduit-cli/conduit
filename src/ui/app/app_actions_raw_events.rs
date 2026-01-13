use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::effect::Effect;

impl App {
    pub(super) fn handle_raw_events_action(&mut self, action: Action, effects: &mut Vec<Effect>) {
        match action {
            Action::RawEventsSelectNext => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.select_next();
                }
            }
            Action::RawEventsSelectPrev => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.select_prev();
                }
            }
            Action::RawEventsToggleExpand => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.toggle_expand();
                }
            }
            Action::RawEventsCollapse => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.collapse();
                }
            }
            Action::EventDetailToggle => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.toggle_detail();
                }
            }
            Action::EventDetailScrollUp => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.event_detail.scroll_up(1);
                }
            }
            Action::EventDetailScrollDown => {
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let content_height = session.raw_events_view.detail_content_height();
                    session.raw_events_view.event_detail.scroll_down(
                        1,
                        content_height,
                        detail_height,
                    );
                }
            }
            Action::EventDetailPageUp => {
                let list_height = self.raw_events_list_visible_height();
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    if session.raw_events_view.is_detail_visible() {
                        session.raw_events_view.event_detail.page_up(detail_height);
                    } else {
                        let amount = list_height.saturating_sub(2).max(1);
                        session.raw_events_view.scroll_up(amount);
                    }
                }
            }
            Action::EventDetailPageDown => {
                let list_height = self.raw_events_list_visible_height();
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let content_height = session.raw_events_view.detail_content_height();
                    if session.raw_events_view.is_detail_visible() {
                        session
                            .raw_events_view
                            .event_detail
                            .page_down(detail_height, content_height);
                    } else {
                        let amount = list_height.saturating_sub(2).max(1);
                        session.raw_events_view.scroll_down(amount, list_height);
                    }
                }
            }
            Action::EventDetailScrollToTop => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.event_detail.scroll_to_top();
                }
            }
            Action::EventDetailScrollToBottom => {
                let detail_height = self.raw_events_detail_visible_height();
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let content_height = session.raw_events_view.detail_content_height();
                    session
                        .raw_events_view
                        .event_detail
                        .scroll_to_bottom(content_height, detail_height);
                }
            }
            Action::EventDetailCopy => {
                if let Some(session) = self.state.tab_manager.active_session() {
                    if let Some(json) = session.raw_events_view.get_selected_json() {
                        effects.push(Effect::CopyToClipboard(json));
                    }
                }
            }
            _ => {}
        }
    }
}
