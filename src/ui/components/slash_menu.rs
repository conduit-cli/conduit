//! Slash and skill menu component.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::command_resolver::{MenuEntry, MenuEntryKind};

use super::{
    accent_primary, bg_highlight, dialog_bg, ensure_contrast_bg, ensure_contrast_fg,
    render_minimal_scrollbar, text_muted, text_primary, truncate_to_width, SearchableListState,
};

#[derive(Debug, Clone)]
pub struct SlashMenuEntry {
    pub label: String,
    pub description: String,
    pub source_badge: String,
    pub kind: MenuEntryKind,
}

impl SlashMenuEntry {
    fn from_resolved(entry: MenuEntry) -> Self {
        Self {
            label: entry.label,
            description: entry.description,
            source_badge: entry.source_badge,
            kind: entry.kind,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlashMenuState {
    pub visible: bool,
    pub prompt_prefix: char,
    pub commands: Vec<SlashMenuEntry>,
    pub list: SearchableListState,
}

impl SlashMenuState {
    pub fn new() -> Self {
        Self {
            visible: false,
            prompt_prefix: '/',
            commands: Vec::new(),
            list: SearchableListState::new(8),
        }
    }

    pub fn show_with_entries(&mut self, prompt_prefix: char, entries: Vec<MenuEntry>) {
        self.visible = true;
        self.prompt_prefix = prompt_prefix;
        self.commands = entries
            .into_iter()
            .filter(|entry| entry.trigger == prompt_prefix)
            .map(SlashMenuEntry::from_resolved)
            .collect();
        self.list.reset();
        self.list.filtered = (0..self.commands.len()).collect();
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn filtered_len(&self) -> usize {
        self.list.filtered.len()
    }

    pub fn set_max_visible(&mut self, max_visible: usize) {
        let max_visible = max_visible.max(1);
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

    pub fn select_next(&mut self) {
        self.list.select_next();
    }

    pub fn select_prev(&mut self) {
        self.list.select_prev();
    }

    pub fn selected_entry(&self) -> Option<&SlashMenuEntry> {
        if self.list.filtered.is_empty() {
            return None;
        }
        let idx = self.list.filtered.get(self.list.selected)?;
        self.commands.get(*idx)
    }

    fn filter(&mut self) {
        let query = self.list.search.value().to_lowercase();
        let filtered: Vec<usize> = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| {
                if query.is_empty() {
                    return true;
                }
                cmd.label.to_lowercase().contains(&query)
                    || cmd.description.to_lowercase().contains(&query)
                    || cmd.source_badge.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        self.list.set_filtered(filtered);
    }
}

impl Default for SlashMenuState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SlashMenu;

impl SlashMenu {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &SlashMenuState) {
        if !state.visible {
            return;
        }

        if area.height < 5 || area.width < 10 {
            return;
        }

        Clear.render(area, buf);
        buf.set_style(area, Style::default().bg(dialog_bg()));

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(accent_primary()).bg(dialog_bg()))
            .style(Style::default().bg(dialog_bg()));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 3 || inner.width == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);

        self.render_search(chunks[0], buf, state);
        self.render_separator(chunks[1], buf);
        self.render_list(chunks[2], buf, state);
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer, state: &SlashMenuState) {
        let prompt = state.prompt_prefix.to_string();
        let input = state.list.search.value();

        if input.is_empty() {
            Paragraph::new(format!("{} Type a command...", state.prompt_prefix))
                .style(Style::default().fg(text_muted()))
                .render(area, buf);
        } else {
            let line = Line::from(vec![
                Span::styled(prompt.as_str(), Style::default().fg(accent_primary())),
                Span::styled(input, Style::default().fg(text_primary())),
            ]);
            Paragraph::new(line).render(area, buf);
        }

        let prompt_width = UnicodeWidthStr::width(prompt.as_str()) as u16;
        let cursor_offset = input
            .chars()
            .take(state.list.search.cursor)
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(1) as u16)
            .sum::<u16>();
        let cursor_x = area.x + prompt_width + cursor_offset;
        if cursor_x < area.x + area.width {
            buf[(cursor_x, area.y)]
                .set_style(Style::default().add_modifier(ratatui::style::Modifier::REVERSED));
        }
    }

    fn render_separator(&self, area: Rect, buf: &mut Buffer) {
        let separator = "\u{2500}".repeat(area.width as usize);
        Paragraph::new(separator)
            .style(Style::default().fg(text_muted()))
            .render(area, buf);
    }

    fn render_list(&self, area: Rect, buf: &mut Buffer, state: &SlashMenuState) {
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_bg(dialog_bg());
            }
        }

        if state.list.filtered.is_empty() {
            let msg = if state.commands.is_empty() {
                "No commands available"
            } else {
                "No matching commands"
            };
            Paragraph::new(msg)
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

        for (i, &cmd_idx) in state
            .list
            .filtered
            .iter()
            .skip(state.list.scroll_offset)
            .take(visible_count)
            .enumerate()
        {
            let entry = &state.commands[cmd_idx];
            let is_selected = state.list.scroll_offset + i == state.list.selected;
            let y = area.y + i as u16;
            let prefix = if is_selected { "> " } else { "  " };
            let prefix_width = UnicodeWidthStr::width(prefix);
            let available_label_width = (content_width as usize).saturating_sub(prefix_width + 1);
            let label = truncate_to_width(entry.label.as_str(), available_label_width);
            let mut description = entry.description.clone();
            if !entry.source_badge.is_empty() {
                description.push_str(" [");
                description.push_str(&entry.source_badge);
                description.push(']');
            }
            let desc_width = (content_width as usize)
                .saturating_sub(prefix_width + UnicodeWidthStr::width(label.as_str()) + 3);
            let desc_display = truncate_to_width(description.as_str(), desc_width);

            let (prefix_style, label_style, desc_style) = if is_selected {
                (
                    Style::default().fg(selected_muted).bg(selected_bg),
                    Style::default().fg(selected_fg).bg(selected_bg),
                    Style::default().fg(selected_muted).bg(selected_bg),
                )
            } else {
                (
                    Style::default().fg(text_muted()),
                    Style::default().fg(text_primary()),
                    Style::default().fg(text_muted()),
                )
            };

            if is_selected {
                for x in area.x..area.x + content_width {
                    buf[(x, y)].set_bg(selected_bg);
                }
            }

            let mut spans = vec![
                Span::styled(prefix, prefix_style),
                Span::styled(label, label_style),
            ];
            if !desc_display.is_empty() {
                spans.push(Span::styled(" - ", desc_style));
                spans.push(Span::styled(desc_display, desc_style));
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
}

impl Default for SlashMenu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_resolver::{
        ConduitCommand, MenuEntryKind, ProviderArtifactSource, ProviderInvocation,
    };
    use std::path::PathBuf;

    fn sample_entries() -> Vec<MenuEntry> {
        vec![
            MenuEntry {
                label: "/model".to_string(),
                description: "Select model".to_string(),
                source_badge: "Conduit".to_string(),
                trigger: '/',
                kind: MenuEntryKind::ConduitCommand(ConduitCommand::Model),
            },
            MenuEntry {
                label: "$ship".to_string(),
                description: "Ship the project".to_string(),
                source_badge: "Codex skill".to_string(),
                trigger: '$',
                kind: MenuEntryKind::ProviderInvocation(ProviderInvocation::Skill {
                    source: ProviderArtifactSource::Codex,
                    name: "ship".to_string(),
                    description: "Ship the project".to_string(),
                    path: PathBuf::from("/tmp/ship/SKILL.md"),
                }),
            },
        ]
    }

    #[test]
    fn filters_by_trigger() {
        let mut state = SlashMenuState::new();
        state.show_with_entries('$', sample_entries());
        assert_eq!(state.commands.len(), 1);
        assert_eq!(state.commands[0].label, "$ship");
    }

    #[test]
    fn supports_searching_source_badge() {
        let mut state = SlashMenuState::new();
        state.show_with_entries('$', sample_entries());
        state.insert_char('c');
        state.insert_char('o');
        state.insert_char('d');
        assert_eq!(state.filtered_len(), 1);
    }
}
