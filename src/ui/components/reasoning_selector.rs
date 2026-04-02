//! Reasoning effort selector dialog.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    accent_primary, bg_highlight, dialog_bg, ensure_contrast_bg, ensure_contrast_fg,
    render_minimal_scrollbar, text_muted, text_primary, text_secondary, DialogFrame,
    SearchableListState,
};
use crate::agent::{AgentType, ReasoningEffort};

const DIALOG_WIDTH: u16 = 58;
const DIALOG_HEIGHT: u16 = 14;

#[derive(Debug, Clone)]
pub struct ReasoningOption {
    pub effort: Option<ReasoningEffort>,
    pub label: &'static str,
    pub description: &'static str,
}

impl ReasoningOption {
    fn auto() -> Self {
        Self {
            effort: None,
            label: "Auto",
            description: "Provider default reasoning behavior",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReasoningSelectorState {
    pub visible: bool,
    pub agent_type: Option<AgentType>,
    pub options: Vec<ReasoningOption>,
    pub list: SearchableListState,
}

impl ReasoningSelectorState {
    pub fn new() -> Self {
        Self {
            visible: false,
            agent_type: None,
            options: Vec::new(),
            list: SearchableListState::new(8),
        }
    }

    pub fn show(&mut self, agent_type: AgentType, current: Option<ReasoningEffort>) {
        self.visible = true;
        self.agent_type = Some(agent_type);
        self.options = Self::build_options(agent_type);
        self.list.reset();
        self.list.filtered = (0..self.options.len()).collect();
        self.select_current(current);
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn selected_option(&self) -> Option<&ReasoningOption> {
        let idx = *self.list.filtered.get(self.list.selected)?;
        self.options.get(idx)
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

    pub fn set_max_visible(&mut self, max_visible: usize) {
        self.list.max_visible = max_visible.max(1);
    }

    pub fn insert_char(&mut self, c: char) {
        self.list.search.insert_char(c);
        self.filter();
    }

    pub fn insert_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch.is_control() {
                continue;
            }
            self.list.search.insert_char(ch);
        }
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

    fn select_current(&mut self, current: Option<ReasoningEffort>) {
        if let Some((idx, _)) = self
            .options
            .iter()
            .enumerate()
            .find(|(_, option)| option.effort == current)
        {
            if let Some(filtered_idx) = self.list.filtered.iter().position(|v| *v == idx) {
                self.list.selected = filtered_idx;
                self.list.scroll_offset = filtered_idx.saturating_sub(2);
            }
        }
    }

    fn filter(&mut self) {
        let query = self.list.search.value().trim().to_lowercase();
        let filtered = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, option)| {
                query.is_empty()
                    || option.label.to_lowercase().contains(&query)
                    || option.description.to_lowercase().contains(&query)
            })
            .map(|(idx, _)| idx)
            .collect();
        self.list.set_filtered(filtered);
    }

    fn build_options(agent_type: AgentType) -> Vec<ReasoningOption> {
        let mut options = vec![ReasoningOption::auto()];
        match agent_type {
            AgentType::Claude => {
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::Low),
                    label: "Low",
                    description: "Faster and cheaper",
                });
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::Medium),
                    label: "Medium",
                    description: "Balanced quality and speed",
                });
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::High),
                    label: "High",
                    description: "Best quality, slower and costlier",
                });
            }
            AgentType::Codex => {
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::Minimal),
                    label: "Minimal",
                    description: "Smallest reasoning budget",
                });
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::Low),
                    label: "Low",
                    description: "Lower reasoning budget",
                });
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::Medium),
                    label: "Medium",
                    description: "Balanced reasoning budget",
                });
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::High),
                    label: "High",
                    description: "High reasoning budget",
                });
                options.push(ReasoningOption {
                    effort: Some(ReasoningEffort::XHigh),
                    label: "XHigh",
                    description: "Maximum reasoning budget",
                });
            }
            AgentType::Gemini | AgentType::Opencode | AgentType::Pi => {}
        }
        options
    }
}

impl Default for ReasoningSelectorState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ReasoningSelector;

impl ReasoningSelector {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &ReasoningSelectorState) {
        if !state.visible {
            return;
        }

        let frame = DialogFrame::new(" Reasoning Effort ", DIALOG_WIDTH, DIALOG_HEIGHT)
            .instructions(vec![("Enter", "apply"), ("Esc", "cancel")]);
        let inner = frame.render(area, buf);

        if inner.height < 4 || inner.width < 10 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

        self.render_search(chunks[0], buf, state);
        self.render_separator(chunks[1], buf);
        self.render_list(chunks[2], buf, state);
        self.render_hint(chunks[3], buf, state);
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer, state: &ReasoningSelectorState) {
        if state.list.search.value().is_empty() {
            Paragraph::new("Filter options...")
                .style(Style::default().fg(text_muted()))
                .render(area, buf);
        } else {
            Paragraph::new(state.list.search.value())
                .style(Style::default().fg(text_primary()))
                .render(area, buf);
        }

        let cursor_offset = state
            .list
            .search
            .value()
            .chars()
            .take(state.list.search.cursor)
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(1) as u16)
            .sum::<u16>();
        let cursor_x = area.x + cursor_offset;
        if cursor_x < area.x + area.width {
            buf[(cursor_x, area.y)].set_style(
                Style::default()
                    .fg(text_primary())
                    .bg(bg_highlight())
                    .add_modifier(Modifier::REVERSED),
            );
        }
    }

    fn render_separator(&self, area: Rect, buf: &mut Buffer) {
        let separator = "─".repeat(area.width as usize);
        Paragraph::new(separator)
            .style(Style::default().fg(text_muted()))
            .render(area, buf);
    }

    fn render_list(&self, area: Rect, buf: &mut Buffer, state: &ReasoningSelectorState) {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                buf[(x, y)].set_bg(dialog_bg());
            }
        }

        if state.list.filtered.is_empty() {
            Paragraph::new("No matching options")
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
        let selected_muted = ensure_contrast_fg(text_secondary(), selected_bg, 3.0);

        let start = state.list.scroll_offset;
        let end = (start + visible_count).min(state.list.filtered.len());
        for (row, filtered_idx) in (start..end).enumerate() {
            let Some(option_idx) = state.list.filtered.get(filtered_idx) else {
                continue;
            };
            let Some(option) = state.options.get(*option_idx) else {
                continue;
            };
            let selected = filtered_idx == state.list.selected;
            let y = area.y + row as u16;
            let bg = if selected { selected_bg } else { dialog_bg() };
            let primary = if selected {
                selected_fg
            } else {
                text_primary()
            };
            let secondary = if selected {
                selected_muted
            } else {
                text_secondary()
            };

            let mut spans = vec![
                Span::styled(
                    format!("{:>2}. {}", filtered_idx + 1, option.label),
                    Style::default().fg(primary).bg(bg),
                ),
                Span::styled("  ", Style::default().bg(bg)),
                Span::styled(option.description, Style::default().fg(secondary).bg(bg)),
            ];

            let width_used: usize = spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            if width_used < content_width as usize {
                spans.push(Span::styled(
                    " ".repeat(content_width as usize - width_used),
                    Style::default().bg(bg),
                ));
            }

            Paragraph::new(Line::from(spans)).render(
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
                area,
                buf,
                state.list.filtered.len(),
                visible_count,
                state.list.scroll_offset,
            );
        }
    }

    fn render_hint(&self, area: Rect, buf: &mut Buffer, state: &ReasoningSelectorState) {
        let hint = match state.agent_type {
            Some(AgentType::Claude) => "Claude supports: auto, low, medium, high",
            Some(AgentType::Codex) => "Codex supports: auto, minimal, low, medium, high, xhigh",
            Some(AgentType::Gemini) | Some(AgentType::Opencode) | Some(AgentType::Pi) | None => {
                "Reasoning effort is not available for this agent"
            }
        };
        Paragraph::new(hint)
            .style(Style::default().fg(accent_primary()))
            .render(area, buf);
    }
}

impl Default for ReasoningSelector {
    fn default() -> Self {
        Self::new()
    }
}
