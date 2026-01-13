//! Session import picker dialog component
//!
//! Allows users to import sessions from Claude Code, Codex CLI, and Gemini CLI.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Paragraph, Widget},
};

use super::{
    agent_claude, agent_codex, agent_gemini, bg_highlight, dialog_bg, ensure_contrast_bg,
    ensure_contrast_fg, render_minimal_scrollbar, selected_bg, text_muted, text_primary,
    DialogFrame, InstructionBar, ScrollbarMetrics, SearchableListState,
};
use crate::agent::AgentType;
use crate::session::ExternalSession;

// ============ Dialog Sizing Constants ============
/// Dialog width as percentage of screen (0-100)
const DIALOG_WIDTH_PERCENT: u16 = 70;
/// Dialog height as percentage of screen (0-100)
const DIALOG_HEIGHT_PERCENT: u16 = 70;
/// Minimum dialog width
const DIALOG_MIN_WIDTH: u16 = 60;
/// Maximum dialog width
const DIALOG_MAX_WIDTH: u16 = 100;
/// Minimum dialog height
const DIALOG_MIN_HEIGHT: u16 = 15;
/// Maximum dialog height
const DIALOG_MAX_HEIGHT: u16 = 40;

/// Filter for session agent type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentFilter {
    /// Show all sessions
    #[default]
    All,
    /// Show only Claude Code sessions
    Claude,
    /// Show only Codex CLI sessions
    Codex,
    /// Show only Gemini CLI sessions
    Gemini,
}

impl AgentFilter {
    /// Cycle to the next filter
    pub fn next(self) -> Self {
        match self {
            AgentFilter::All => AgentFilter::Claude,
            AgentFilter::Claude => AgentFilter::Codex,
            AgentFilter::Codex => AgentFilter::Gemini,
            AgentFilter::Gemini => AgentFilter::All,
        }
    }

    /// Get display label
    pub fn label(self) -> &'static str {
        match self {
            AgentFilter::All => "All",
            AgentFilter::Claude => "Claude",
            AgentFilter::Codex => "Codex",
            AgentFilter::Gemini => "Gemini",
        }
    }
}

/// Computed layout for the picker dialog
#[derive(Debug, Clone, Copy)]
struct PickerLayout {
    /// Area for the scrollable list
    list_area: Rect,
}

/// Calculate dialog layout for consistent positioning
fn calculate_picker_layout(area: Rect) -> Option<PickerLayout> {
    let dialog_width = (area.width * DIALOG_WIDTH_PERCENT / 100)
        .clamp(DIALOG_MIN_WIDTH, DIALOG_MAX_WIDTH)
        .min(area.width.saturating_sub(4));
    let dialog_height = (area.height * DIALOG_HEIGHT_PERCENT / 100)
        .clamp(DIALOG_MIN_HEIGHT, DIALOG_MAX_HEIGHT)
        .min(area.height.saturating_sub(2));

    let dialog_x = area.width.saturating_sub(dialog_width) / 2;
    let dialog_y = area.height.saturating_sub(dialog_height) / 2;

    // Inner area after dialog border (2 chars each side, 1 top/bottom)
    let inner_x = dialog_x + 2;
    let inner_y = dialog_y + 1;
    let inner_width = dialog_width.saturating_sub(4);

    // List starts after: tab bar (1) + search (1) + separator (1) = 3 rows
    // Bottom has: spacing (1) + instructions (1) + border (1) = 3 rows
    // Total vertical overhead from dialog_height: 3 top + 3 bottom + 1 top border = 7
    let list_height = dialog_height.saturating_sub(7);
    if list_height == 0 {
        return None;
    }

    Some(PickerLayout {
        list_area: Rect {
            x: inner_x,
            y: inner_y + 3,
            width: inner_width,
            height: list_height,
        },
    })
}

/// State for the session import picker dialog
#[derive(Debug, Clone)]
pub struct SessionImportPickerState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// All discovered sessions
    pub sessions: Vec<ExternalSession>,
    /// Agent type filter
    pub agent_filter: AgentFilter,
    /// Searchable list state
    pub list: SearchableListState,
    /// Whether currently loading sessions
    pub loading: bool,
    /// Error message if discovery failed
    pub error: Option<String>,
    /// Spinner frame for loading animation
    pub spinner_frame: usize,
}

impl Default for SessionImportPickerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionImportPickerState {
    pub fn new() -> Self {
        Self {
            visible: false,
            sessions: Vec::new(),
            agent_filter: AgentFilter::All,
            list: SearchableListState::new(10),
            loading: false,
            error: None,
            spinner_frame: 0,
        }
    }

    /// Advance the spinner animation
    pub fn tick(&mut self) {
        if self.loading {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
        }
    }

    /// Show the picker and discover sessions
    pub fn show(&mut self) {
        self.visible = true;
        self.list.reset();
        self.agent_filter = AgentFilter::All;
        self.error = None;
        self.loading = true;
    }

    /// Load discovered sessions (called when cached sessions are loaded)
    /// Note: Does NOT set loading=false since background refresh may continue
    pub fn load_sessions(&mut self, sessions: Vec<ExternalSession>) {
        self.sessions = sessions;
        self.filter();
    }

    /// Set error state
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Filter sessions based on search string and agent type
    pub fn filter(&mut self) {
        let query = self.list.search.value().to_lowercase();
        let filtered = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                // Agent filter
                match self.agent_filter {
                    AgentFilter::All => true,
                    AgentFilter::Claude => matches!(s.agent_type, AgentType::Claude),
                    AgentFilter::Codex => matches!(s.agent_type, AgentType::Codex),
                    AgentFilter::Gemini => matches!(s.agent_type, AgentType::Gemini),
                }
            })
            .filter(|(_, s)| {
                // Search filter
                if query.is_empty() {
                    true
                } else {
                    s.display.to_lowercase().contains(&query)
                        || s.project
                            .as_ref()
                            .map(|p| p.to_lowercase().contains(&query))
                            .unwrap_or(false)
                }
            })
            .map(|(i, _)| i)
            .collect();
        self.list.set_filtered(filtered);
    }

    /// Cycle agent filter
    pub fn cycle_filter(&mut self) {
        self.agent_filter = self.agent_filter.next();
        self.filter();
    }

    // Delegate search input methods
    pub fn insert_char(&mut self, c: char) {
        self.list.search.insert_char(c);
        self.filter();
    }

    pub fn delete_char(&mut self) {
        self.list.search.delete_char();
        self.filter();
    }

    pub fn delete_forward(&mut self) {
        self.list.search.delete_forward();
        self.filter();
    }

    pub fn move_cursor_left(&mut self) {
        self.list.search.move_left();
    }

    pub fn move_cursor_right(&mut self) {
        self.list.search.move_right();
    }

    pub fn move_cursor_start(&mut self) {
        self.list.search.move_start();
    }

    pub fn move_cursor_end(&mut self) {
        self.list.search.move_end();
    }

    pub fn clear_search(&mut self) {
        self.list.search.clear();
        self.filter();
    }

    /// Select previous item
    pub fn select_prev(&mut self) {
        self.list.select_prev();
    }

    /// Select next item
    pub fn select_next(&mut self) {
        self.list.select_next();
    }

    // ============ Incremental Update Methods ============

    /// Add or update a single session (for incremental discovery)
    pub fn upsert_session(&mut self, session: ExternalSession) {
        if let Some(pos) = self
            .sessions
            .iter()
            .position(|s| s.file_path == session.file_path)
        {
            self.sessions[pos] = session;
        } else {
            self.sessions.push(session);
        }
        // Re-sort by timestamp (most recent first)
        self.sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        // Reapply filters
        self.filter();
    }

    /// Remove a session by file path
    pub fn remove_session_by_path(&mut self, path: &std::path::Path) {
        self.sessions.retain(|s| s.file_path != path);
        self.filter();
    }

    /// Set loading state (called when discovery completes)
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    // ============ End Incremental Update Methods ============

    /// Page up
    pub fn page_up(&mut self) {
        self.list.page_up();
    }

    /// Page down
    pub fn page_down(&mut self) {
        self.list.page_down();
    }

    /// Select item at a given visual row (for mouse clicks)
    pub fn select_at_row(&mut self, row: usize) -> bool {
        self.list.select_at_row(row)
    }

    /// Get the currently selected session
    pub fn selected_session(&self) -> Option<&ExternalSession> {
        self.list
            .filtered
            .get(self.list.selected)
            .and_then(|&idx| self.sessions.get(idx))
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check if there are no sessions
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn scrollbar_metrics(&self, area: Rect) -> Option<ScrollbarMetrics> {
        if !self.visible {
            return None;
        }

        let layout = calculate_picker_layout(area)?;
        let list_area = layout.list_area;

        let total = self.list.filtered.len();
        let visible = list_area.height as usize;
        if total <= visible {
            return None;
        }

        Some(ScrollbarMetrics {
            area: Rect {
                x: list_area.x + list_area.width - 1,
                y: list_area.y,
                width: 1,
                height: list_area.height,
            },
            total,
            visible,
        })
    }
}

/// Session import picker dialog widget
pub struct SessionImportPicker;

impl SessionImportPicker {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &SessionImportPickerState) {
        if !state.visible {
            return;
        }

        // Calculate dialog dimensions based on screen size (percentage-based)
        let dialog_width =
            (area.width * DIALOG_WIDTH_PERCENT / 100).clamp(DIALOG_MIN_WIDTH, DIALOG_MAX_WIDTH);
        let dialog_height =
            (area.height * DIALOG_HEIGHT_PERCENT / 100).clamp(DIALOG_MIN_HEIGHT, DIALOG_MAX_HEIGHT);

        // Render dialog frame
        let frame = DialogFrame::new("Import Session", dialog_width, dialog_height);
        let inner = frame.render(area, buf);

        // Layout inside dialog
        let chunks = Layout::vertical([
            Constraint::Length(1), // Tab bar
            Constraint::Length(1), // Search label
            Constraint::Length(1), // Separator
            Constraint::Min(1),    // Session list
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        // Render tab bar
        self.render_tab_bar(chunks[0], buf, state);

        // Render search with placeholder
        let search_display = if state.list.search.is_empty() {
            "Search: (type to filter)".to_string()
        } else {
            format!("Search: {}", state.list.search.value())
        };
        let search_style = if state.list.search.is_empty() {
            Style::default().fg(text_muted())
        } else {
            Style::default().fg(text_primary())
        };
        let search_label = Paragraph::new(search_display).style(search_style);
        search_label.render(chunks[1], buf);

        // Render cursor in search field
        if !state.list.search.is_empty() || state.list.search.cursor > 0 {
            let cursor_x = chunks[1].x + 8 + state.list.search.cursor as u16;
            if cursor_x < chunks[1].x + chunks[1].width {
                buf[(cursor_x, chunks[1].y)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }

        // Render separator
        let separator = "─".repeat(inner.width as usize);
        let sep_paragraph = Paragraph::new(separator).style(Style::default().fg(Color::DarkGray));
        sep_paragraph.render(chunks[2], buf);

        // Render session list
        let list_area = chunks[3];
        if state.loading {
            const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = SPINNER_FRAMES[state.spinner_frame % SPINNER_FRAMES.len()];
            let loading = Paragraph::new(format!("{} Discovering sessions...", spinner))
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center);
            loading.render(list_area, buf);
        } else if let Some(ref error) = state.error {
            let error_msg = Paragraph::new(error.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center);
            error_msg.render(list_area, buf);
        } else if state.list.filtered.is_empty() {
            let empty_msg = if state.sessions.is_empty() {
                "No sessions found"
            } else {
                "No sessions match your filter"
            };
            let empty = Paragraph::new(empty_msg)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            empty.render(list_area, buf);
        } else {
            self.render_session_list(list_area, buf, state);
        }

        // Render instructions
        let instructions = InstructionBar::new(vec![
            ("↑↓", "Navigate"),
            ("Tab", "Filter"),
            ("Enter", "Import"),
            ("Esc", "Cancel"),
        ]);
        instructions.render(chunks[5], buf);
    }

    fn render_tab_bar(&self, area: Rect, buf: &mut Buffer, state: &SessionImportPickerState) {
        let mut x = area.x;
        let tab_selected_bg = ensure_contrast_bg(bg_highlight(), dialog_bg(), 2.0);

        for filter in [
            AgentFilter::All,
            AgentFilter::Claude,
            AgentFilter::Codex,
            AgentFilter::Gemini,
        ] {
            let is_selected = state.agent_filter == filter;
            let label = format!(" {} ", filter.label());
            let width = label.len() as u16;

            let base_fg = match filter {
                AgentFilter::All => text_primary(),
                AgentFilter::Claude => agent_claude(),
                AgentFilter::Codex => agent_codex(),
                AgentFilter::Gemini => agent_gemini(),
            };
            let style = if is_selected {
                let fg = ensure_contrast_fg(base_fg, tab_selected_bg, 4.5);
                Style::default().fg(fg).bg(tab_selected_bg)
            } else {
                Style::default().fg(text_muted())
            };

            if x + width <= area.x + area.width {
                let tab = Paragraph::new(label).style(style);
                tab.render(
                    Rect {
                        x,
                        y: area.y,
                        width,
                        height: 1,
                    },
                    buf,
                );
                x += width + 1; // Gap between tabs
            }
        }

        // Show count
        let count = format!("({}/{})", state.list.filtered.len(), state.sessions.len());
        let count_len = count.len() as u16;
        let count_style = Style::default().fg(text_muted());
        let count_x = area.x + area.width - count_len;
        if count_x > x {
            let count_para = Paragraph::new(count).style(count_style);
            count_para.render(
                Rect {
                    x: count_x,
                    y: area.y,
                    width: count_len,
                    height: 1,
                },
                buf,
            );
        }
    }

    fn render_session_list(&self, area: Rect, buf: &mut Buffer, state: &SessionImportPickerState) {
        let visible_count = area.height as usize;

        let row_selected_bg = ensure_contrast_bg(selected_bg(), dialog_bg(), 3.0);
        let row_selected_fg = ensure_contrast_fg(text_primary(), row_selected_bg, 4.5);

        for (i, &session_idx) in state
            .list
            .filtered
            .iter()
            .skip(state.list.scroll_offset)
            .take(visible_count)
            .enumerate()
        {
            let session = &state.sessions[session_idx];
            let is_selected = state.list.scroll_offset + i == state.list.selected;
            let y = area.y + i as u16;

            if y >= area.y + area.height {
                break;
            }

            // Row 1: Display text (with selection indicator)
            let prefix = if is_selected { "> " } else { "  " };
            let agent_icon = match session.agent_type {
                AgentType::Claude => "C",
                AgentType::Codex => "X",
                AgentType::Gemini => "G",
            };
            let agent_color = match session.agent_type {
                AgentType::Claude => agent_claude(),
                AgentType::Codex => agent_codex(),
                AgentType::Gemini => agent_gemini(),
            };

            // Calculate widths
            let available = area.width as usize;
            let display_max = available.saturating_sub(5); // prefix + icon + space
            let display = session.truncated_display(display_max);

            // Background for selected row
            let bg_style = if is_selected {
                Style::default().bg(row_selected_bg)
            } else {
                Style::default()
            };

            // Clear the line first
            for j in 0..area.width as usize {
                buf[(area.x + j as u16, y)]
                    .set_char(' ')
                    .set_style(bg_style);
            }

            // Render prefix
            let mut x = area.x;
            for c in prefix.chars() {
                if x < area.x + area.width {
                    let fg = if is_selected {
                        row_selected_fg
                    } else {
                        text_primary()
                    };
                    buf[(x, y)].set_char(c).set_style(bg_style.fg(fg));
                    x += 1;
                }
            }

            // Render agent icon
            if x < area.x + area.width {
                let fg = if is_selected {
                    row_selected_fg
                } else {
                    text_muted()
                };
                buf[(x, y)].set_char('[').set_style(bg_style.fg(fg));
                x += 1;
            }
            if x < area.x + area.width {
                let fg = if is_selected {
                    row_selected_fg
                } else {
                    agent_color
                };
                buf[(x, y)]
                    .set_char(agent_icon.chars().next().unwrap_or(' '))
                    .set_style(bg_style.fg(fg).add_modifier(Modifier::BOLD));
                x += 1;
            }
            if x < area.x + area.width {
                let fg = if is_selected {
                    row_selected_fg
                } else {
                    text_muted()
                };
                buf[(x, y)].set_char(']').set_style(bg_style.fg(fg));
                x += 1;
            }
            if x < area.x + area.width {
                buf[(x, y)].set_char(' ').set_style(bg_style);
                x += 1;
            }

            // Render display text
            for c in display.chars() {
                if x < area.x + area.width {
                    let fg = if is_selected {
                        row_selected_fg
                    } else {
                        text_primary()
                    };
                    buf[(x, y)].set_char(c).set_style(bg_style.fg(fg));
                    x += 1;
                }
            }

            // Render metadata on the right side
            let time_str = session.relative_time();
            let msg_str = format!("{} msgs", session.message_count);
            let project_name = session.project_name().unwrap_or_default();

            // Format: "project • time • msgs"
            let mut meta_parts = Vec::new();
            if !project_name.is_empty() {
                meta_parts.push(project_name);
            }
            meta_parts.push(time_str);
            meta_parts.push(msg_str);
            let meta = meta_parts.join(" • ");

            let meta_x = (area.x + area.width).saturating_sub(meta.len() as u16 + 1);
            if meta_x > x + 2 {
                for (j, c) in meta.chars().enumerate() {
                    let mx = meta_x + j as u16;
                    if mx < area.x + area.width {
                        let fg = if is_selected {
                            row_selected_fg
                        } else {
                            text_muted()
                        };
                        buf[(mx, y)].set_char(c).set_style(bg_style.fg(fg));
                    }
                }
            }
        }

        // Render scrollbar
        let total_filtered = state.list.filtered.len();
        render_minimal_scrollbar(
            Rect {
                x: area.x + area.width - 1,
                y: area.y,
                width: 1,
                height: area.height,
            },
            buf,
            total_filtered,
            visible_count,
            state.list.scroll_offset,
        );
    }
}

impl Default for SessionImportPicker {
    fn default() -> Self {
        Self::new()
    }
}
