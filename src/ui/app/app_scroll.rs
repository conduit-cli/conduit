use crossterm::terminal;
use ratatui::layout::Rect;

use crate::ui::app::App;
use crate::ui::app_state::ScrollDragTarget;
use crate::ui::components::{
    scrollbar_offset_from_point, RawEventsScrollbarMetrics, ScrollbarMetrics,
};
use crate::ui::events::{InputMode, ViewMode};

impl App {
    pub(super) fn record_scroll(&mut self, lines: usize) {
        if lines > 0 {
            self.state.metrics.record_scroll_event(lines);
        }
    }

    pub(super) fn should_route_scroll_to_chat(&self) -> bool {
        self.state.input_mode != InputMode::ShowingHelp
            && !(self.state.input_mode == InputMode::PickingProject
                && self.state.project_picker_state.is_visible())
            && !(self.state.input_mode == InputMode::ImportingSession
                && self.state.session_import_state.is_visible())
            && !(self.state.input_mode == InputMode::CommandPalette
                && self.state.command_palette_state.is_visible())
            && !(self.state.input_mode == InputMode::SlashMenu
                && self.state.slash_menu_state.is_visible())
            && !(self.state.input_mode == InputMode::SelectingTheme
                && self.state.theme_picker_state.is_visible())
            && !(self.state.input_mode == InputMode::SelectingModel
                && self.state.model_selector_state.is_visible())
    }

    pub(super) fn raw_events_list_visible_height(&self) -> usize {
        self.state
            .raw_events_area
            .map(|r| r.height.saturating_sub(2) as usize)
            .unwrap_or(20)
    }

    pub(super) fn raw_events_detail_visible_height(&self) -> usize {
        let Some(area) = self.state.raw_events_area else {
            return 20;
        };
        if area.width < crate::ui::components::DETAIL_PANEL_BREAKPOINT {
            let overlay_height = (area.height as f32 * 0.8) as u16;
            overlay_height.saturating_sub(2) as usize
        } else {
            area.height.saturating_sub(2) as usize
        }
    }

    pub(super) fn flush_scroll_deltas(&mut self, pending_up: &mut usize, pending_down: &mut usize) {
        if *pending_up == 0 && *pending_down == 0 {
            return;
        }

        if self.state.input_mode == InputMode::ShowingHelp {
            // Route scroll to help dialog
            if *pending_up > 0 {
                self.state.help_dialog_state.scroll_up(*pending_up);
            }
            if *pending_down > 0 {
                self.state.help_dialog_state.scroll_down(*pending_down);
            }
        } else if self.state.input_mode == InputMode::PickingProject
            && self.state.project_picker_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.project_picker_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.project_picker_state.select_next();
            }
        } else if self.state.input_mode == InputMode::ImportingSession
            && self.state.session_import_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.session_import_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.session_import_state.select_next();
            }
        } else if self.state.input_mode == InputMode::CommandPalette
            && self.state.command_palette_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.command_palette_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.command_palette_state.select_next();
            }
        } else if self.state.input_mode == InputMode::SlashMenu
            && self.state.slash_menu_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.slash_menu_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.slash_menu_state.select_next();
            }
        } else if self.state.input_mode == InputMode::SelectingTheme
            && self.state.theme_picker_state.is_visible()
        {
            for _ in 0..*pending_up {
                self.state.theme_picker_state.select_prev();
            }
            for _ in 0..*pending_down {
                self.state.theme_picker_state.select_next();
            }
        } else if self.state.view_mode == ViewMode::RawEvents {
            let list_height = self.raw_events_list_visible_height();
            let detail_height = self.raw_events_detail_visible_height();
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                if session.raw_events_view.is_detail_visible() {
                    let content_height = session.raw_events_view.detail_content_height();
                    let visible_height = detail_height;
                    if *pending_up > 0 {
                        session.raw_events_view.event_detail.scroll_up(*pending_up);
                    }
                    if *pending_down > 0 {
                        session.raw_events_view.event_detail.scroll_down(
                            *pending_down,
                            content_height,
                            visible_height,
                        );
                    }
                } else {
                    if *pending_up > 0 {
                        session.raw_events_view.scroll_up(*pending_up);
                    }
                    if *pending_down > 0 {
                        session
                            .raw_events_view
                            .scroll_down(*pending_down, list_height);
                    }
                }
            }
        } else if let Some(session) = self.state.tab_manager.active_session_mut() {
            if *pending_up > 0 {
                session.chat_view.scroll_up(*pending_up);
            }
            if *pending_down > 0 {
                session.chat_view.scroll_down(*pending_down);
            }
        }

        *pending_up = 0;
        *pending_down = 0;
    }

    pub(super) fn handle_scrollbar_press(&mut self, x: u16, y: u16) -> bool {
        if let Some(target) = self.scrollbar_target_at(x, y) {
            self.state.scroll_drag = Some(target);
            return self.apply_scrollbar_drag(target, y);
        }
        false
    }

    pub(super) fn handle_scrollbar_drag(&mut self, y: u16) -> bool {
        if let Some(target) = self.state.scroll_drag {
            return self.apply_scrollbar_drag(target, y);
        }
        false
    }

    pub(super) fn scrollbar_target_at(&mut self, x: u16, y: u16) -> Option<ScrollDragTarget> {
        let mut targets = Vec::new();

        if self.state.input_mode == InputMode::ShowingHelp {
            targets.push(ScrollDragTarget::HelpDialog);
        } else if self.state.input_mode == InputMode::PickingProject
            && self.state.project_picker_state.is_visible()
        {
            targets.push(ScrollDragTarget::ProjectPicker);
        } else if self.state.input_mode == InputMode::ImportingSession
            && self.state.session_import_state.is_visible()
        {
            targets.push(ScrollDragTarget::SessionImport);
        } else if self.state.view_mode == ViewMode::RawEvents {
            targets.push(ScrollDragTarget::RawEventsDetail);
            targets.push(ScrollDragTarget::RawEventsList);
        } else {
            if self.state.input_mode != InputMode::Command {
                targets.push(ScrollDragTarget::Input);
            }
            targets.push(ScrollDragTarget::Chat);
        }

        for target in targets {
            if let Some(metrics) = self.scrollbar_metrics_for_target(target) {
                if Self::point_in_rect(x, y, metrics.area) {
                    return Some(target);
                }
            }
        }

        None
    }

    pub(super) fn apply_scrollbar_drag(&mut self, target: ScrollDragTarget, y: u16) -> bool {
        let Some(metrics) = self.scrollbar_metrics_for_target(target) else {
            return false;
        };

        let new_offset =
            scrollbar_offset_from_point(y, metrics.area, metrics.total, metrics.visible);

        match target {
            ScrollDragTarget::Chat => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.set_scroll_from_top(
                        new_offset,
                        metrics.total,
                        metrics.visible,
                    );
                }
            }
            ScrollDragTarget::Input => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session
                        .input_box
                        .set_scroll_offset(new_offset, metrics.total, metrics.visible);
                }
            }
            ScrollDragTarget::HelpDialog => {
                let max_scroll = metrics.total.saturating_sub(metrics.visible);
                self.state.help_dialog_state.scroll_offset = new_offset.min(max_scroll);
            }
            ScrollDragTarget::ProjectPicker => {
                let max_scroll = metrics.total.saturating_sub(metrics.visible);
                self.state.project_picker_state.list.scroll_offset = new_offset.min(max_scroll);
            }
            ScrollDragTarget::SessionImport => {
                let max_scroll = metrics.total.saturating_sub(metrics.visible);
                self.state.session_import_state.list.scroll_offset = new_offset.min(max_scroll);
            }
            ScrollDragTarget::RawEventsList => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.set_list_scroll_offset(
                        new_offset,
                        metrics.total,
                        metrics.visible,
                    );
                }
            }
            ScrollDragTarget::RawEventsDetail => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.raw_events_view.set_detail_scroll_offset(
                        new_offset,
                        metrics.total,
                        metrics.visible,
                    );
                }
            }
        }

        true
    }

    pub(super) fn scrollbar_metrics_for_target(
        &mut self,
        target: ScrollDragTarget,
    ) -> Option<ScrollbarMetrics> {
        let (width, height) = terminal::size().unwrap_or((0, 0));
        let screen = Rect::new(0, 0, width, height);

        match target {
            ScrollDragTarget::HelpDialog => self.state.help_dialog_state.scrollbar_metrics(screen),
            ScrollDragTarget::ProjectPicker => {
                self.state.project_picker_state.scrollbar_metrics(screen)
            }
            ScrollDragTarget::SessionImport => {
                self.state.session_import_state.scrollbar_metrics(screen)
            }
            ScrollDragTarget::Chat => {
                let area = self.state.chat_area?;
                let show_chat_scrollbar = self.config().ui.show_chat_scrollbar;
                let session = self.state.tab_manager.active_session_mut()?;
                session.chat_view.scrollbar_metrics(
                    area,
                    session.is_processing,
                    session.queued_messages.len(),
                    show_chat_scrollbar,
                )
            }
            ScrollDragTarget::Input => {
                let area = self.state.input_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                session.input_box.scrollbar_metrics(area)
            }
            ScrollDragTarget::RawEventsList => {
                let area = self.state.raw_events_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                let RawEventsScrollbarMetrics { list, .. } =
                    session.raw_events_view.scrollbar_metrics(area);
                list
            }
            ScrollDragTarget::RawEventsDetail => {
                let area = self.state.raw_events_area?;
                let session = self.state.tab_manager.active_session_mut()?;
                let RawEventsScrollbarMetrics { detail, .. } =
                    session.raw_events_view.scrollbar_metrics(area);
                detail
            }
        }
    }
}
