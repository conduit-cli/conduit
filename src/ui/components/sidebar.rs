//! Sidebar component for repository/workspace navigation

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph, StatefulWidget, Widget},
};

use crate::ui::components::{
    accent_primary, border_default, ensure_contrast_fg, selected_bg, selected_bg_dim, sidebar_bg,
    text_muted, text_primary,
};

use super::tree_view::{SidebarData, TreeView, TreeViewState};

/// Sidebar widget for workspace navigation
pub struct Sidebar<'a> {
    /// Tree data
    data: &'a SidebarData,
    /// Whether the sidebar is visible
    visible: bool,
    /// Width of the sidebar
    width: u16,
    /// Title
    title: &'a str,
}

impl<'a> Sidebar<'a> {
    pub fn new(data: &'a SidebarData) -> Self {
        Self {
            data,
            visible: true,
            width: 30,
            title: "⬒ Workspaces",
        }
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// Check if the sidebar is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Get the width of the sidebar (0 if hidden)
    pub fn effective_width(&self) -> u16 {
        if self.visible {
            self.width
        } else {
            0
        }
    }
}

/// State for the sidebar
#[derive(Debug, Default)]
pub struct SidebarState {
    /// Tree view state
    pub tree_state: TreeViewState,
    /// Whether the sidebar is visible
    pub visible: bool,
    /// Whether the sidebar is focused
    pub focused: bool,
    /// Area of the "Add Project" button (when sidebar is empty)
    pub add_project_button_area: Option<Rect>,
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            tree_state: TreeViewState::new(),
            visible: false, // Hidden by default
            focused: false,
            add_project_button_area: None,
        }
    }

    /// Toggle sidebar visibility
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Show sidebar and focus it
    pub fn show(&mut self) {
        self.visible = true;
        self.focused = true;
    }

    /// Hide sidebar
    pub fn hide(&mut self) {
        self.visible = false;
        self.focused = false;
    }

    /// Set focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

impl StatefulWidget for Sidebar<'_> {
    type State = SidebarState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if !state.visible || area.width < 10 {
            return;
        }

        // // Determine border color based on focus
        // let border_style = if state.focused {
        //     Style::default().fg(ACCENT_PRIMARY)
        // } else {
        //     Style::default().fg(BORDER_DEFAULT)
        // };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title area
                Constraint::Length(1), // Separator
                Constraint::Min(1),    // Tree content
            ])
            .split(area);

        let title = Paragraph::new(format!(" {} ", self.title.trim()))
            .style(Style::default().fg(text_primary()).bg(sidebar_bg()));

        let title_area = chunks[0];

        for y in title_area.y..title_area.y + title_area.height {
            for x in title_area.x..title_area.x + title_area.width {
                buf[(x, y)].set_bg(sidebar_bg());
            }
        }

        let middle_row = Rect::new(title_area.x, title_area.y + 1, title_area.width, 1);
        title.render(middle_row, buf);

        // Draw horizontal separator line
        let separator_y = chunks[1].y; // Last row of title area
                                       // Or: let separator_y = chunks[1].y; // First row of content area

        for x in area.x..area.x + area.width {
            buf[(x, separator_y)]
                .set_char('─')
                .set_fg(border_default())
                .set_bg(sidebar_bg());
        }

        let content_area = chunks[2];

        // Fill content area background
        for y in content_area.y..content_area.y + content_area.height {
            for x in content_area.x..content_area.x + content_area.width {
                buf[(x, y)].set_bg(sidebar_bg());
            }
        }

        // Check if sidebar is empty
        if self.data.nodes.is_empty() {
            // Render centered "Add Project" button
            let button_text = "+ Add Project";
            let button_width = button_text.len() as u16 + 4; // padding on each side

            // Center horizontally
            let button_x = content_area
                .x
                .saturating_add((content_area.width.saturating_sub(button_width)) / 2);

            // Center vertically
            let button_y = content_area
                .y
                .saturating_add(content_area.height.saturating_sub(1) / 2);

            // Store button area for click detection
            let button_area = Rect::new(button_x, button_y, button_width, 1);
            state.add_project_button_area = Some(button_area);

            // Render button with styling
            let button_style = if state.focused {
                Style::default().fg(accent_primary())
            } else {
                Style::default().fg(text_muted())
            };

            let button = Paragraph::new(format!("  {}  ", button_text)).style(button_style);
            button.render(button_area, buf);
        } else {
            // Clear button area when not empty
            state.add_project_button_area = None;

            let sidebar_background = sidebar_bg();
            let label_fg = ensure_contrast_fg(text_primary(), sidebar_background, 4.5);
            let suffix_fg = ensure_contrast_fg(text_muted(), sidebar_background, 3.0);

            let focused_bg = selected_bg();
            let focused_fg = ensure_contrast_fg(text_primary(), focused_bg, 4.5);
            let unfocused_bg = selected_bg_dim();
            let unfocused_fg = ensure_contrast_fg(text_primary(), unfocused_bg, 4.5);

            let block = Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(sidebar_bg()));

            // Create and render tree view
            let selected_style = if state.focused {
                Style::default().bg(focused_bg).fg(focused_fg)
            } else {
                Style::default().bg(unfocused_bg).fg(unfocused_fg)
            };
            let tree = TreeView::new(&self.data.nodes)
                .block(block)
                .style(Style::default().fg(label_fg))
                .suffix_style(Style::default().fg(suffix_fg))
                .selected_style(selected_style);

            StatefulWidget::render(tree, content_area, buf, &mut state.tree_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_sidebar_data() {
        let mut data = SidebarData::new();

        data.add_repository(
            Uuid::new_v4(),
            "my-project",
            vec![
                (Uuid::new_v4(), "main".to_string(), "main".to_string()),
                (
                    Uuid::new_v4(),
                    "feature-x".to_string(),
                    "feature/x".to_string(),
                ),
            ],
        );

        assert_eq!(data.nodes.len(), 1);
        // 3 children: 1 action node + 2 workspace nodes
        assert_eq!(data.nodes[0].children.len(), 3);

        // Nodes are expanded by default, so all 4 are visible (1 repo + 1 action + 2 workspaces)
        let visible = data.visible_nodes();
        assert_eq!(visible.len(), 4);

        // Toggle to collapse
        data.toggle_at(0);

        // Now only repository is visible
        let visible = data.visible_nodes();
        assert_eq!(visible.len(), 1);
    }
}
