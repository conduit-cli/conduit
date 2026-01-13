//! Project picker dialog component with fuzzy search

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Paragraph, Widget},
};
use std::path::PathBuf;

use super::{
    dialog_bg, dialog_content_area, ensure_contrast_bg, ensure_contrast_fg,
    render_minimal_scrollbar, selected_bg, text_primary, DialogFrame, InstructionBar,
    ScrollbarMetrics, SearchableListState,
};

/// A project entry (directory with .git)
#[derive(Debug, Clone)]
pub struct ProjectEntry {
    /// Display name (folder name)
    pub name: String,
    /// Full path to the project
    pub path: PathBuf,
}

/// State for the project picker dialog
#[derive(Debug, Clone)]
pub struct ProjectPickerState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// All projects found in base directory
    pub projects: Vec<ProjectEntry>,
    /// Base directory being scanned
    pub base_dir: PathBuf,
    /// Searchable list state
    pub list: SearchableListState,
}

impl Default for ProjectPickerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectPickerState {
    pub fn new() -> Self {
        Self {
            visible: false,
            projects: Vec::new(),
            base_dir: PathBuf::new(),
            list: SearchableListState::new(10),
        }
    }

    /// Show the picker with projects from the given base directory
    pub fn show(&mut self, base_dir: PathBuf) {
        self.visible = true;
        self.base_dir = base_dir;
        self.list.reset();
        self.scan_directory();
        self.filter();
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn scrollbar_metrics(&self, area: Rect) -> Option<ScrollbarMetrics> {
        if !self.visible {
            return None;
        }

        let list_height = self.list.visible_len() as u16;
        let dialog_height = 9 + list_height;
        let dialog_width: u16 = 60;

        let dialog_width = dialog_width.min(area.width.saturating_sub(4));
        let dialog_height = dialog_height.min(area.height.saturating_sub(2));

        let dialog_x = area.width.saturating_sub(dialog_width) / 2;
        let dialog_y = area.height.saturating_sub(dialog_height) / 2;

        let dialog_area = Rect {
            x: dialog_x,
            y: dialog_y,
            width: dialog_width,
            height: dialog_height,
        };

        let inner = dialog_content_area(dialog_area);

        let list_y = inner.y + 3;
        let list_height_actual = inner.height.saturating_sub(5);
        if list_height_actual == 0 {
            return None;
        }

        let list_area = Rect {
            x: inner.x,
            y: list_y,
            width: inner.width,
            height: list_height_actual,
        };

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

    /// Scan the base directory for git projects
    pub fn scan_directory(&mut self) {
        self.projects.clear();

        let Ok(entries) = std::fs::read_dir(&self.base_dir) else {
            return;
        };

        let mut projects: Vec<ProjectEntry> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter(|e| e.path().join(".git").exists())
            .map(|e| ProjectEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path(),
            })
            .collect();

        // Sort by last modified time (most recent first)
        projects.sort_by(|a, b| {
            let time_a = std::fs::metadata(&a.path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let time_b = std::fs::metadata(&b.path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            time_b.cmp(&time_a) // Descending order (most recent first)
        });
        self.projects = projects;
    }

    /// Filter projects based on search string
    pub fn filter(&mut self) {
        let query = self.list.search.value().to_lowercase();
        let filtered = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if query.is_empty() {
                    true
                } else {
                    p.name.to_lowercase().contains(&query)
                }
            })
            .map(|(i, _)| i)
            .collect();
        self.list.set_filtered(filtered);
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

    /// Page up (move up by visible count)
    pub fn page_up(&mut self) {
        self.list.page_up();
    }

    /// Page down (move down by visible count)
    pub fn page_down(&mut self) {
        self.list.page_down();
    }

    /// Select item at a given visual row (for mouse clicks)
    /// Returns true if an item was selected
    pub fn select_at_row(&mut self, row: usize) -> bool {
        self.list.select_at_row(row)
    }

    /// Get the currently selected project
    pub fn selected_project(&self) -> Option<&ProjectEntry> {
        self.list
            .filtered
            .get(self.list.selected)
            .and_then(|&idx| self.projects.get(idx))
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check if there are no projects found
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }
}

/// Project picker dialog widget
pub struct ProjectPicker;

impl ProjectPicker {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &ProjectPickerState) {
        if !state.visible {
            return;
        }

        // Calculate dialog size
        let list_height = state.list.visible_len() as u16;
        let dialog_height = 9 + list_height; // header + search + separator + list + footer

        // Render dialog frame
        let frame = DialogFrame::new("Select Project", 60, dialog_height);
        let inner = frame.render(area, buf);

        // Layout inside dialog
        let chunks = Layout::vertical([
            Constraint::Length(1), // Search label
            Constraint::Length(1), // Search input
            Constraint::Length(1), // Separator
            Constraint::Min(1),    // Project list
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        // Render search with placeholder
        let search_display = if state.list.search.is_empty() {
            "Search: (type to filter)".to_string()
        } else {
            format!("Search: {}", state.list.search.value())
        };
        let search_style = if state.list.search.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search_label = Paragraph::new(search_display).style(search_style);
        search_label.render(chunks[0], buf);

        // Render cursor in search field
        if !state.list.search.is_empty() || state.list.search.cursor > 0 {
            let cursor_x = chunks[0].x + 8 + state.list.search.cursor as u16; // "Search: " is 8 chars
            if cursor_x < chunks[0].x + chunks[0].width {
                use ratatui::style::Modifier;
                buf[(cursor_x, chunks[0].y)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }

        // Render separator
        let separator = "─".repeat(inner.width as usize);
        let sep_paragraph = Paragraph::new(separator).style(Style::default().fg(Color::DarkGray));
        sep_paragraph.render(chunks[2], buf);

        // Render project list
        let list_area = chunks[3];
        if state.list.filtered.is_empty() {
            let empty_msg = if state.projects.is_empty() {
                "No git projects found in this directory"
            } else {
                "No projects match your search"
            };
            let empty = Paragraph::new(empty_msg)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            empty.render(list_area, buf);
        } else {
            // Compute selected colors once for the list
            let selected_bg_color = ensure_contrast_bg(selected_bg(), dialog_bg(), 2.0);
            let selected_fg_color = ensure_contrast_fg(text_primary(), selected_bg_color, 4.5);

            // Render visible items
            let visible_count = list_area.height as usize;
            for (i, &project_idx) in state
                .list
                .filtered
                .iter()
                .skip(state.list.scroll_offset)
                .take(visible_count)
                .enumerate()
            {
                let project = &state.projects[project_idx];
                let is_selected = state.list.scroll_offset + i == state.list.selected;

                let y = list_area.y + i as u16;
                if y >= list_area.y + list_area.height {
                    break;
                }

                // Format: "> name          path"
                let prefix = if is_selected { "> " } else { "  " };
                let name = &project.name;

                // Calculate path display (shortened)
                let path_str = project.path.to_string_lossy().replace(
                    dirs::home_dir()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                    "~",
                );

                let available_width = list_area.width as usize;
                let name_width = 20.min(available_width / 2);
                let path_width = available_width.saturating_sub(name_width + 4);

                let name_display = if name.len() > name_width {
                    format!("{}...", &name[..name_width - 3])
                } else {
                    format!("{:width$}", name, width = name_width)
                };

                let path_display = if path_str.len() > path_width {
                    format!("...{}", &path_str[path_str.len() - path_width + 3..])
                } else {
                    path_str.to_string()
                };

                let line_text = format!("{}{} {}", prefix, name_display, path_display);

                let style = if is_selected {
                    Style::default().fg(selected_fg_color).bg(selected_bg_color)
                } else {
                    Style::default().fg(text_primary())
                };

                // Render the line
                for (j, c) in line_text.chars().enumerate() {
                    if j < list_area.width as usize {
                        buf[(list_area.x + j as u16, y)]
                            .set_char(c)
                            .set_style(style);
                    }
                }
                // Fill rest of line with style for selected item
                if is_selected {
                    for j in line_text.len()..list_area.width as usize {
                        buf[(list_area.x + j as u16, y)].set_style(style);
                    }
                }
            }

            // Render scrollbar
            let total_filtered = state.list.filtered.len();
            render_minimal_scrollbar(
                Rect {
                    x: list_area.x + list_area.width - 1,
                    y: list_area.y,
                    width: 1,
                    height: list_area.height,
                },
                buf,
                total_filtered,
                visible_count,
                state.list.scroll_offset,
            );
        }

        // Render instructions
        let instructions = InstructionBar::new(vec![
            ("↑↓/^J^K", "Navigate"),
            ("^F/^B", "Page"),
            ("Enter", "Select"),
            ("Esc", "Cancel"),
        ]);
        instructions.render(chunks[5], buf);
    }
}

impl Default for ProjectPicker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_picker_delete_char() {
        let mut state = ProjectPickerState::new();

        // Type "abc"
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.list.search.input, "abc");

        // Backspace should delete 'c'
        state.delete_char();
        assert_eq!(state.list.search.input, "ab");

        // Backspace should delete 'b'
        state.delete_char();
        assert_eq!(state.list.search.input, "a");

        // Backspace should delete 'a'
        state.delete_char();
        assert_eq!(state.list.search.input, "");

        // Backspace on empty should do nothing
        state.delete_char();
        assert_eq!(state.list.search.input, "");
    }

    #[test]
    fn test_project_picker_cursor_position_after_delete() {
        let mut state = ProjectPickerState::new();

        // Type "abc"
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.list.search.cursor, 3);

        // Delete should move cursor back
        state.delete_char();
        assert_eq!(state.list.search.input, "ab");
        assert_eq!(state.list.search.cursor, 2);

        state.delete_char();
        assert_eq!(state.list.search.input, "a");
        assert_eq!(state.list.search.cursor, 1);

        state.delete_char();
        assert_eq!(state.list.search.input, "");
        assert_eq!(state.list.search.cursor, 0);

        // Delete on empty should keep cursor at 0
        state.delete_char();
        assert_eq!(state.list.search.cursor, 0);
    }

    #[test]
    fn test_project_picker_mid_string_deletion() {
        let mut state = ProjectPickerState::new();

        // Type "abc"
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.list.search.input, "abc");
        assert_eq!(state.list.search.cursor, 3);

        // Move cursor to middle (after 'a')
        state.move_cursor_start();
        assert_eq!(state.list.search.cursor, 0);

        state.move_cursor_right();
        assert_eq!(state.list.search.cursor, 1);

        // Delete should remove 'a' (char before cursor), leaving "bc"
        state.delete_char();
        assert_eq!(state.list.search.input, "bc");
        assert_eq!(state.list.search.cursor, 0);
    }

    #[test]
    fn test_project_picker_ascii_only() {
        // Note: The TextInputState uses byte-based cursor positioning, which means
        // multi-byte UTF-8 characters (like 'é' or emoji) are not fully supported.
        // This test verifies ASCII characters work correctly.
        let mut state = ProjectPickerState::new();

        // Type ASCII characters
        state.insert_char('h');
        state.insert_char('e');
        state.insert_char('l');
        state.insert_char('l');
        state.insert_char('o');
        assert_eq!(state.list.search.input, "hello");
        assert_eq!(state.list.search.cursor, 5);

        // Delete 'o'
        state.delete_char();
        assert_eq!(state.list.search.input, "hell");
        assert_eq!(state.list.search.cursor, 4);

        // Delete back to 'e'
        state.delete_char();
        state.delete_char();
        assert_eq!(state.list.search.input, "he");
        assert_eq!(state.list.search.cursor, 2);

        state.delete_char();
        assert_eq!(state.list.search.input, "h");
        assert_eq!(state.list.search.cursor, 1);
    }
}
