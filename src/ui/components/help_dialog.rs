//! Help dialog showing all keybindings with search
//!
//! A scrollable modal dialog that displays keybindings organized by category
//! with real-time search filtering.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::config::{KeybindingConfig, KeyContext};
use crate::ui::action::Action;

use super::{
    render_vertical_scrollbar, DialogFrame, InstructionBar, ScrollbarMetrics, ScrollbarSymbols,
    TextInputState,
};

/// A keybinding entry for display
#[derive(Debug, Clone)]
pub struct KeybindingEntry {
    pub action_description: String,
    pub key_display: String,
}

/// Category for grouping keybindings in the help dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpCategory {
    Global,
    Readline,
    Chat,
    Scrolling,
    Sidebar,
    Dialog,
}

impl HelpCategory {
    pub fn title(&self) -> &'static str {
        match self {
            HelpCategory::Global => "GLOBAL",
            HelpCategory::Readline => "READLINE",
            HelpCategory::Chat => "CHAT",
            HelpCategory::Scrolling => "SCROLLING",
            HelpCategory::Sidebar => "SIDEBAR",
            HelpCategory::Dialog => "DIALOG",
        }
    }

    /// Order for display
    fn order(&self) -> usize {
        match self {
            HelpCategory::Global => 0,
            HelpCategory::Readline => 1,
            HelpCategory::Chat => 2,
            HelpCategory::Scrolling => 3,
            HelpCategory::Sidebar => 4,
            HelpCategory::Dialog => 5,
        }
    }
}

/// State for the help dialog
#[derive(Debug, Clone)]
pub struct HelpDialogState {
    /// Whether visible
    pub visible: bool,
    /// Search filter
    pub search: TextInputState,
    /// All keybindings organized by category
    entries: Vec<(HelpCategory, Vec<KeybindingEntry>)>,
    /// Scroll offset
    pub scroll_offset: usize,
    /// Total lines (for scrolling calculation)
    total_lines: usize,
    /// Visible height (set during render)
    visible_height: usize,
}

impl Default for HelpDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpDialogState {
    pub fn new() -> Self {
        Self {
            visible: false,
            search: TextInputState::new(),
            entries: Vec::new(),
            scroll_offset: 0,
            total_lines: 0,
            visible_height: 20,
        }
    }

    /// Show the dialog and populate with keybindings
    pub fn show(&mut self, config: &KeybindingConfig) {
        self.visible = true;
        self.search.clear();
        self.scroll_offset = 0;
        self.populate_entries(config);
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn scrollbar_metrics(&self, area: Rect) -> Option<ScrollbarMetrics> {
        if !self.visible {
            return None;
        }

        let dialog_width = (area.width * 70 / 100).min(80).max(50);
        let dialog_height = (area.height * 80 / 100).min(35).max(15);

        let dialog_width = dialog_width.min(area.width.saturating_sub(4));
        let dialog_height = dialog_height.min(area.height.saturating_sub(2));

        let x = (area.width.saturating_sub(dialog_width)) / 2;
        let y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width,
            height: dialog_height,
        };

        let block = Block::default()
            .title(" Help - Keybindings ")
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Rgb(130, 170, 255)));
        let inner = block.inner(dialog_area);
        let inner = Rect {
            x: inner.x.saturating_add(1),
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height,
        };

        if inner.height < 5 {
            return None;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(inner);

        let content_area = Rect {
            x: chunks[1].x,
            y: chunks[1].y,
            width: chunks[1].width.saturating_sub(2),
            height: chunks[1].height,
        };
        let visible_height = content_area.height as usize;
        let total_lines = self.total_lines;

        if total_lines <= visible_height {
            return None;
        }

        Some(ScrollbarMetrics {
            area: Rect {
                x: chunks[1].x + chunks[1].width.saturating_sub(1),
                y: chunks[1].y,
                width: 1,
                height: chunks[1].height,
            },
            total: total_lines,
            visible: visible_height,
        })
    }

    /// Populate entries from keybinding config
    fn populate_entries(&mut self, config: &KeybindingConfig) {
        use std::collections::HashMap;

        self.entries.clear();

        let mut categories: HashMap<HelpCategory, Vec<KeybindingEntry>> = HashMap::new();

        // Categorize global bindings
        for (combo, action) in &config.global {
            let category = if Self::is_readline_action(action) {
                HelpCategory::Readline
            } else {
                HelpCategory::Global
            };

            let entry = KeybindingEntry {
                action_description: action.description().to_string(),
                key_display: combo.to_string(),
            };

            categories.entry(category).or_default().push(entry);
        }

        // Add context-specific bindings
        for (ctx, bindings) in &config.context {
            let category = match ctx {
                KeyContext::Chat => Some(HelpCategory::Chat),
                KeyContext::Scrolling => Some(HelpCategory::Scrolling),
                KeyContext::Sidebar => Some(HelpCategory::Sidebar),
                KeyContext::Dialog | KeyContext::ProjectPicker | KeyContext::ModelSelector => {
                    Some(HelpCategory::Dialog)
                }
                _ => None,
            };

            if let Some(cat) = category {
                for (combo, action) in bindings {
                    let entry = KeybindingEntry {
                        action_description: action.description().to_string(),
                        key_display: combo.to_string(),
                    };
                    categories.entry(cat).or_default().push(entry);
                }
            }
        }

        // Sort entries within each category and convert to vec
        let mut entries: Vec<_> = categories.into_iter().collect();

        // Sort categories by order
        entries.sort_by_key(|(cat, _)| cat.order());

        // Sort entries within each category by action description
        for (_, items) in &mut entries {
            items.sort_by(|a, b| a.action_description.cmp(&b.action_description));
            // Deduplicate entries with same action description
            items.dedup_by(|a, b| a.action_description == b.action_description);
        }

        self.entries = entries;
        self.calculate_total_lines();
    }

    fn is_readline_action(action: &Action) -> bool {
        matches!(
            action,
            Action::MoveCursorLeft
                | Action::MoveCursorRight
                | Action::MoveCursorStart
                | Action::MoveCursorEnd
                | Action::MoveWordLeft
                | Action::MoveWordRight
                | Action::Backspace
                | Action::Delete
                | Action::DeleteWordBack
                | Action::DeleteWordForward
                | Action::DeleteToStart
                | Action::DeleteToEnd
                | Action::InsertNewline
        )
    }

    fn calculate_total_lines(&mut self) {
        self.total_lines = 0;
        let query = self.search.value().to_lowercase();

        for (_, entries) in &self.entries {
            let matching: Vec<_> = entries
                .iter()
                .filter(|e| {
                    query.is_empty()
                        || e.action_description.to_lowercase().contains(&query)
                        || e.key_display.to_lowercase().contains(&query)
                })
                .collect();

            if !matching.is_empty() {
                self.total_lines += 2 + matching.len(); // header + blank line after + entries
            }
        }
    }

    /// Get filtered entries
    fn filtered_entries(&self) -> Vec<(HelpCategory, Vec<&KeybindingEntry>)> {
        let query = self.search.value().to_lowercase();
        let mut result = Vec::new();

        for (category, entries) in &self.entries {
            let matching: Vec<_> = entries
                .iter()
                .filter(|e| {
                    query.is_empty()
                        || e.action_description.to_lowercase().contains(&query)
                        || e.key_display.to_lowercase().contains(&query)
                })
                .collect();

            if !matching.is_empty() {
                result.push((*category, matching));
            }
        }

        result
    }

    /// Handle character input for search
    pub fn insert_char(&mut self, c: char) {
        // Only accept printable characters for search (no control chars/newlines)
        if c.is_control() {
            return;
        }
        self.search.insert_char(c);
        self.calculate_total_lines();
        self.scroll_offset = 0;
    }

    pub fn delete_char(&mut self) {
        self.search.delete_char();
        self.calculate_total_lines();
        self.scroll_offset = 0;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.total_lines.saturating_sub(self.visible_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.visible_height.saturating_sub(2));
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.visible_height.saturating_sub(2));
    }
}

/// Help dialog widget
pub struct HelpDialog;

impl HelpDialog {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &mut HelpDialogState) {
        if !state.visible {
            return;
        }

        // Centered dialog (70% width, 80% height, max 80x35)
        let dialog_width = (area.width * 70 / 100).min(80).max(50);
        let dialog_height = (area.height * 80 / 100).min(35).max(15);

        let frame = DialogFrame::new("Help - Keybindings", dialog_width, dialog_height)
            .border_color(Color::Cyan);
        let inner = frame.render(area, buf);

        if inner.height < 5 {
            return;
        }

        // Layout: search bar at top, content below, instructions at bottom
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Search bar with border
                Constraint::Min(3),    // Content
                Constraint::Length(1), // Instructions
            ])
            .split(inner);

        // Search bar with border
        self.render_search_bar(chunks[0], buf, state);

        // Content area - reserve space for scrollbar
        let content_area = Rect {
            x: chunks[1].x,
            y: chunks[1].y,
            width: chunks[1].width.saturating_sub(2), // Leave room for scrollbar
            height: chunks[1].height,
        };
        state.visible_height = content_area.height as usize;
        self.render_content(content_area, buf, state);

        // Scrollbar
        let scrollbar_area = Rect {
            x: chunks[1].x + chunks[1].width.saturating_sub(1),
            y: chunks[1].y,
            width: 1,
            height: chunks[1].height,
        };
        self.render_scrollbar(scrollbar_area, buf, state);

        // Instructions bar
        let instructions = InstructionBar::new(vec![
            ("Esc/q", "Close"),
            ("↑↓/jk", "Scroll"),
            ("PgUp/Dn", "Page"),
            ("Type", "Search"),
        ]);
        instructions.render(chunks[2], buf);
    }

    fn render_search_bar(&self, area: Rect, buf: &mut Buffer, state: &HelpDialogState) {
        // Search box with border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Rgb(50, 50, 65)))
            .title(" Search ");

        let inner = block.inner(area);
        block.render(area, buf);

        // Search text
        let search_text = if state.search.value().is_empty() {
            "Type to filter...".to_string()
        } else {
            state.search.value().to_string()
        };

        let search_style = if state.search.value().is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };

        Paragraph::new(search_text)
            .style(search_style)
            .render(inner, buf);

        // Cursor
        if !state.search.value().is_empty() {
            let cursor_x = inner.x + state.search.cursor as u16;
            if cursor_x < inner.x + inner.width {
                buf[(cursor_x, inner.y)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }
    }

    fn render_content(&self, area: Rect, buf: &mut Buffer, state: &HelpDialogState) {
        let filtered = state.filtered_entries();

        if filtered.is_empty() {
            let no_results = Paragraph::new("No matching keybindings")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            no_results.render(area, buf);
            return;
        }

        // Build all lines with proper formatting
        let mut all_lines: Vec<(LineType, String, String)> = Vec::new();

        for (i, (category, entries)) in filtered.iter().enumerate() {
            // Add blank line between categories (but not before first)
            if i > 0 {
                all_lines.push((LineType::Blank, String::new(), String::new()));
            }

            // Category header
            all_lines.push((LineType::Header, category.title().to_string(), String::new()));

            // Entries
            for entry in entries {
                all_lines.push((
                    LineType::Entry,
                    entry.key_display.clone(),
                    entry.action_description.clone(),
                ));
            }
        }

        // Find max key width from ALL entries (not just visible) for consistent alignment
        let max_key_width = all_lines
            .iter()
            .filter(|(lt, _, _)| matches!(lt, LineType::Entry))
            .map(|(_, key, _)| key.len())
            .max()
            .unwrap_or(8)
            .max(8) as u16;

        // Apply scroll offset and render visible lines
        let visible_lines: Vec<_> = all_lines
            .iter()
            .skip(state.scroll_offset)
            .take(area.height as usize)
            .collect();

        for (i, (line_type, left, right)) in visible_lines.iter().enumerate() {
            let y = area.y + i as u16;
            if y >= area.y + area.height {
                break;
            }

            match line_type {
                LineType::Blank => {}
                LineType::Header => {
                    // Category header in yellow bold
                    Paragraph::new(left.as_str())
                        .style(
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )
                        .render(
                            Rect { x: area.x, y, width: area.width, height: 1 },
                            buf,
                        );
                }
                LineType::Entry => {
                    // Key in cyan, right-aligned in fixed width column
                    let key_area = Rect {
                        x: area.x,
                        y,
                        width: max_key_width + 2,
                        height: 1,
                    };
                    Paragraph::new(left.as_str())
                        .style(Style::default().fg(Color::Cyan))
                        .alignment(Alignment::Right)
                        .render(key_area, buf);

                    // Description in white
                    let desc_x = area.x + max_key_width + 3;
                    let desc_width = area.width.saturating_sub(max_key_width + 3);
                    if desc_width > 0 {
                        // Truncate if needed
                        let desc = if right.len() > desc_width as usize {
                            format!("{}...", &right[..desc_width.saturating_sub(3) as usize])
                        } else {
                            right.clone()
                        };

                        Paragraph::new(desc)
                            .style(Style::default().fg(Color::White))
                            .render(
                                Rect { x: desc_x, y, width: desc_width, height: 1 },
                                buf,
                            );
                    }
                }
            }
        }
    }

    fn render_scrollbar(&self, area: Rect, buf: &mut Buffer, state: &HelpDialogState) {
        render_vertical_scrollbar(
            area,
            buf,
            state.total_lines,
            state.visible_height,
            state.scroll_offset,
            ScrollbarSymbols::standard(),
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum LineType {
    Blank,
    Header,
    Entry,
}

impl Default for HelpDialog {
    fn default() -> Self {
        Self::new()
    }
}
