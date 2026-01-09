//! Theme picker dialog component
//!
//! Allows users to browse and select themes with live preview.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    accent_primary, bg_highlight, dialog_bg, ensure_contrast_bg, ensure_contrast_fg,
    render_minimal_scrollbar, text_muted, text_primary, text_secondary, DialogFrame,
    InstructionBar,
};
use crate::ui::components::theme::{
    current_theme_name, list_themes, load_theme_by_name, load_theme_from_path, ThemeInfo,
    ThemeSource,
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
    item_idx: usize,
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
    search_cursor: usize,
    /// Filtered selectable indices
    pub filtered: Vec<usize>,
    /// Scroll offset for the list
    pub scroll_offset: usize,
    /// Maximum visible items
    pub max_visible: usize,
    /// Theme name when dialog was opened (for cancel/restore)
    pub original_theme_name: Option<String>,
    /// Theme path when dialog was opened (for cancel/restore)
    pub original_theme_path: Option<PathBuf>,
    /// Currently previewing theme name
    pub preview_theme: Option<String>,
    /// Pending preview request (debounced)
    pending_preview: Option<PendingPreview>,
    /// Last preview error (for footer message)
    last_error: Option<String>,
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
            original_theme_name: None,
            original_theme_path: None,
            preview_theme: None,
            pending_preview: None,
            last_error: None,
        }
    }

    /// Show the theme picker dialog
    pub fn show(&mut self, theme_path: Option<&std::path::Path>) {
        self.visible = true;
        self.original_theme_name = Some(current_theme_name());
        self.original_theme_path = theme_path.map(|path| path.to_path_buf());
        self.preview_theme = None;
        self.pending_preview = None;
        self.last_error = None;
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

    /// Update the list viewport height (based on the screen size).
    pub fn update_viewport(&mut self, area: Rect) {
        let dialog_height = 18u16.min(area.height.saturating_sub(2));
        let inner_height = dialog_height.saturating_sub(2);
        let list_height = inner_height.saturating_sub(3).max(1);
        self.max_visible = list_height as usize;
        self.ensure_visible();
    }

    /// Hide the dialog and restore original theme if cancelled
    pub fn hide(&mut self, cancelled: bool) {
        if cancelled {
            self.restore_original_theme();
        }
        self.visible = false;
        self.preview_theme = None;
        self.pending_preview = None;
        if !cancelled {
            self.last_error = None;
        }
    }

    /// Check if visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    fn restore_original_theme(&mut self) -> bool {
        if let Some(path) = self.original_theme_path.as_ref() {
            if load_theme_from_path(path) {
                return true;
            }
            tracing::warn!(
                path = %path.display(),
                "Failed to restore original theme from path"
            );
        }

        if let Some(name) = self.original_theme_name.as_ref() {
            if load_theme_by_name(name) {
                return true;
            }
            tracing::warn!(
                theme = %name,
                "Failed to restore original theme by name"
            );
            self.last_error = Some(format!("Failed to restore theme: {name}"));
        } else {
            self.last_error = Some("Failed to restore theme".to_string());
        }

        false
    }

    /// Build the list of items grouped by source
    fn build_items() -> Vec<ThemePickerItem> {
        let themes = list_themes();
        let mut items = Vec::new();

        // Group themes by source
        let mut builtin: Vec<&ThemeInfo> = Vec::new();
        let mut vscode: Vec<&ThemeInfo> = Vec::new();
        let mut custom: Vec<&ThemeInfo> = Vec::new();
        let mut seen_builtin: HashSet<String> = HashSet::new();
        let mut seen_vscode: HashSet<String> = HashSet::new();
        let mut seen_custom: HashSet<String> = HashSet::new();

        let normalize_key = |name: &str| name.trim().to_lowercase();

        for theme in &themes {
            match &theme.source {
                ThemeSource::Builtin => {
                    let key = normalize_key(&theme.display_name);
                    if seen_builtin.insert(key) {
                        builtin.push(theme);
                    } else {
                        tracing::debug!(
                            display = %theme.display_name,
                            "Skipping duplicate built-in theme"
                        );
                    }
                }
                ThemeSource::VsCodeExtension { .. } => {
                    let key = normalize_key(&theme.display_name);
                    if seen_vscode.insert(key) {
                        vscode.push(theme);
                    } else {
                        tracing::debug!(
                            display = %theme.display_name,
                            "Skipping duplicate VS Code theme"
                        );
                    }
                }
                ThemeSource::CustomPath { .. } => {
                    let key = normalize_key(&theme.display_name);
                    if seen_custom.insert(key) {
                        custom.push(theme);
                    } else {
                        tracing::debug!(
                            display = %theme.display_name,
                            "Skipping duplicate custom theme"
                        );
                    }
                }
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
                if theme_matches_current(&current, info) {
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
        if self.last_error.is_some() {
            return None;
        }
        if let Some(theme) = self.selected_theme() {
            let name = theme.name.clone();
            // Theme is already applied via preview, just confirm it.
            self.last_error = None;
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

    fn theme_key(info: &ThemeInfo) -> String {
        match &info.source {
            ThemeSource::CustomPath { path } => format!("path:{}", path.display()),
            _ => format!("name:{}", info.name),
        }
    }

    fn apply_theme_info(&self, info: &ThemeInfo) -> bool {
        match &info.source {
            ThemeSource::CustomPath { path } => load_theme_from_path(path),
            _ => load_theme_by_name(&info.name),
        }
    }

    /// Queue the currently selected theme as preview (debounced)
    fn queue_preview(&mut self) {
        let Some(&item_idx) = self.filtered.get(self.selected) else {
            self.pending_preview = None;
            return;
        };
        let ThemePickerItem::Theme(info) = &self.items[item_idx] else {
            self.pending_preview = None;
            return;
        };
        let key = Self::theme_key(info);
        if self.preview_theme.as_ref() == Some(&key) {
            self.pending_preview = None;
            return;
        }
        tracing::debug!(theme = %key, "Theme preview queued");
        self.pending_preview = Some(PendingPreview {
            item_idx,
            requested_at: Instant::now(),
        });
    }

    /// Apply the currently selected theme immediately as preview
    fn apply_preview_now(&mut self) {
        let Some(info) = self.selected_theme().cloned() else {
            self.pending_preview = None;
            return;
        };
        let key = Self::theme_key(&info);
        if self.preview_theme.as_ref() != Some(&key) {
            if self.apply_theme_info(&info) {
                tracing::debug!(theme = %key, "Theme preview applied immediately");
                self.preview_theme = Some(key);
                self.last_error = None;
            } else {
                tracing::warn!(theme = %key, "Theme preview failed to load");
                self.last_error = Some(format!("Failed to load theme: {}", info.display_name));
            }
        } else {
            self.last_error = None;
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
            let item_idx = pending.item_idx;
            if let Some(ThemePickerItem::Theme(info)) = self.items.get(item_idx) {
                let key = Self::theme_key(info);
                if self.preview_theme.as_ref() != Some(&key) {
                    if self.apply_theme_info(info) {
                        tracing::debug!(theme = %key, "Theme preview applied");
                        self.preview_theme = Some(key);
                        self.last_error = None;
                    } else {
                        tracing::warn!(theme = %key, "Theme preview failed to load");
                        self.last_error =
                            Some(format!("Failed to load theme: {}", info.display_name));
                    }
                } else {
                    self.last_error = None;
                }
            }
            self.pending_preview = None;
        }
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    /// Ensure the selected item is visible
    fn ensure_visible(&mut self) {
        if self.max_visible == 0 || self.filtered.is_empty() {
            self.scroll_offset = 0;
            return;
        }

        let mut seen_headers: HashSet<String> = HashSet::new();
        let mut render_index = 0usize;
        let mut selected_render_index = None;

        for (filter_idx, &item_idx) in self.filtered.iter().enumerate() {
            if let ThemePickerItem::Theme(_) = self.items[item_idx] {
                let mut header_text: Option<String> = None;
                for i in (0..item_idx).rev() {
                    if let ThemePickerItem::SectionHeader(ref header) = self.items[i] {
                        header_text = Some(header.clone());
                        break;
                    }
                }
                if let Some(header_text) = header_text {
                    if seen_headers.insert(header_text) {
                        render_index += 1;
                    }
                }

                if filter_idx == self.selected {
                    selected_render_index = Some(render_index);
                    break;
                }

                render_index += 1;
            }
        }

        let selected_render_index = selected_render_index.unwrap_or(0);
        if selected_render_index < self.scroll_offset {
            self.scroll_offset = selected_render_index;
        } else if selected_render_index >= self.scroll_offset + self.max_visible {
            self.scroll_offset = selected_render_index.saturating_sub(self.max_visible - 1);
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
        let previous_key = self.selected_theme().map(Self::theme_key);
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

        // Preserve selection when possible
        self.selected = 0;
        if let Some(key) = previous_key {
            if let Some((idx, _)) = self.filtered.iter().enumerate().find(|(_, &item_idx)| {
                if let ThemePickerItem::Theme(info) = &self.items[item_idx] {
                    Self::theme_key(info) == key
                } else {
                    false
                }
            }) {
                self.selected = idx;
            }
        }
        self.scroll_offset = 0;
        self.ensure_visible();

        // Apply preview for new selection
        self.queue_preview();
    }
}

fn theme_matches_current(current: &str, info: &ThemeInfo) -> bool {
    let current_norm = current.trim().to_lowercase();
    if current_norm.is_empty() {
        return false;
    }
    let name_norm = info.name.trim().to_lowercase();
    let display_norm = info.display_name.trim().to_lowercase();
    name_norm == current_norm || display_norm == current_norm
}

/// Theme picker dialog widget
pub struct ThemePicker<'a> {
    state: &'a ThemePickerState,
}

struct RenderListContext {
    area: Rect,
    line_width: u16,
    selected_bg: Color,
    selected_fg: Color,
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
    fn build_render_items(&self, current_theme: &str) -> Vec<(bool, String, bool, bool)> {
        let mut render_items = Vec::new();
        let mut seen_headers: HashSet<String> = HashSet::new();

        for (filter_idx, &item_idx) in self.state.filtered.iter().enumerate() {
            if let ThemePickerItem::Theme(ref info) = self.state.items[item_idx] {
                for i in (0..item_idx).rev() {
                    if let ThemePickerItem::SectionHeader(ref header) = self.state.items[i] {
                        if seen_headers.insert(header.clone()) {
                            render_items.push((true, header.clone(), false, false));
                        }
                        break;
                    }
                }

                let is_selected = filter_idx == self.state.selected;
                let is_current = theme_matches_current(current_theme, info);
                let display = if is_current {
                    format!("\u{2713} {}", info.display_name)
                } else {
                    format!("  {}", info.display_name)
                };
                render_items.push((false, display, is_selected, is_current));
            }
        }

        render_items
    }

    fn render_list_item(
        &self,
        buf: &mut Buffer,
        y: u16,
        text: &str,
        is_header: bool,
        is_selected: bool,
        ctx: &RenderListContext,
    ) {
        if is_selected {
            let fill_style = Style::default().bg(ctx.selected_bg);
            for x in ctx.area.x..ctx.area.x.saturating_add(ctx.line_width) {
                buf[(x, y)].set_style(fill_style);
            }
        }

        let style = if is_header {
            Style::default()
                .fg(text_secondary())
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(ctx.selected_fg).bg(ctx.selected_bg)
        } else {
            Style::default().fg(text_primary())
        };

        let line = Line::from(Span::styled(text, style));
        Paragraph::new(line).render(
            Rect {
                x: ctx.area.x,
                y,
                width: ctx.line_width,
                height: 1,
            },
            buf,
        );
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer) {
        let prompt = "> ";
        let input = &self.state.search;

        let (line, show_placeholder) = if input.is_empty() {
            let placeholder = " Type to filter themes...";
            (
                Line::from(vec![
                    Span::styled(prompt, Style::default().fg(accent_primary())),
                    Span::styled(placeholder, Style::default().fg(text_muted())),
                ]),
                true,
            )
        } else {
            (
                Line::from(vec![
                    Span::styled(prompt, Style::default().fg(accent_primary())),
                    Span::styled(input.as_str(), Style::default().fg(text_primary())),
                ]),
                false,
            )
        };

        Paragraph::new(line).render(area, buf);

        // Render cursor
        let prompt_width = UnicodeWidthStr::width(prompt) as u16;
        let cursor_offset: u16 = input[..self.state.search_cursor.min(input.len())]
            .chars()
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(1) as u16)
            .sum();
        let cursor_x = area.x + prompt_width + cursor_offset;
        if cursor_x < area.x + area.width {
            if show_placeholder {
                buf[(cursor_x, area.y)]
                    .set_char(' ')
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            } else {
                buf[(cursor_x, area.y)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
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

        let render_items = self.build_render_items(&current_theme);

        // Calculate scroll offset considering headers
        let total_items = render_items.len();
        let scroll = self
            .state
            .scroll_offset
            .min(total_items.saturating_sub(list_height));

        // Render visible items
        let ctx = RenderListContext {
            area,
            line_width: area.width.saturating_sub(1), // Leave room for scrollbar
            selected_bg,
            selected_fg,
        };
        for (rendered, (_idx, (is_header, text, is_selected, _is_current))) in
            render_items.iter().enumerate().skip(scroll).enumerate()
        {
            if rendered >= list_height {
                break;
            }
            let y = area.y + rendered as u16;
            self.render_list_item(buf, y, text, *is_header, *is_selected, &ctx);
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
