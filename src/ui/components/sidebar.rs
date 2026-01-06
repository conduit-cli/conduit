//! Sidebar component for repository/workspace navigation

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, StatefulWidget, Widget},
};

use crate::ui::components::{BG_BASE, BORDER_DEFAULT, TEXT_PRIMARY};

use super::tree_view::{SidebarData, TreeView, TreeViewState};
use super::{SELECTED_BG, SELECTED_BG_DIM};

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
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            tree_state: TreeViewState::new(),
            visible: false, // Hidden by default
            focused: false,
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
            .style(Style::default().fg(TEXT_PRIMARY).bg(BG_BASE));

        let title_area = chunks[0];

        for y in title_area.y..title_area.y + title_area.height {
            for x in title_area.x..title_area.x + title_area.width {
                buf[(x, y)].set_bg(BG_BASE);
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
                .set_fg(BORDER_DEFAULT)
                .set_bg(BG_BASE);
        }

        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(BG_BASE));

        // Create and render tree view
        let tree = TreeView::new(&self.data.nodes).block(block).selected_style(
            Style::default()
                .bg(if state.focused {
                    SELECTED_BG
                } else {
                    SELECTED_BG_DIM
                })
                .fg(Color::White),
        );

        StatefulWidget::render(tree, chunks[2], buf, &mut state.tree_state);
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

        // Initially only repository is visible
        let visible = data.visible_nodes();
        assert_eq!(visible.len(), 1);

        // Toggle expand
        data.toggle_at(0);

        // Now all 4 nodes are visible (1 repo + 1 action + 2 workspaces)
        let visible = data.visible_nodes();
        assert_eq!(visible.len(), 4);
    }
}
