//! Model selector dialog component

use std::collections::HashSet;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::agent::{AgentType, ModelInfo, ModelRegistry};
use crate::ui::components::{
    accent_primary, bg_highlight, dialog_bg, ensure_contrast_bg, ensure_contrast_fg,
    render_minimal_scrollbar, text_muted, text_primary, text_secondary, DialogFrame,
    InstructionBar, TextInputState,
};

/// Represents an item in the model selector (either a section header or a model)
#[derive(Debug, Clone)]
pub enum ModelSelectorItem {
    SectionHeader(AgentType),
    Model(ModelInfo),
}

/// Default model selection (single agent + model pair)
#[derive(Debug, Clone, Default)]
pub struct DefaultModelSelection {
    pub agent_type: Option<AgentType>,
    pub model_id: Option<String>,
}

impl DefaultModelSelection {
    pub fn is_default(&self, model: &ModelInfo) -> bool {
        self.agent_type == Some(model.agent_type)
            && self.model_id.as_deref().is_some_and(|id| id == model.id)
    }

    pub fn set(&mut self, agent_type: AgentType, model_id: String) {
        self.agent_type = Some(agent_type);
        self.model_id = Some(model_id);
    }
}

const DIALOG_WIDTH: u16 = 60;
const DIALOG_HEIGHT: u16 = 18;

/// State for the model selector dialog
#[derive(Debug, Clone)]
pub struct ModelSelectorState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// Currently selected index (among filtered items only)
    selected: usize,
    /// All items (headers + models)
    items: Vec<ModelSelectorItem>,
    /// Indices of selectable items (models only)
    selectable_indices: Vec<usize>,
    /// Indices of selectable items matching the search
    filtered: Vec<usize>,
    /// Scroll offset in rendered list rows
    scroll_offset: usize,
    /// Maximum visible list rows
    max_visible: usize,
    /// Search input
    search: TextInputState,
    /// Currently active model ID (shows checkmark)
    current_model_id: Option<String>,
    /// Default model IDs (per agent)
    default_model: DefaultModelSelection,
}

impl Default for ModelSelectorState {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelSelectorState {
    pub fn new() -> Self {
        let items = Self::build_items();
        let selectable_indices: Vec<usize> = items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                ModelSelectorItem::Model(_) => Some(i),
                ModelSelectorItem::SectionHeader(_) => None,
            })
            .collect();

        Self {
            visible: false,
            selected: 0,
            items,
            selectable_indices: selectable_indices.clone(),
            filtered: selectable_indices,
            scroll_offset: 0,
            max_visible: 10,
            search: TextInputState::new(),
            current_model_id: None,
            default_model: DefaultModelSelection::default(),
        }
    }

    fn build_items() -> Vec<ModelSelectorItem> {
        let mut items = Vec::new();

        // Claude Code section
        items.push(ModelSelectorItem::SectionHeader(AgentType::Claude));
        for model in ModelRegistry::claude_models() {
            items.push(ModelSelectorItem::Model(model));
        }

        // Codex section
        items.push(ModelSelectorItem::SectionHeader(AgentType::Codex));
        for model in ModelRegistry::codex_models() {
            items.push(ModelSelectorItem::Model(model));
        }

        // Gemini section
        items.push(ModelSelectorItem::SectionHeader(AgentType::Gemini));
        for model in ModelRegistry::gemini_models() {
            items.push(ModelSelectorItem::Model(model));
        }

        items
    }

    /// Show the dialog, optionally setting the current model
    pub fn show(&mut self, current_model_id: Option<String>, default_model: DefaultModelSelection) {
        self.visible = true;
        self.current_model_id = current_model_id;
        self.default_model = default_model;
        self.search.clear();
        self.scroll_offset = 0;

        // Rebuild items to pick up registry changes
        self.items = Self::build_items();
        self.selectable_indices = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                ModelSelectorItem::Model(_) => Some(i),
                ModelSelectorItem::SectionHeader(_) => None,
            })
            .collect();

        self.update_filter();
        self.select_current_model();
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Update the list viewport height (based on screen size).
    pub fn update_viewport(&mut self, area: Rect) {
        if let Some(layout) = self.layout(area) {
            self.max_visible = layout.list_area.height.max(1) as usize;
            self.ensure_visible();
        }
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.filtered.len() - 1;
        }
        self.ensure_visible();
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
        self.ensure_visible();
    }

    /// Select item at a given visual row (for mouse clicks)
    /// Returns true if a model was selected.
    pub fn select_at_row(&mut self, row: usize) -> bool {
        let target_render = self.scroll_offset + row;
        let mut seen_headers: HashSet<AgentType> = HashSet::new();
        let mut render_idx = 0usize;

        for (filter_idx, &item_idx) in self.filtered.iter().enumerate() {
            if let ModelSelectorItem::Model(ref model) = self.items[item_idx] {
                if seen_headers.insert(model.agent_type) {
                    if render_idx == target_render {
                        return false;
                    }
                    render_idx += 1;
                }

                if render_idx == target_render {
                    self.selected = filter_idx;
                    self.ensure_visible();
                    return true;
                }

                render_idx += 1;
            }
        }

        false
    }

    /// Get the currently selected model
    pub fn selected_model(&self) -> Option<&ModelInfo> {
        let item_idx = self.filtered.get(self.selected)?;
        match &self.items[*item_idx] {
            ModelSelectorItem::Model(model) => Some(model),
            ModelSelectorItem::SectionHeader(_) => None,
        }
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Insert a character into search
    pub fn insert_char(&mut self, c: char) {
        self.search.insert_char(c);
        self.update_filter();
    }

    /// Insert text into search (paste)
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        for ch in s.chars() {
            if ch.is_control() {
                continue;
            }
            self.search.insert_char(ch);
        }
        self.update_filter();
    }

    /// Delete character before cursor
    pub fn delete_char(&mut self) {
        self.search.delete_char();
        self.update_filter();
    }

    /// Delete character at cursor
    pub fn delete_forward(&mut self) {
        self.search.delete_forward();
        self.update_filter();
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        self.search.move_left();
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        self.search.move_right();
    }

    /// Move cursor to start
    pub fn move_cursor_start(&mut self) {
        self.search.move_start();
    }

    /// Move cursor to end
    pub fn move_cursor_end(&mut self) {
        self.search.move_end();
    }

    /// Update default model IDs (per agent)
    pub fn set_default_model(&mut self, agent_type: AgentType, model_id: String) {
        self.default_model.set(agent_type, model_id);
    }

    pub fn default_model(&self) -> &DefaultModelSelection {
        &self.default_model
    }

    fn update_filter(&mut self) {
        let query = self.search.value().trim().to_lowercase();
        if query.is_empty() {
            self.filtered = self.selectable_indices.clone();
        } else {
            self.filtered = self
                .selectable_indices
                .iter()
                .filter_map(|&idx| match &self.items[idx] {
                    ModelSelectorItem::Model(model) => {
                        let matches = model.display_name.to_lowercase().contains(&query)
                            || model.id.to_lowercase().contains(&query)
                            || model.alias.to_lowercase().contains(&query)
                            || model.agent_type.as_str().contains(&query);
                        if matches {
                            Some(idx)
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect();
        }

        if self.filtered.is_empty() {
            self.selected = 0;
            self.scroll_offset = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
        self.ensure_visible();
    }

    fn select_current_model(&mut self) {
        if let Some(ref model_id) = self.current_model_id {
            for (filter_idx, &item_idx) in self.filtered.iter().enumerate() {
                if let ModelSelectorItem::Model(ref model) = self.items[item_idx] {
                    if &model.id == model_id {
                        self.selected = filter_idx;
                        self.ensure_visible();
                        return;
                    }
                }
            }
        }
        self.selected = 0;
        self.ensure_visible();
    }

    fn render_index_for_filtered(&self, target_filter_idx: usize) -> usize {
        let mut seen_headers: HashSet<AgentType> = HashSet::new();
        let mut render_index = 0usize;

        for (filter_idx, &item_idx) in self.filtered.iter().enumerate() {
            if let ModelSelectorItem::Model(ref model) = self.items[item_idx] {
                if seen_headers.insert(model.agent_type) {
                    render_index += 1;
                }

                if filter_idx == target_filter_idx {
                    return render_index;
                }

                render_index += 1;
            }
        }

        0
    }

    fn ensure_visible(&mut self) {
        if self.filtered.is_empty() {
            self.scroll_offset = 0;
            return;
        }
        let render_index = self.render_index_for_filtered(self.selected);
        if render_index < self.scroll_offset {
            self.scroll_offset = render_index;
        } else if render_index >= self.scroll_offset + self.max_visible {
            self.scroll_offset = render_index.saturating_sub(self.max_visible - 1);
        }
    }

    fn render_len(&self) -> usize {
        let mut seen_headers: HashSet<AgentType> = HashSet::new();
        let mut count = 0usize;
        for &item_idx in &self.filtered {
            if let ModelSelectorItem::Model(ref model) = self.items[item_idx] {
                if seen_headers.insert(model.agent_type) {
                    count += 1;
                }
                count += 1;
            }
        }
        count
    }

    fn layout(&self, area: Rect) -> Option<ModelSelectorLayout> {
        if area.width < 10 || area.height < 6 {
            return None;
        }

        let dialog_width = DIALOG_WIDTH.min(area.width.saturating_sub(4));
        let dialog_height = DIALOG_HEIGHT.min(area.height.saturating_sub(2));

        let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        };

        // DialogFrame adds 1 char padding on each side horizontally.
        let inner = Rect {
            x: dialog_area.x.saturating_add(2),
            y: dialog_area.y.saturating_add(1),
            width: dialog_area.width.saturating_sub(4),
            height: dialog_area.height.saturating_sub(2),
        };

        if inner.height < 4 {
            return None;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1), // Search
            Constraint::Length(1), // Separator
            Constraint::Min(1),    // List
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        Some(ModelSelectorLayout {
            dialog_area,
            search_area: chunks[0],
            separator_area: chunks[1],
            list_area: chunks[2],
            instructions_area: chunks[3],
        })
    }
}

struct ModelSelectorLayout {
    dialog_area: Rect,
    search_area: Rect,
    separator_area: Rect,
    list_area: Rect,
    instructions_area: Rect,
}

/// Model selector dialog widget
pub struct ModelSelector;

impl ModelSelector {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &ModelSelectorState) {
        if !state.visible {
            return;
        }

        let Some(layout) = state.layout(area) else {
            return;
        };

        // Render dialog frame
        let frame = DialogFrame::new("Model", layout.dialog_area.width, layout.dialog_area.height);
        let inner = frame.render(area, buf);

        if inner.height < 4 {
            return;
        }

        // Render search box
        Self::render_search(state, layout.search_area, buf);

        // Render separator
        Self::render_separator(layout.separator_area, buf);

        // Render list
        Self::render_list(state, layout.list_area, buf);

        // Render instructions
        let instructions = InstructionBar::new(vec![
            ("Enter", "Select"),
            ("M-d", "Default"),
            ("Esc", "Cancel"),
            ("\u{2191}\u{2193}", "Navigate"),
        ]);
        instructions.render(layout.instructions_area, buf);
    }

    fn render_search(state: &ModelSelectorState, area: Rect, buf: &mut Buffer) {
        let prompt = "Search: ";
        let input = state.search.value();

        let (line, show_placeholder) = if input.is_empty() {
            let placeholder = "type to filter models...";
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
                    Span::styled(input, Style::default().fg(text_primary())),
                ]),
                false,
            )
        };

        Paragraph::new(line).render(area, buf);

        // Render cursor
        let prompt_width = UnicodeWidthStr::width(prompt) as u16;
        let prefix = &state.search.input[..state.search.cursor.min(state.search.input.len())];
        let cursor_offset: u16 = prefix
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

    fn render_separator(area: Rect, buf: &mut Buffer) {
        let separator = "\u{2500}".repeat(area.width as usize);
        let para = Paragraph::new(separator).style(Style::default().fg(text_muted()));
        para.render(area, buf);
    }

    fn render_list(state: &ModelSelectorState, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let selected_bg_color = ensure_contrast_bg(bg_highlight(), dialog_bg(), 2.0);
        let selected_fg_color = ensure_contrast_fg(text_primary(), selected_bg_color, 4.5);
        let line_width = area.width.saturating_sub(1);

        let total_items = state.render_len();
        let scroll = state
            .scroll_offset
            .min(total_items.saturating_sub(area.height as usize));

        let mut seen_headers: HashSet<AgentType> = HashSet::new();
        let mut render_index = 0usize;
        let mut visible_index = 0usize;

        for (filter_idx, &item_idx) in state.filtered.iter().enumerate() {
            let ModelSelectorItem::Model(model) = &state.items[item_idx] else {
                continue;
            };

            if seen_headers.insert(model.agent_type) {
                if render_index >= scroll && visible_index < area.height as usize {
                    let title = ModelRegistry::agent_section_title(model.agent_type);
                    let header_line = Line::from(Span::styled(
                        title,
                        Style::default()
                            .fg(text_secondary())
                            .add_modifier(Modifier::BOLD),
                    ));
                    Paragraph::new(header_line).render(
                        Rect {
                            x: area.x,
                            y: area.y + visible_index as u16,
                            width: line_width,
                            height: 1,
                        },
                        buf,
                    );
                    visible_index += 1;
                }
                render_index += 1;
            }

            if render_index < scroll {
                render_index += 1;
                continue;
            }

            if visible_index >= area.height as usize {
                break;
            }

            let is_selected = filter_idx == state.selected;
            let is_current = state
                .current_model_id
                .as_ref()
                .map(|id| id == &model.id)
                .unwrap_or(false);
            let is_default = state.default_model.is_default(model);

            let row_rect = Rect {
                x: area.x,
                y: area.y + visible_index as u16,
                width: line_width,
                height: 1,
            };

            if is_selected {
                buf.set_style(row_rect, Style::default().bg(selected_bg_color));
            }

            let icon = ModelRegistry::agent_icon(model.agent_type);
            let mut spans = vec![
                Span::styled(format!("  {} ", icon), Style::default().fg(text_primary())),
                Span::styled(
                    &model.display_name,
                    if is_selected {
                        Style::default()
                            .fg(selected_fg_color)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(text_primary())
                    },
                ),
            ];

            if is_default {
                spans.push(Span::raw("  "));
                spans.push(Span::styled("DEFAULT", Style::default().fg(text_muted())));
            }

            let content_len: usize = spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            let checkmark_col = line_width.saturating_sub(2) as usize;

            if is_current && content_len < checkmark_col {
                let padding = checkmark_col.saturating_sub(content_len);
                spans.push(Span::raw(" ".repeat(padding)));
                let checkmark_fg = if is_selected {
                    selected_fg_color
                } else {
                    text_primary()
                };
                spans.push(Span::styled("✓", Style::default().fg(checkmark_fg)));
            }

            let line = Line::from(spans);
            Paragraph::new(line).render(row_rect, buf);

            render_index += 1;
            visible_index += 1;
        }

        // Empty state
        if state.filtered.is_empty() {
            let empty = Paragraph::new("No models match your search")
                .style(Style::default().fg(text_muted()));
            empty.render(area, buf);
        }

        if total_items > area.height as usize {
            let scrollbar_area = Rect {
                x: area.x + area.width - 1,
                y: area.y,
                width: 1,
                height: area.height,
            };
            render_minimal_scrollbar(
                scrollbar_area,
                buf,
                total_items,
                area.height as usize,
                scroll,
            );
        }
    }
}

impl Default for ModelSelector {
    fn default() -> Self {
        Self::new()
    }
}
