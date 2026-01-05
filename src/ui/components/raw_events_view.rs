use std::collections::HashSet;
use std::time::Instant;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use super::raw_events_types::{
    EventDetailState, EventDirection, RawEventEntry, DETAIL_PANEL_BREAKPOINT,
};
use super::{render_vertical_scrollbar, ScrollbarMetrics, ScrollbarSymbols};

pub enum RawEventsClick {
    SessionId,
    Event(usize),
}
/// View for displaying raw agent events with interactive selection
pub struct RawEventsView {
    /// All recorded events
    events: Vec<RawEventEntry>,
    /// Currently selected event index
    selected_index: usize,
    /// Set of expanded event indices (allows multiple items to be expanded)
    expanded_indices: HashSet<usize>,
    /// Scroll offset for viewport (in lines)
    scroll_offset: usize,
    /// Session start time
    session_start: Instant,
    /// Event detail panel state
    pub event_detail: EventDetailState,
    /// Agent session ID (for display/copy)
    session_id: Option<String>,
}

pub struct RawEventsScrollbarMetrics {
    pub list: Option<ScrollbarMetrics>,
    pub detail: Option<ScrollbarMetrics>,
}

impl RawEventsView {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            selected_index: 0,
            expanded_indices: HashSet::new(),
            scroll_offset: 0,
            session_start: Instant::now(),
            event_detail: EventDetailState::new(),
            session_id: None,
        }
    }

    pub fn set_session_id(&mut self, session_id: Option<String>) {
        self.session_id = session_id;
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Add a new event and select it
    pub fn push_event(
        &mut self,
        direction: EventDirection,
        event_type: impl Into<String>,
        raw_json: Value,
    ) {
        self.events.push(RawEventEntry::new(
            direction,
            event_type,
            raw_json,
            self.session_start,
        ));
        // Auto-select new event (keep existing expansions)
        self.selected_index = self.events.len().saturating_sub(1);
    }

    /// Move selection to previous event
    pub fn select_prev(&mut self) {
        if self.events.is_empty() {
            return;
        }
        self.selected_index = self.selected_index.saturating_sub(1);
        // Sync detail panel to follow selection
        self.event_detail.sync_to_event(self.selected_index);
    }

    /// Move selection to next event
    pub fn select_next(&mut self) {
        if self.events.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1).min(self.events.len().saturating_sub(1));
        // Sync detail panel to follow selection
        self.event_detail.sync_to_event(self.selected_index);
    }

    /// Toggle expand/collapse of selected event
    pub fn toggle_expand(&mut self) {
        if !self.events.is_empty() {
            if self.expanded_indices.contains(&self.selected_index) {
                self.expanded_indices.remove(&self.selected_index);
            } else {
                self.expanded_indices.insert(self.selected_index);
            }
        }
    }

    /// Collapse all expanded events
    pub fn collapse(&mut self) {
        self.expanded_indices.clear();
    }

    /// Check if the selected event is expanded
    pub fn is_expanded(&self) -> bool {
        self.expanded_indices.contains(&self.selected_index)
    }

    /// Check if a specific event is expanded
    fn is_index_expanded(&self, index: usize) -> bool {
        self.expanded_indices.contains(&index)
    }

    /// Get event count
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get all events (for debug dump)
    pub fn events(&self) -> &[RawEventEntry] {
        &self.events
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.events.clear();
        self.selected_index = 0;
        self.expanded_indices.clear();
        self.scroll_offset = 0;
        self.session_start = Instant::now();
        self.event_detail = EventDetailState::new();
    }

    /// Toggle detail panel visibility
    pub fn toggle_detail(&mut self) {
        if !self.events.is_empty() {
            // Sync to current selection before toggling
            self.event_detail.sync_to_event(self.selected_index);
            self.event_detail.toggle();
        }
    }

    /// Check if detail panel is visible
    pub fn is_detail_visible(&self) -> bool {
        self.event_detail.visible
    }

    /// Get the selected event's JSON as pretty-printed string (for copy action)
    pub fn get_selected_json(&self) -> Option<String> {
        self.events
            .get(self.selected_index)
            .and_then(|event| serde_json::to_string_pretty(&event.raw_json).ok())
    }

    /// Get the selected event index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Build detail content lines for the current event
    fn build_detail_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let Some(event) = self.events.get(self.event_detail.event_index) else {
            lines.push(Line::from(Span::styled(
                "No event selected",
                Style::default().fg(Color::DarkGray),
            )));
            return lines;
        };

        // Header with direction and timestamp
        let direction_symbol = event.direction.symbol();
        let direction_label = match event.direction {
            EventDirection::Sent => "Outgoing",
            EventDirection::Received => "Incoming",
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} {} ", direction_symbol, direction_label),
                Style::default().fg(event.direction.color()),
            ),
            Span::styled(
                event.format_timestamp(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        // Event type
        lines.push(Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                event.event_type.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Separator
        lines.push(Line::from(""));

        // Full JSON with syntax highlighting (no line limit)
        if let Ok(pretty) = serde_json::to_string_pretty(&event.raw_json) {
            for json_line in pretty.lines() {
                let mut highlighted = Vec::new();
                highlighted.extend(RawEventEntry::highlight_json_line(json_line));
                lines.push(Line::from(highlighted));
            }
        }

        lines
    }

    /// Get total content height for detail panel
    pub fn detail_content_height(&self) -> usize {
        self.build_detail_lines().len()
    }

    /// Scroll up by n lines
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines
    pub fn scroll_down(&mut self, n: usize, visible_height: usize) {
        let (lines, _, _) = self.build_lines();
        let max_scroll = lines.len().saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
    }

    /// Handle a click at the given position within the view area
    /// Returns Some(event_index) if a click was handled, None otherwise
    pub fn handle_click(&mut self, x: u16, y: u16, area: Rect) -> Option<RawEventsClick> {
        // Check title click (top border)
        if y == area.y {
            if let Some((_, start, width)) =
                self.title_right_span(area.width.saturating_sub(2) as usize)
            {
                let title_x = area.x.saturating_add(1 + start);
                if x >= title_x && x < title_x.saturating_add(width) {
                    return Some(RawEventsClick::SessionId);
                }
            }
        }

        // Check if click is within the inner area (accounting for border)
        let inner_x = area.x + 1;
        let inner_y = area.y + 1;
        let inner_height = area.height.saturating_sub(2);

        if x < inner_x || y < inner_y || y >= inner_y + inner_height {
            return None;
        }

        // Calculate which line was clicked
        let clicked_line = (y - inner_y) as usize + self.scroll_offset;

        // Map the clicked line to an event index
        let (lines, _, _) = self.build_lines();
        if clicked_line >= lines.len() {
            return None;
        }

        // Walk through events to find which one corresponds to the clicked line
        let mut current_line: usize = 0;
        for (i, _event) in self.events.iter().enumerate() {
            let is_expanded = self.is_index_expanded(i);
            let event_start = current_line;

            // Calculate how many lines this event takes
            let event_lines = if is_expanded {
                // Header + JSON lines (max 20) + possible truncation line
                1 + self.events[i].render_json_lines(20).len()
            } else {
                1
            };

            let event_end = current_line + event_lines;

            if clicked_line >= event_start && clicked_line < event_end {
                // Clicked on this event - select it
                self.selected_index = i;
                // Sync detail panel to follow selection
                self.event_detail.sync_to_event(self.selected_index);
                return Some(RawEventsClick::Event(i));
            }

            current_line = event_end;
        }

        None
    }

    /// Build all lines for rendering, tracking which line range corresponds to selected item
    fn build_lines(&self) -> (Vec<Line<'static>>, usize, usize) {
        let mut lines = Vec::new();
        let mut selected_start = 0;
        let mut selected_end = 0;

        if self.events.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No events recorded yet",
                Style::default().fg(Color::DarkGray),
            )));
            return (lines, 0, 0);
        }

        for (i, event) in self.events.iter().enumerate() {
            let is_selected = i == self.selected_index;
            let is_expanded = self.is_index_expanded(i);

            if is_selected {
                selected_start = lines.len();
            }

            if is_expanded {
                // Expanded view: header with ▼ prefix, then JSON
                let bg_style = if is_selected {
                    Style::default().bg(Color::Rgb(40, 40, 50))
                } else {
                    Style::default()
                };
                let header = event.render_compact("▼", bg_style);
                lines.push(header);

                // Add JSON lines (limit to 20 lines)
                let json_lines = event.render_json_lines(20);
                lines.extend(json_lines);
            } else if is_selected {
                // Selected but collapsed: ▶ prefix with highlight
                let line = event.render_compact("▶", Style::default().bg(Color::Rgb(40, 40, 50)));
                lines.push(line);
            } else {
                // Normal: space prefix
                let line = event.render_compact(" ", Style::default());
                lines.push(line);
            }

            if is_selected {
                selected_end = lines.len();
            }
        }

        (lines, selected_start, selected_end)
    }

    fn session_id_label(&self, max_width: usize) -> Option<String> {
        let session_id = self.session_id.as_ref()?;
        if max_width == 0 {
            return None;
        }

        let prefix = "Session ID: ";
        let mut label = format!(" {prefix}{session_id} ");
        if UnicodeWidthStr::width(label.as_str()) > max_width {
            let available = max_width.saturating_sub(UnicodeWidthStr::width(prefix) + 3);
            if available == 0 {
                return None;
            }
            let shortened: String = session_id.chars().take(available).collect();
            label = format!("{prefix}{shortened}...");
        }

        Some(label)
    }

    fn title_right_span(&self, max_width: usize) -> Option<(String, u16, u16)> {
        let left = format!(" Raw Events ({}) ", self.events.len());
        let left_width = UnicodeWidthStr::width(left.as_str());
        let available = max_width.saturating_sub(left_width);
        let right = self.session_id_label(available)?;
        let right_width = UnicodeWidthStr::width(right.as_str());
        if left_width + right_width > max_width {
            return None;
        }
        let spacer = max_width.saturating_sub(left_width + right_width);
        Some((right, (left_width + spacer) as u16, right_width as u16))
    }

    fn build_title_line(&self, max_width: usize) -> Line<'static> {
        let left = format!(" Raw Events ({}) ", self.events.len());
        let left_width = UnicodeWidthStr::width(left.as_str());

        if let Some(right) = self.session_id_label(max_width.saturating_sub(left_width)) {
            let right_width = UnicodeWidthStr::width(right.as_str());
            if left_width + right_width <= max_width {
                let spacer = max_width.saturating_sub(left_width + right_width);
                return Line::from(vec![
                    Span::styled(left, Style::default().fg(Color::DarkGray)),
                    Span::styled("─".repeat(spacer), Style::default().fg(Color::DarkGray)),
                    Span::styled(right, Style::default().fg(Color::DarkGray)),
                ]);
            }
        }

        Line::from(Span::styled(left, Style::default().fg(Color::DarkGray)))
    }

    /// Render the raw events view with optional detail panel
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let use_split = self.event_detail.visible && area.width >= DETAIL_PANEL_BREAKPOINT;
        let use_overlay = self.event_detail.visible && area.width < DETAIL_PANEL_BREAKPOINT;

        if use_split {
            // Split layout: event list on left, detail panel on right
            let chunks =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area);

            self.render_event_list(chunks[0], buf);
            self.render_detail_panel(chunks[1], buf);
        } else {
            // Full width event list
            self.render_event_list(area, buf);

            // Overlay detail panel if visible
            if use_overlay {
                self.render_detail_overlay(area, buf);
            }
        }
    }

    pub fn scrollbar_metrics(&mut self, area: Rect) -> RawEventsScrollbarMetrics {
        let use_split = self.event_detail.visible && area.width >= DETAIL_PANEL_BREAKPOINT;
        let use_overlay = self.event_detail.visible && area.width < DETAIL_PANEL_BREAKPOINT;

        if use_split {
            let chunks =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area);
            RawEventsScrollbarMetrics {
                list: self.event_list_scrollbar_metrics(chunks[0]),
                detail: self.detail_panel_scrollbar_metrics(chunks[1]),
            }
        } else {
            RawEventsScrollbarMetrics {
                list: self.event_list_scrollbar_metrics(area),
                detail: if use_overlay {
                    self.detail_overlay_scrollbar_metrics(area)
                } else {
                    None
                },
            }
        }
    }

    pub fn set_list_scroll_offset(&mut self, offset: usize, total: usize, visible: usize) {
        let max_scroll = total.saturating_sub(visible);
        self.scroll_offset = offset.min(max_scroll);
    }

    pub fn set_detail_scroll_offset(&mut self, offset: usize, total: usize, visible: usize) {
        let max_scroll = total.saturating_sub(visible);
        self.event_detail.scroll_offset = offset.min(max_scroll);
    }

    /// Render the event list
    fn render_event_list(&mut self, area: Rect, buf: &mut Buffer) {
        let title = self.build_title_line(area.width.saturating_sub(2) as usize);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(title);

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width < 3 || inner.height < 1 {
            return;
        }

        let (lines, selected_start, selected_end) = self.build_lines();
        let total_lines = lines.len();
        let visible_height = inner.height as usize;

        // Ensure selected item is visible
        self.ensure_selection_visible(selected_start, selected_end, visible_height, total_lines);

        // Calculate which lines to show
        let start_line = self.scroll_offset;
        let end_line = (self.scroll_offset + visible_height).min(total_lines);

        let visible_lines: Vec<Line<'static>> = lines[start_line..end_line].to_vec();

        let paragraph = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);

        render_vertical_scrollbar(
            Rect {
                x: inner.x + inner.width,
                y: inner.y,
                width: 1,
                height: inner.height,
            },
            buf,
            total_lines,
            visible_height,
            self.scroll_offset,
            ScrollbarSymbols::arrows(),
        );
    }

    fn event_list_scrollbar_metrics(&mut self, area: Rect) -> Option<ScrollbarMetrics> {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        if inner.width < 3 || inner.height < 1 {
            return None;
        }

        let (lines, _, _) = self.build_lines();
        let total_lines = lines.len();
        let visible_height = inner.height as usize;
        if total_lines <= visible_height {
            return None;
        }

        Some(ScrollbarMetrics {
            area: Rect {
                x: inner.x + inner.width,
                y: inner.y,
                width: 1,
                height: inner.height,
            },
            total: total_lines,
            visible: visible_height,
        })
    }

    /// Render the detail panel (split mode - right side)
    fn render_detail_panel(&mut self, area: Rect, buf: &mut Buffer) {
        let event_type = self
            .events
            .get(self.event_detail.event_index)
            .map(|e| e.event_type.as_str())
            .unwrap_or("Event");
        let title = format!(" {} ", event_type);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title);

        let inner = block.inner(area);
        block.render(area, buf);

        self.render_detail_content(inner, buf);
    }

    fn detail_panel_scrollbar_metrics(&mut self, area: Rect) -> Option<ScrollbarMetrics> {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);
        self.detail_content_scrollbar_metrics(inner)
    }

    /// Render the detail overlay (centered floating dialog)
    fn render_detail_overlay(&mut self, area: Rect, buf: &mut Buffer) {
        // Calculate overlay size (90% width, 80% height)
        let overlay_width = (area.width as f32 * 0.9) as u16;
        let overlay_height = (area.height as f32 * 0.8) as u16;

        let overlay_x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
        let overlay_y = area.y + (area.height.saturating_sub(overlay_height)) / 2;

        let overlay_area = Rect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_width,
            height: overlay_height,
        };

        // Clear the overlay area
        Clear.render(overlay_area, buf);

        let event_type = self
            .events
            .get(self.event_detail.event_index)
            .map(|e| e.event_type.as_str())
            .unwrap_or("Event");
        let title = format!(" {} ", event_type);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title);

        let inner = block.inner(overlay_area);
        block.render(overlay_area, buf);

        self.render_detail_content(inner, buf);
    }

    fn detail_overlay_scrollbar_metrics(&mut self, area: Rect) -> Option<ScrollbarMetrics> {
        let overlay_width = (area.width as f32 * 0.9) as u16;
        let overlay_height = (area.height as f32 * 0.8) as u16;

        let overlay_x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
        let overlay_y = area.y + (area.height.saturating_sub(overlay_height)) / 2;

        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(overlay_area);
        self.detail_content_scrollbar_metrics(inner)
    }

    /// Render the detail content (shared by panel and overlay)
    fn render_detail_content(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 1 {
            return;
        }

        let lines = self.build_detail_lines();
        let total_lines = lines.len();
        let visible_height = area.height as usize;

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(visible_height);
        if self.event_detail.scroll_offset > max_scroll {
            self.event_detail.scroll_offset = max_scroll;
        }

        // Calculate which lines to show
        let start_line = self.event_detail.scroll_offset;
        let end_line = (self.event_detail.scroll_offset + visible_height).min(total_lines);

        let visible_lines: Vec<Line<'static>> = if start_line < end_line && end_line <= lines.len()
        {
            lines[start_line..end_line].to_vec()
        } else {
            vec![]
        };

        let paragraph = Paragraph::new(visible_lines);
        paragraph.render(area, buf);

        render_vertical_scrollbar(
            Rect {
                x: area.x + area.width,
                y: area.y,
                width: 1,
                height: area.height,
            },
            buf,
            total_lines,
            visible_height,
            self.event_detail.scroll_offset,
            ScrollbarSymbols::arrows(),
        );
    }

    fn detail_content_scrollbar_metrics(&mut self, area: Rect) -> Option<ScrollbarMetrics> {
        if area.width < 3 || area.height < 1 {
            return None;
        }

        let lines = self.build_detail_lines();
        let total_lines = lines.len();
        let visible_height = area.height as usize;
        if total_lines <= visible_height {
            return None;
        }

        Some(ScrollbarMetrics {
            area: Rect {
                x: area.x + area.width,
                y: area.y,
                width: 1,
                height: area.height,
            },
            total: total_lines,
            visible: visible_height,
        })
    }

    /// Ensure the selected item is visible in the viewport
    fn ensure_selection_visible(
        &mut self,
        selected_start: usize,
        selected_end: usize,
        visible_height: usize,
        total_lines: usize,
    ) {
        // If selected item starts before viewport, scroll up
        if selected_start < self.scroll_offset {
            self.scroll_offset = selected_start;
        }
        // If selected item ends after viewport, scroll down
        else if selected_end > self.scroll_offset + visible_height {
            self.scroll_offset = selected_end.saturating_sub(visible_height);
        }

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(visible_height);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
    }
}

impl Default for RawEventsView {
    fn default() -> Self {
        Self::new()
    }
}
