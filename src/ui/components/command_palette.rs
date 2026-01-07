//! Command palette component for quick action lookup and execution

use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{
    render_minimal_scrollbar, DialogFrame, InstructionBar, SearchableListState, ACCENT_PRIMARY,
    BG_HIGHLIGHT, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::config::keys::KeybindingConfig;
use crate::ui::action::Action;

/// A command entry in the palette
#[derive(Debug, Clone)]
pub struct CommandPaletteEntry {
    /// The action to execute
    pub action: Action,
    /// Display description (e.g., "New project...")
    pub description: String,
    /// Formatted keybinding string (e.g., "C-n")
    pub keybinding: Option<String>,
}

/// State for the command palette dialog
#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// All available commands
    pub commands: Vec<CommandPaletteEntry>,
    /// Searchable list state (reuse existing component)
    pub list: SearchableListState,
}

impl CommandPaletteState {
    pub fn new() -> Self {
        Self {
            visible: false,
            commands: Vec::new(),
            list: SearchableListState::new(12), // Show up to 12 items
        }
    }

    /// Show the command palette and populate commands from keybindings
    pub fn show(&mut self, keybindings: &KeybindingConfig) {
        self.visible = true;
        self.commands = Self::build_commands(keybindings);
        self.list.reset();
        // Initialize filtered list with all commands
        self.list.filtered = (0..self.commands.len()).collect();
    }

    /// Hide the command palette
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Check if visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Build command list with keybinding lookup
    fn build_commands(keybindings: &KeybindingConfig) -> Vec<CommandPaletteEntry> {
        // Build reverse lookup: Action discriminant -> key display string
        let mut keybinding_cache: HashMap<String, String> = HashMap::new();

        for (combo, action) in &keybindings.global {
            let key = Self::action_discriminant_key(action);
            let display = combo.to_string();
            // Prefer shorter keybindings when multiple exist
            keybinding_cache
                .entry(key)
                .and_modify(|existing| {
                    if display.len() < existing.len() {
                        *existing = display.clone();
                    }
                })
                .or_insert(display);
        }

        // Collect all actions that should appear in palette
        let palette_actions: Vec<Action> = vec![
            Action::Quit,
            Action::ToggleSidebar,
            Action::NewProject,
            Action::OpenPr,
            Action::InterruptAgent,
            Action::ToggleViewMode,
            Action::ShowModelSelector,
            Action::ToggleMetrics,
            Action::DumpDebugState,
            Action::CloseTab,
            Action::NextTab,
            Action::PrevTab,
            Action::ScrollPageUp,
            Action::ScrollPageDown,
            Action::ScrollToTop,
            Action::ScrollToBottom,
            Action::EnterSidebarMode,
            Action::AddRepository,
            Action::OpenSettings,
            Action::ArchiveOrRemove,
            Action::ToggleAgentMode,
            Action::OpenSessionImport,
            Action::ShowHelp,
        ];

        let mut entries: Vec<CommandPaletteEntry> = palette_actions
            .into_iter()
            .filter(|a| a.show_in_palette())
            .map(|action| {
                let key = Self::action_discriminant_key(&action);
                let keybinding = keybinding_cache.get(&key).cloned();
                CommandPaletteEntry {
                    description: action.palette_description(),
                    action,
                    keybinding,
                }
            })
            .collect();

        // Sort alphabetically by description
        entries.sort_by(|a, b| {
            a.description
                .to_lowercase()
                .cmp(&b.description.to_lowercase())
        });

        entries
    }

    /// Generate a string key for an action (handles parameterized variants)
    fn action_discriminant_key(action: &Action) -> String {
        // Use debug format but strip parameters for matching
        // e.g., "ScrollUp(5)" -> "ScrollUp"
        let debug = format!("{:?}", action);
        debug.split('(').next().unwrap_or(&debug).to_string()
    }

    /// Filter commands based on search query
    pub fn filter(&mut self) {
        let query = self.list.search.value().to_lowercase();
        let filtered: Vec<usize> = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| {
                if query.is_empty() {
                    true
                } else {
                    cmd.description.to_lowercase().contains(&query)
                }
            })
            .map(|(i, _)| i)
            .collect();
        self.list.set_filtered(filtered);
    }

    /// Insert a character into the search field
    pub fn insert_char(&mut self, c: char) {
        self.list.search.insert_char(c);
        self.filter();
    }

    /// Delete character before cursor
    pub fn delete_char(&mut self) {
        self.list.search.delete_char();
        self.filter();
    }

    /// Select next item
    pub fn select_next(&mut self) {
        self.list.select_next();
    }

    /// Select previous item
    pub fn select_prev(&mut self) {
        self.list.select_prev();
    }

    /// Get the currently selected entry
    pub fn selected_entry(&self) -> Option<&CommandPaletteEntry> {
        if self.list.filtered.is_empty() {
            return None;
        }
        let idx = self.list.filtered.get(self.list.selected)?;
        self.commands.get(*idx)
    }
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self::new()
    }
}

/// Command palette widget for rendering
pub struct CommandPalette;

impl CommandPalette {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &CommandPaletteState) {
        if !state.visible {
            return;
        }

        // Calculate dialog dimensions
        let dialog_width = 60u16.min(area.width.saturating_sub(4));
        let list_height = state.list.filtered.len().min(12) as u16;
        let dialog_height = 5 + list_height.max(1); // search + separator + list + separator + instructions

        // Use DialogFrame for consistent styling
        let frame = DialogFrame::new("Command Palette", dialog_width, dialog_height);
        let inner = frame.render(area, buf);

        // Layout: search input, separator, list, instructions
        let chunks = Layout::vertical([
            Constraint::Length(1), // Search input with ">" prompt
            Constraint::Length(1), // Separator
            Constraint::Min(1),    // Command list
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        // Render search with ">" prompt
        self.render_search(chunks[0], buf, state);

        // Separator
        self.render_separator(chunks[1], buf);

        // Command list
        self.render_list(chunks[2], buf, state);

        // Instructions
        let instructions = InstructionBar::new(vec![
            ("\u{2191}\u{2193}", "Navigate"),
            ("Enter", "Execute"),
            ("Esc", "Cancel"),
        ]);
        instructions.render(chunks[3], buf);
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer, state: &CommandPaletteState) {
        let prompt = "> ";
        let input = state.list.search.value();

        if input.is_empty() {
            // Show placeholder
            let placeholder = format!("{}Type to search commands...", prompt);
            let para = Paragraph::new(placeholder).style(Style::default().fg(TEXT_MUTED));
            para.render(area, buf);
        } else {
            // Show prompt and input
            let line = Line::from(vec![
                Span::styled(prompt, Style::default().fg(ACCENT_PRIMARY)),
                Span::styled(input, Style::default().fg(TEXT_PRIMARY)),
            ]);
            let para = Paragraph::new(line);
            para.render(area, buf);
        }

        // Render cursor
        let cursor_x = area.x + prompt.len() as u16 + state.list.search.cursor as u16;
        if cursor_x < area.x + area.width {
            buf[(cursor_x, area.y)]
                .set_style(Style::default().add_modifier(ratatui::style::Modifier::REVERSED));
        }
    }

    fn render_separator(&self, area: Rect, buf: &mut Buffer) {
        let separator = "\u{2500}".repeat(area.width as usize);
        let para = Paragraph::new(separator).style(Style::default().fg(TEXT_MUTED));
        para.render(area, buf);
    }

    fn render_list(&self, area: Rect, buf: &mut Buffer, state: &CommandPaletteState) {
        if state.list.filtered.is_empty() {
            // Show empty message
            let msg = if state.commands.is_empty() {
                "No commands available"
            } else {
                "No matching commands"
            };
            let para = Paragraph::new(msg).style(Style::default().fg(TEXT_MUTED));
            para.render(area, buf);
            return;
        }

        let visible_count = area.height as usize;
        let has_scrollbar = state.list.filtered.len() > visible_count;
        // Reserve space for scrollbar if needed
        let content_width = if has_scrollbar {
            area.width.saturating_sub(1)
        } else {
            area.width
        };

        for (i, &cmd_idx) in state
            .list
            .filtered
            .iter()
            .skip(state.list.scroll_offset)
            .take(visible_count)
            .enumerate()
        {
            let cmd = &state.commands[cmd_idx];
            let is_selected = state.list.scroll_offset + i == state.list.selected;
            let y = area.y + i as u16;

            // Calculate available width (use content_width to account for scrollbar)
            let key_str = cmd.keybinding.as_deref().unwrap_or("");
            let key_width = key_str.len();
            let prefix_width = 2; // "> " or "  "
            let gap = 2; // Gap between description and keybinding
            let trailing_gap = 2; // Gap after keybinding before scrollbar
            let available_desc_width = (content_width as usize)
                .saturating_sub(prefix_width + key_width + gap + trailing_gap);

            // Truncate description if needed
            let desc = if cmd.description.len() > available_desc_width {
                format!(
                    "{}...",
                    &cmd.description[..available_desc_width.saturating_sub(3)]
                )
            } else {
                cmd.description.clone()
            };

            // Build the line with proper alignment
            let prefix = if is_selected { "> " } else { "  " };

            // Calculate padding between description and keybinding (use content_width, reserve trailing_gap)
            let padding_width = (content_width as usize)
                .saturating_sub(prefix_width + desc.len() + key_width + trailing_gap);
            let padding = " ".repeat(padding_width);
            let trailing = " ".repeat(trailing_gap);

            // Apply styling
            let (prefix_style, desc_style, key_style, bg) = if is_selected {
                (
                    Style::default().fg(ACCENT_PRIMARY).bg(BG_HIGHLIGHT),
                    Style::default().fg(Color::White).bg(BG_HIGHLIGHT),
                    Style::default().fg(TEXT_MUTED).bg(BG_HIGHLIGHT),
                    BG_HIGHLIGHT,
                )
            } else {
                (
                    Style::default().fg(TEXT_MUTED),
                    Style::default().fg(TEXT_PRIMARY),
                    Style::default().fg(TEXT_MUTED),
                    Color::Reset,
                )
            };

            // Build spans
            let line = Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(&desc, desc_style),
                Span::styled(&padding, Style::default().bg(bg)),
                Span::styled(key_str, key_style),
                Span::styled(&trailing, Style::default().bg(bg)),
            ]);

            // Render to buffer (use content_width to leave room for scrollbar)
            let line_area = Rect {
                x: area.x,
                y,
                width: content_width,
                height: 1,
            };

            // Fill background for selected line (only up to content_width)
            if is_selected {
                for x in area.x..area.x + content_width {
                    buf[(x, y)].set_bg(BG_HIGHLIGHT);
                }
            }

            Paragraph::new(line).render(line_area, buf);
        }

        // Render scrollbar if needed
        if has_scrollbar {
            let scrollbar_area = Rect {
                x: area.x + area.width - 1,
                y: area.y,
                width: 1,
                height: area.height,
            };
            render_minimal_scrollbar(
                scrollbar_area,
                buf,
                state.list.filtered.len(),
                visible_count,
                state.list.scroll_offset,
            );
        }
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}
