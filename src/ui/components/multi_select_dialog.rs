//! Reusable searchable multi-select dialog component.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{
    accent_primary, bg_highlight, dialog_bg, dialog_content_area, ensure_contrast_bg,
    ensure_contrast_fg, render_minimal_scrollbar, text_muted, text_primary, DialogFrame,
    SearchableListState,
};

const DIALOG_WIDTH: u16 = 72;
const DIALOG_HEIGHT: u16 = 18;

#[derive(Debug, Clone)]
pub struct MultiSelectItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub checked: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone)]
pub struct MultiSelectDialogState {
    pub visible: bool,
    pub title: String,
    pub subtitle: Option<String>,
    pub items: Vec<MultiSelectItem>,
    pub list: SearchableListState,
    pub validation_error: Option<String>,
}

impl MultiSelectDialogState {
    pub fn new(max_visible: usize) -> Self {
        Self {
            visible: false,
            title: String::new(),
            subtitle: None,
            items: Vec::new(),
            list: SearchableListState::new(max_visible),
            validation_error: None,
        }
    }

    pub fn configure(
        &mut self,
        title: impl Into<String>,
        subtitle: Option<String>,
        items: Vec<MultiSelectItem>,
    ) {
        self.title = title.into();
        self.subtitle = subtitle;
        self.items = items;
        self.list.reset();
        self.validation_error = None;
        self.update_filter();
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.validation_error = None;
        self.update_filter();
    }

    pub fn update_viewport(&mut self, area: Rect) {
        let max_visible = MultiSelectDialog::list_area(area).height.max(1) as usize;
        self.list.max_visible = max_visible;
        self.list.clamp_selection();
        if self.list.selected < self.list.scroll_offset {
            self.list.scroll_offset = self.list.selected;
        } else if self.list.selected >= self.list.scroll_offset + self.list.max_visible {
            self.list.scroll_offset = self
                .list
                .selected
                .saturating_sub(self.list.max_visible.saturating_sub(1));
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn update_filter(&mut self) {
        let query = self.list.search.value().trim().to_lowercase();
        let filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if query.is_empty() {
                    return true;
                }
                item.title.to_lowercase().contains(&query)
                    || item.description.to_lowercase().contains(&query)
                    || item.id.to_lowercase().contains(&query)
            })
            .map(|(idx, _)| idx)
            .collect();
        self.list.set_filtered(filtered);
    }

    pub fn insert_char(&mut self, c: char) {
        self.list.search.insert_char(c);
        self.update_filter();
    }

    pub fn insert_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch.is_control() {
                continue;
            }
            self.list.search.insert_char(ch);
        }
        self.update_filter();
    }

    pub fn delete_char(&mut self) {
        self.list.search.delete_char();
        self.update_filter();
    }

    pub fn delete_forward(&mut self) {
        self.list.search.delete_forward();
        self.update_filter();
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

    pub fn select_next(&mut self) {
        self.list.select_next();
    }

    pub fn select_previous(&mut self) {
        self.list.select_prev();
    }

    pub fn select_at_row(&mut self, row: usize) -> bool {
        self.list.select_at_row(row)
    }

    pub fn toggle_selected(&mut self) -> bool {
        let Some(idx) = self.list.filtered.get(self.list.selected).copied() else {
            return false;
        };
        let Some(item) = self.items.get_mut(idx) else {
            return false;
        };
        if item.disabled {
            return false;
        }
        item.checked = !item.checked;
        self.validation_error = None;
        true
    }

    pub fn selected_item(&self) -> Option<&MultiSelectItem> {
        let idx = self.list.filtered.get(self.list.selected)?;
        self.items.get(*idx)
    }

    pub fn selected_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.checked)
            .map(|item| item.id.clone())
            .collect()
    }
}

pub struct MultiSelectDialog;

impl MultiSelectDialog {
    pub fn new() -> Self {
        Self
    }

    pub fn dialog_area(area: Rect) -> Rect {
        let dialog_width = DIALOG_WIDTH.min(area.width.saturating_sub(4));
        let dialog_height = DIALOG_HEIGHT.min(area.height.saturating_sub(2));
        let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
        Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &MultiSelectDialogState) {
        if !state.visible {
            return;
        }

        let title: &str = if state.title.is_empty() {
            "Select"
        } else {
            &state.title
        };
        let frame = DialogFrame::new(title, DIALOG_WIDTH, DIALOG_HEIGHT).instructions(vec![
            ("↑↓", "select"),
            ("Space", "toggle"),
            ("Enter", "confirm"),
            ("Esc", "cancel"),
        ]);
        let inner = frame.render(area, buf);
        self.render_inner(inner, buf, state);
    }

    fn render_inner(&self, inner: Rect, buf: &mut Buffer, state: &MultiSelectDialogState) {
        if inner.height < 6 {
            return;
        }

        let subtitle_len = if state.subtitle.is_some() { 1 } else { 0 };
        let error_len = if state.validation_error.is_some() {
            1
        } else {
            0
        };
        let chunks = Layout::vertical([
            Constraint::Length(subtitle_len as u16),
            Constraint::Length(1), // search
            Constraint::Length(1), // separator
            Constraint::Min(3),    // list
            Constraint::Length(error_len as u16),
        ])
        .split(inner);

        let mut idx = 0usize;
        if let Some(subtitle) = &state.subtitle {
            Paragraph::new(subtitle.as_str())
                .style(Style::default().fg(text_muted()))
                .render(chunks[idx], buf);
            idx += 1;
        }

        self.render_search(chunks[idx], buf, state);
        idx += 1;

        Paragraph::new("─".repeat(chunks[idx].width as usize))
            .style(Style::default().fg(text_muted()))
            .render(chunks[idx], buf);
        idx += 1;

        self.render_list(chunks[idx], buf, state);
        idx += 1;

        if let Some(error) = &state.validation_error {
            Paragraph::new(format!("✗ {}", error))
                .style(Style::default().fg(accent_primary()))
                .render(chunks[idx], buf);
        }
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer, state: &MultiSelectDialogState) {
        let prompt = "> ";
        let input = state.list.search.value();
        if input.is_empty() {
            Paragraph::new(format!("{prompt}Type to search"))
                .style(Style::default().fg(text_muted()))
                .render(area, buf);
        } else {
            Paragraph::new(Line::from(vec![
                Span::styled(prompt, Style::default().fg(accent_primary())),
                Span::styled(input, Style::default().fg(text_primary())),
            ]))
            .render(area, buf);
        }
    }

    fn render_list(&self, area: Rect, buf: &mut Buffer, state: &MultiSelectDialogState) {
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_bg(dialog_bg());
            }
        }

        if state.list.filtered.is_empty() {
            Paragraph::new("No matching items")
                .style(Style::default().fg(text_muted()))
                .render(area, buf);
            return;
        }

        let visible_count = area.height as usize;
        let has_scrollbar = state.list.filtered.len() > visible_count;
        let content_width = if has_scrollbar {
            area.width.saturating_sub(1)
        } else {
            area.width
        };

        let selected_bg = ensure_contrast_bg(bg_highlight(), dialog_bg(), 2.0);
        let selected_fg = ensure_contrast_fg(text_primary(), selected_bg, 4.5);
        let selected_muted = ensure_contrast_fg(text_muted(), selected_bg, 3.0);
        let selected_accent = ensure_contrast_fg(accent_primary(), selected_bg, 3.0);

        for (i, &item_idx) in state
            .list
            .filtered
            .iter()
            .skip(state.list.scroll_offset)
            .take(visible_count)
            .enumerate()
        {
            let Some(item) = state.items.get(item_idx) else {
                continue;
            };
            let is_selected = state.list.scroll_offset + i == state.list.selected;
            let y = area.y + i as u16;
            let marker = if item.checked { "x" } else { " " };
            let prefix = if is_selected { "›" } else { " " };
            let title = if item.disabled {
                format!("{prefix} [{marker}] {} (not installed)", item.title)
            } else {
                format!("{prefix} [{marker}] {}", item.title)
            };
            let text = if item.description.is_empty() {
                title
            } else {
                format!("{title} — {}", item.description)
            };

            if is_selected {
                for x in area.x..area.x + content_width {
                    buf[(x, y)].set_bg(selected_bg);
                }
            }

            let style = if is_selected {
                if item.disabled {
                    Style::default().fg(selected_muted).bg(selected_bg)
                } else {
                    Style::default().fg(selected_fg).bg(selected_bg)
                }
            } else if item.disabled {
                Style::default().fg(text_muted())
            } else {
                Style::default().fg(text_primary())
            };
            let prefix_style = if is_selected {
                Style::default().fg(selected_accent).bg(selected_bg)
            } else {
                Style::default().fg(text_muted())
            };

            let line = if let Some(rest) = text.strip_prefix('›') {
                Line::from(vec![
                    Span::styled("›", prefix_style),
                    Span::styled(rest.to_string(), style),
                ])
            } else {
                Line::from(Span::styled(text, style))
            };

            Paragraph::new(line).render(
                Rect {
                    x: area.x,
                    y,
                    width: content_width,
                    height: 1,
                },
                buf,
            );
        }

        if has_scrollbar {
            render_minimal_scrollbar(
                Rect {
                    x: area.x + area.width - 1,
                    y: area.y,
                    width: 1,
                    height: area.height,
                },
                buf,
                state.list.filtered.len(),
                visible_count,
                state.list.scroll_offset,
            );
        }
    }

    pub fn list_area(area: Rect) -> Rect {
        let dialog = Self::dialog_area(area);
        let inner = dialog_content_area(dialog);
        Rect {
            x: inner.x,
            y: inner.y + 3,
            width: inner.width,
            height: inner.height.saturating_sub(4),
        }
    }
}

impl Default for MultiSelectDialog {
    fn default() -> Self {
        Self::new()
    }
}
