//! Theme picker dialog component
//!
//! Allows users to browse and select themes with live preview.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    accent_primary, bg_highlight, dialog_bg, ensure_contrast_bg, ensure_contrast_fg,
    render_minimal_scrollbar, text_muted, text_primary, text_secondary, DialogFrame,
    InstructionBar,
};
use crate::ui::components::theme::{
    current_theme_name, list_themes, load_theme_by_name, ThemeInfo, ThemeSource,
};

/// Represents an item in the theme picker (either a section header or a theme)
#[derive(Debug, Clone)]
pub enum ThemePickerItem {
    SectionHeader(String),
    Theme(ThemeInfo),
}

const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone)]
struct PendingPreview {
    name: String,
    requested_at: Instant,
}

/// State for the theme picker dialog
#[derive(Debug, Clone)]
pub struct ThemePickerState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// All items (headers + themes)
    pub items: Vec<ThemePickerItem>,
    /// Indices of selectable items (themes only)
    pub selectable_indices: Vec<usize>,
    /// Currently selected index (among selectable items)
    pub selected: usize,
    /// Search input
    pub search: String,
    /// Cursor position in search
    pub search_cursor: usize,
    /// Filtered selectable indices
    pub filtered: Vec<usize>,
    /// Scroll offset for the list
    pub scroll_offset: usize,
    /// Maximum visible items
    pub max_visible: usize,
    /// Theme name when dialog was opened (for cancel/restore)
    pub original_theme: String,
    /// Currently previewing theme name
    pub preview_theme: Option<String>,
    /// Pending preview request (debounced)
    pending_preview: Option<PendingPreview>,
}

impl Default for ThemePickerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemePickerState {
    pub fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            selectable_indices: Vec::new(),
            selected: 0,
            search: String::new(),
            search_cursor: 0,
            filtered: Vec::new(),
            scroll_offset: 0,
            max_visible: 10,
            original_theme: String::new(),
            preview_theme: None,
            pending_preview: None,
        }
    }

    /// Show the theme picker dialog
    pub fn show(&mut self) {
        self.visible = true;
        self.original_theme = current_theme_name();
        self.preview_theme = None;
        self.pending_preview = None;
        self.search.clear();
        self.search_cursor = 0;
        self.scroll_offset = 0;

        // Build items from available themes
        self.items = Self::build_items();
        self.selectable_indices = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                ThemePickerItem::Theme(_) => Some(i),
                ThemePickerItem::SectionHeader(_) => None,
            })
            .collect();

        // Initialize filtered to all selectable
        self.filtered = self.selectable_indices.clone();

        // Select the current theme
        self.select_current_theme();
    }

    /// Hide the dialog and restore original theme if cancelled
    pub fn hide(&mut self, cancelled: bool) {
        if cancelled && !self.original_theme.is_empty() {
            // Restore original theme
            load_theme_by_name(&self.original_theme);
        }
        self.visible = false;
        self.preview_theme = None;
        self.pending_preview = None;
    }

    /// Check if visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Build the list of items grouped by source
    fn build_items() -> Vec<ThemePickerItem> {
        let themes = list_themes();
        let mut items = Vec::new();

        // Group themes by source
        let mut builtin: Vec<&ThemeInfo> = Vec::new();
        let mut vscode: Vec<&ThemeInfo> = Vec::new();
        let mut custom: Vec<&ThemeInfo> = Vec::new();

        for theme in &themes {
            match &theme.source {
                ThemeSource::Builtin => builtin.push(theme),
                ThemeSource::VsCodeExtension { .. } => vscode.push(theme),
                ThemeSource::CustomPath { .. } => custom.push(theme),
            }
        }

        // Add built-in section
        if !builtin.is_empty() {
            items.push(ThemePickerItem::SectionHeader("Built-in".to_string()));
            for theme in builtin {
                items.push(ThemePickerItem::Theme(theme.clone()));
            }
        }

        // Add VS Code section
        if !vscode.is_empty() {
            items.push(ThemePickerItem::SectionHeader("VS Code".to_string()));
            for theme in vscode {
                items.push(ThemePickerItem::Theme(theme.clone()));
            }
        }

        // Add custom section
        if !custom.is_empty() {
            items.push(ThemePickerItem::SectionHeader("Custom".to_string()));
            for theme in custom {
                items.push(ThemePickerItem::Theme(theme.clone()));
            }
        }

        items
    }

    /// Select the current theme in the list
    fn select_current_theme(&mut self) {
        let current = current_theme_name();
        for (idx, &item_idx) in self.filtered.iter().enumerate() {
            if let ThemePickerItem::Theme(ref info) = self.items[item_idx] {
                if info.name == current {
                    self.selected = idx;
                    self.ensure_visible();
                    return;
                }
            }
        }
        self.selected = 0;
    }

    /// Get the currently selected theme info
    pub fn selected_theme(&self) -> Option<&ThemeInfo> {
        if self.filtered.is_empty() {
            return None;
        }
        let item_idx = self.filtered.get(self.selected)?;
        match &self.items[*item_idx] {
            ThemePickerItem::Theme(info) => Some(info),
            _ => None,
        }
    }

    /// Confirm selection and apply theme
    pub fn confirm(&mut self) -> Option<String> {
        self.apply_preview_now();
        if let Some(theme) = self.selected_theme() {
            let name = theme.name.clone();
            // Theme is already applied via preview, just confirm it
            self.original_theme = name.clone(); // Prevent restore on hide
            Some(name)
        } else {
            None
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.filtered.len() - 1;
        }
        self.ensure_visible();
        self.queue_preview();
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
        self.ensure_visible();
        self.queue_preview();
    }

    /// Queue the currently selected theme as preview (debounced)
    fn queue_preview(&mut self) {
        let theme_name = self.selected_theme().map(|t| t.name.clone());
        if let Some(name) = theme_name {
            if self.preview_theme.as_ref() == Some(&name) {
                self.pending_preview = None;
                return;
            }
            self.pending_preview = Some(PendingPreview {
                name,
                requested_at: Instant::now(),
            });
            if let Some(pending) = self.pending_preview.as_ref() {
                tracing::debug!(theme = %pending.name, "Theme preview queued");
            }
        } else {
            self.pending_preview = None;
        }
    }

    /// Apply the currently selected theme immediately as preview
    fn apply_preview_now(&mut self) {
        let theme_name = self.selected_theme().map(|t| t.name.clone());
        if let Some(name) = theme_name {
            if self.preview_theme.as_ref() != Some(&name) {
                load_theme_by_name(&name);
                tracing::debug!(theme = %name, "Theme preview applied immediately");
                self.preview_theme = Some(name);
            }
        }
        self.pending_preview = None;
    }

    /// Tick handler for debounced preview.
    pub fn tick(&mut self) {
        if !self.visible {
            self.pending_preview = None;
            return;
        }
        let Some(pending) = self.pending_preview.as_ref() else {
            return;
        };
        if pending.requested_at.elapsed() >= PREVIEW_DEBOUNCE {
            let name = pending.name.clone();
            if self.preview_theme.as_ref() != Some(&name) {
                load_theme_by_name(&name);
                tracing::debug!(theme = %name, "Theme preview applied");
                self.preview_theme = Some(name);
            }
            self.pending_preview = None;
        }
    }

    /// Ensure the selected item is visible
    fn ensure_visible(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected - self.max_visible + 1;
        }
    }

    /// Insert a character into search
    pub fn insert_char(&mut self, c: char) {
        self.search.insert(self.search_cursor, c);
        self.search_cursor += c.len_utf8();
        self.update_filter();
    }

    /// Delete character before cursor
    pub fn backspace(&mut self) {
        if self.search_cursor > 0 {
            let prev = self.search[..self.search_cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.search.remove(prev);
            self.search_cursor = prev;
            self.update_filter();
        }
    }

    /// Delete character at cursor
    pub fn delete(&mut self) {
        if self.search_cursor < self.search.len() {
            self.search.remove(self.search_cursor);
            self.update_filter();
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        if self.search_cursor > 0 {
            self.search_cursor = self.search[..self.search_cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        if self.search_cursor < self.search.len() {
            self.search_cursor = self.search[self.search_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.search_cursor + i)
                .unwrap_or(self.search.len());
        }
    }

    /// Update filtered list based on search
    fn update_filter(&mut self) {
        let query = self.search.to_lowercase();
        if query.is_empty() {
            self.filtered = self.selectable_indices.clone();
        } else {
            self.filtered = self
                .selectable_indices
                .iter()
                .copied()
                .filter(|&idx| {
                    if let ThemePickerItem::Theme(ref info) = self.items[idx] {
                        info.display_name.to_lowercase().contains(&query)
                            || info.name.to_lowercase().contains(&query)
                    } else {
                        false
                    }
                })
                .collect();
        }

        // Reset selection
        self.selected = 0;
        self.scroll_offset = 0;

        // Apply preview for new selection
        self.queue_preview();
    }
}

/// Theme picker dialog widget
pub struct ThemePicker<'a> {
    state: &'a ThemePickerState,
}

impl<'a> ThemePicker<'a> {
    pub fn new(state: &'a ThemePickerState) -> Self {
        Self { state }
    }
}

impl Widget for ThemePicker<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.state.visible {
            return;
        }

        let dialog_width: u16 = 50;
        let dialog_height: u16 = 18;

        // Render dialog frame
        let frame = DialogFrame::new("Theme", dialog_width, dialog_height);
        let inner = frame.render(area, buf);

        if inner.height < 5 {
            return;
        }

        // Layout: search box, separator, list, instructions
        let chunks = Layout::default()
            .constraints([
                Constraint::Length(1), // Search
                Constraint::Length(1), // Separator
                Constraint::Min(1),    // List
                Constraint::Length(1), // Instructions
            ])
            .split(inner);

        // Render search box
        self.render_search(chunks[0], buf);

        // Render separator
        self.render_separator(chunks[1], buf);

        // Render theme list
        self.render_list(chunks[2], buf);

        // Render instructions
        let instructions = InstructionBar::new(vec![
            ("Enter", "Select"),
            ("Esc", "Cancel"),
            ("\u{2191}\u{2193}", "Navigate"),
        ]);
        instructions.render(chunks[3], buf);
    }
}

impl ThemePicker<'_> {
    fn render_search(&self, area: Rect, buf: &mut Buffer) {
        let prompt = "> ";
        let input = &self.state.search;

        if input.is_empty() {
            // Show placeholder
            let placeholder = format!("{}Type to filter themes...", prompt);
            let para = Paragraph::new(placeholder).style(Style::default().fg(text_muted()));
            para.render(area, buf);
        } else {
            // Show prompt and input
            let line = Line::from(vec![
                Span::styled(prompt, Style::default().fg(accent_primary())),
                Span::styled(input.as_str(), Style::default().fg(text_primary())),
            ]);
            let para = Paragraph::new(line);
            para.render(area, buf);
        }

        // Render cursor
        let prompt_width = UnicodeWidthStr::width(prompt) as u16;
        let cursor_offset: u16 = input
            .chars()
            .take(self.state.search_cursor)
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(1) as u16)
            .sum();
        let cursor_x = area.x + prompt_width + cursor_offset;
        if cursor_x < area.x + area.width {
            buf[(cursor_x, area.y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    }

    fn render_separator(&self, area: Rect, buf: &mut Buffer) {
        let separator = "\u{2500}".repeat(area.width as usize);
        let para = Paragraph::new(separator).style(Style::default().fg(text_muted()));
        para.render(area, buf);
    }

    fn render_list(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let current_theme = current_theme_name();
        let selected_bg = ensure_contrast_bg(bg_highlight(), dialog_bg(), 2.0);
        let selected_fg = ensure_contrast_fg(text_primary(), selected_bg, 4.5);
        let list_height = area.height as usize;

        // Build visible items with section context
        let mut y = area.y;
        let mut rendered = 0;

        // We need to show items with their section headers
        // First, build a flat list of what to render
        let mut render_items: Vec<(bool, String, bool, bool)> = Vec::new(); // (is_header, text, is_selected, is_current)

        for (filter_idx, &item_idx) in self.state.filtered.iter().enumerate() {
            // Check if we need to show a section header before this item
            if let ThemePickerItem::Theme(ref info) = self.state.items[item_idx] {
                // Find the section header for this theme
                for i in (0..item_idx).rev() {
                    if let ThemePickerItem::SectionHeader(ref header) = self.state.items[i] {
                        // Check if this header was already added
                        let header_text = header.clone();
                        let already_added = render_items
                            .iter()
                            .any(|(is_h, text, _, _)| *is_h && text == &header_text);
                        if !already_added {
                            render_items.push((true, header_text, false, false));
                        }
                        break;
                    }
                }

                let is_selected = filter_idx == self.state.selected;
                let is_current = info.name == current_theme;
                let display = if is_current {
                    format!("\u{2713} {}", info.display_name)
                } else {
                    format!("  {}", info.display_name)
                };
                render_items.push((false, display, is_selected, is_current));
            }
        }

        // Calculate scroll offset considering headers
        let total_items = render_items.len();
        let scroll = self
            .state
            .scroll_offset
            .min(total_items.saturating_sub(list_height));

        // Render visible items
        let line_width = area.width.saturating_sub(1); // Leave room for scrollbar
        for (_idx, (is_header, text, is_selected, _is_current)) in
            render_items.iter().enumerate().skip(scroll)
        {
            if rendered >= list_height {
                break;
            }

            if *is_selected {
                let fill_style = Style::default().bg(selected_bg);
                for x in area.x..area.x.saturating_add(line_width) {
                    buf[(x, y)].set_style(fill_style);
                }
            }

            let style = if *is_header {
                Style::default()
                    .fg(text_secondary())
                    .add_modifier(Modifier::BOLD)
            } else if *is_selected {
                Style::default().fg(selected_fg).bg(selected_bg)
            } else {
                Style::default().fg(text_primary())
            };

            let line = Line::from(Span::styled(text.as_str(), style));
            let para = Paragraph::new(line);
            para.render(
                Rect {
                    x: area.x,
                    y,
                    width: line_width,
                    height: 1,
                },
                buf,
            );

            y += 1;
            rendered += 1;
        }

        // Render scrollbar if needed
        if total_items > list_height {
            let scrollbar_area = Rect {
                x: area.x + area.width - 1,
                y: area.y,
                width: 1,
                height: area.height,
            };
            render_minimal_scrollbar(scrollbar_area, buf, total_items, list_height, scroll);
        }
    }
}
