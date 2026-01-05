//! Model selector dialog component

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::agent::{AgentType, ModelInfo, ModelRegistry};

/// Represents an item in the model selector (either a section header or a model)
#[derive(Debug, Clone)]
pub enum ModelSelectorItem {
    SectionHeader(AgentType),
    Model(ModelInfo),
}

/// State for the model selector dialog
#[derive(Debug, Clone)]
pub struct ModelSelectorState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// Currently selected index (among selectable items only)
    pub selected: usize,
    /// All items (headers + models)
    pub items: Vec<ModelSelectorItem>,
    /// Indices of selectable items (models only)
    pub selectable_indices: Vec<usize>,
    /// Currently active model ID (shows checkmark)
    pub current_model_id: Option<String>,
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
            selectable_indices,
            current_model_id: None,
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

        items
    }

    /// Show the dialog, optionally setting the current model
    pub fn show(&mut self, current_model_id: Option<String>) {
        self.visible = true;
        self.current_model_id = current_model_id.clone();

        // Try to select the current model if provided
        if let Some(ref model_id) = current_model_id {
            for (select_idx, &item_idx) in self.selectable_indices.iter().enumerate() {
                if let ModelSelectorItem::Model(ref model) = self.items[item_idx] {
                    if &model.id == model_id {
                        self.selected = select_idx;
                        return;
                    }
                }
            }
        }
        self.selected = 0;
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else if !self.selectable_indices.is_empty() {
            self.selected = self.selectable_indices.len() - 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if !self.selectable_indices.is_empty() {
            self.selected = (self.selected + 1) % self.selectable_indices.len();
        }
    }

    /// Get the currently selected model
    pub fn selected_model(&self) -> Option<&ModelInfo> {
        let item_idx = self.selectable_indices.get(self.selected)?;
        match &self.items[*item_idx] {
            ModelSelectorItem::Model(model) => Some(model),
            ModelSelectorItem::SectionHeader(_) => None,
        }
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Get the item index for the currently selected item
    fn selected_item_index(&self) -> Option<usize> {
        self.selectable_indices.get(self.selected).copied()
    }
}

/// Model selector dialog widget
pub struct ModelSelector;

impl ModelSelector {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog positioned above the status bar
    pub fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &ModelSelectorState,
        status_bar_area: Option<Rect>,
    ) {
        if !state.visible {
            return;
        }

        // Calculate dialog size
        // Each section header: 1 line + spacing
        // Each model: 1 line
        // Plus borders and padding
        let content_height = state.items.len() as u16 + 4; // items + padding
        let dialog_height = content_height.min(area.height.saturating_sub(4));
        let dialog_width: u16 = 40;

        // Position dialog with bottom-left aligned above the agent name in status bar
        let (dialog_x, dialog_y) = if let Some(sb_area) = status_bar_area {
            // Align with the start of the agent badge
            let x = sb_area.x;
            // Position dialog so its bottom is one line above the status bar
            let y = sb_area.y.saturating_sub(dialog_height);
            (x, y)
        } else {
            // Fallback to centered if no status bar area
            let x = (area.width.saturating_sub(dialog_width)) / 2;
            let y = (area.height.saturating_sub(dialog_height)) / 2;
            (x, y)
        };

        // Ensure dialog stays within screen bounds
        let dialog_x = dialog_x.min(area.width.saturating_sub(dialog_width));
        let dialog_y = dialog_y.max(0);

        let dialog_area = Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width.min(area.width.saturating_sub(dialog_x)),
            height: dialog_height,
        };

        // Clear the dialog area
        Clear.render(dialog_area, buf);

        // Render dialog border with rounded corners
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(Color::Rgb(50, 50, 65)));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Add horizontal padding
        let inner = Rect {
            x: inner.x.saturating_add(1),
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height,
        };

        if inner.height < 4 {
            return;
        }

        let selected_item_idx = state.selected_item_index();

        // Render items
        let mut y = inner.y;
        for (item_idx, item) in state.items.iter().enumerate() {
            if y >= inner.y + inner.height.saturating_sub(1) {
                break;
            }

            match item {
                ModelSelectorItem::SectionHeader(agent_type) => {
                    // Add spacing before section (except first)
                    if item_idx > 0 {
                        y += 1;
                        if y >= inner.y + inner.height.saturating_sub(1) {
                            break;
                        }
                    }

                    // Render section header
                    let title = ModelRegistry::agent_section_title(*agent_type);
                    let header_line = Line::from(Span::styled(
                        title,
                        Style::default().fg(Color::DarkGray),
                    ));
                    let header = Paragraph::new(header_line);
                    header.render(
                        Rect {
                            x: inner.x + 1,
                            y,
                            width: inner.width.saturating_sub(2),
                            height: 1,
                        },
                        buf,
                    );
                    y += 1;
                }
                ModelSelectorItem::Model(model) => {
                    let is_selected = selected_item_idx == Some(item_idx);
                    let is_current = state
                        .current_model_id
                        .as_ref()
                        .map(|id| id == &model.id)
                        .unwrap_or(false);

                    // Build the line
                    let icon = ModelRegistry::agent_icon(model.agent_type);
                    let mut spans = vec![
                        Span::styled(
                            format!("  {} ", icon),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            &model.display_name,
                            if is_selected {
                                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            },
                        ),
                    ];

                    // Add NEW badge if applicable
                    if model.is_new {
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(
                            " NEW ",
                            Style::default()
                                .fg(Color::Rgb(180, 160, 140))
                                .bg(Color::Rgb(60, 50, 45)),
                        ));
                    }

                    // Calculate remaining space for checkmark
                    let content_len: usize = spans.iter().map(|s| s.content.len()).sum();
                    let checkmark_col = inner.width.saturating_sub(4) as usize;

                    if is_current && content_len < checkmark_col {
                        // Add padding to right-align checkmark
                        let padding = checkmark_col.saturating_sub(content_len);
                        spans.push(Span::raw(" ".repeat(padding)));
                        spans.push(Span::styled("✓", Style::default().fg(Color::White)));
                    }

                    let line = Line::from(spans);
                    let para = Paragraph::new(line);

                    let row_rect = Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: 1,
                    };

                    // Highlight selected row background
                    if is_selected {
                        for dx in 0..row_rect.width {
                            buf[(row_rect.x + dx, row_rect.y)]
                                .set_bg(Color::Rgb(50, 50, 50));
                        }
                    }

                    para.render(row_rect, buf);
                    y += 1;
                }
            }
        }
    }
}

impl Default for ModelSelector {
    fn default() -> Self {
        Self::new()
    }
}
