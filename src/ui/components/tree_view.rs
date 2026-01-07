//! Tree view widget for repository/workspace navigation

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, StatefulWidget, Widget},
};
use uuid::Uuid;

use crate::git::GitDiffStats;

use super::{ACCENT_ERROR, ACCENT_SUCCESS, ACCENT_WARNING, TEXT_MUTED};

/// Display mode for git status in the sidebar
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarGitDisplay {
    /// No git status display
    Off,
    /// Simple colored dot indicator (green=clean, orange=dirty)
    ColoredDot,
    /// Inline stats showing +12 -4 numbers
    #[default]
    InlineStats,
}

/// Current sidebar git display mode (toggle via constant for now)
pub const SIDEBAR_GIT_DISPLAY: SidebarGitDisplay = SidebarGitDisplay::InlineStats;

/// Type of action for action nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionType {
    /// Create a new workspace under the parent repository
    NewWorkspace,
}

/// Type of node in the tree view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// Repository (can have children)
    Repository,
    /// Workspace (leaf node)
    Workspace,
    /// Action node (e.g., "+ New workspace")
    Action(ActionType),
}

/// A node in the tree view
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// Unique identifier
    pub id: Uuid,
    /// Parent repository ID (for action nodes and workspaces)
    pub parent_id: Option<Uuid>,
    /// Display label
    pub label: String,
    /// Optional suffix (e.g., branch name)
    pub suffix: Option<String>,
    /// Child nodes
    pub children: Vec<TreeNode>,
    /// Whether this node is expanded (for parent nodes)
    pub expanded: bool,
    /// Depth in the tree (0 for root nodes)
    pub depth: usize,
    /// Type of this node
    pub node_type: NodeType,
    /// Git diff stats for workspaces (updated by background tracker)
    pub git_stats: Option<GitDiffStats>,
}

impl TreeNode {
    /// Create a new parent node (repository)
    /// Note: Repositories start expanded by default so users can see the "+ New workspace" action
    pub fn parent(id: Uuid, label: impl Into<String>) -> Self {
        Self {
            id,
            parent_id: None,
            label: label.into(),
            suffix: None,
            children: Vec::new(),
            expanded: true,
            depth: 0,
            node_type: NodeType::Repository,
            git_stats: None,
        }
    }

    /// Create a new leaf node (workspace)
    pub fn leaf(id: Uuid, label: impl Into<String>, suffix: impl Into<String>) -> Self {
        Self {
            id,
            parent_id: None, // Will be set when added as child
            label: label.into(),
            suffix: Some(suffix.into()),
            children: Vec::new(),
            expanded: false,
            depth: 1,
            node_type: NodeType::Workspace,
            git_stats: None,
        }
    }

    /// Create a new action node
    pub fn action(parent_id: Uuid, action_type: ActionType) -> Self {
        let label = match action_type {
            ActionType::NewWorkspace => "+ New workspace".to_string(),
        };
        Self {
            id: Uuid::nil(), // Action nodes don't need unique IDs
            parent_id: Some(parent_id),
            label,
            suffix: None,
            children: Vec::new(),
            expanded: false,
            depth: 1, // Will be set when added as child
            node_type: NodeType::Action(action_type),
            git_stats: None,
        }
    }

    /// Check if this is a leaf node (workspace or action)
    pub fn is_leaf(&self) -> bool {
        matches!(self.node_type, NodeType::Workspace | NodeType::Action(_))
    }

    /// Check if this is an action node
    pub fn is_action(&self) -> bool {
        matches!(self.node_type, NodeType::Action(_))
    }

    /// Calculate the visual row height of this node (including spacer for depth-0)
    /// Used for scroll offset calculations
    fn visual_height(&self) -> usize {
        let base_height = if self.node_type == NodeType::Workspace && self.suffix.is_some() {
            2 // Two-line workspace
        } else {
            1
        };
        // Depth-0 nodes have a spacer line before them
        if self.depth == 0 {
            base_height + 1
        } else {
            base_height
        }
    }

    /// Add a child node
    pub fn with_child(mut self, mut child: TreeNode) -> Self {
        child.depth = self.depth + 1;
        child.parent_id = Some(self.id);
        self.children.push(child);
        self
    }

    /// Toggle expanded state
    pub fn toggle_expanded(&mut self) {
        if !self.is_leaf() {
            self.expanded = !self.expanded;
        }
    }

    /// Get all visible nodes as a flat list
    pub fn flatten(&self) -> Vec<&TreeNode> {
        let mut result = vec![self];
        if self.expanded {
            for child in &self.children {
                result.extend(child.flatten());
            }
        }
        result
    }

    /// Get all visible nodes as a mutable flat list
    pub fn flatten_mut(&mut self) -> Vec<&mut TreeNode> {
        let expanded = self.expanded;
        let mut result = vec![self];
        if expanded {
            // Need to use raw pointers to work around borrow checker
            let children_ptr = result[0].children.as_mut_ptr();
            let children_len = result[0].children.len();
            for i in 0..children_len {
                unsafe {
                    let child = &mut *children_ptr.add(i);
                    result.extend(child.flatten_mut());
                }
            }
        }
        result
    }
}

/// State for the tree view
#[derive(Debug, Default)]
pub struct TreeViewState {
    /// Currently selected index in the flattened view
    pub selected: usize,
    /// Scroll offset
    pub offset: usize,
}

impl TreeViewState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move selection up
    pub fn select_previous(&mut self, visible_count: usize) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = visible_count.saturating_sub(1);
        }
    }

    /// Move selection down
    pub fn select_next(&mut self, visible_count: usize) {
        if visible_count > 0 {
            self.selected = (self.selected + 1) % visible_count;
        }
    }
}

/// Tree view widget
pub struct TreeView<'a> {
    /// Root nodes
    nodes: &'a [TreeNode],
    /// Block for border and title
    block: Option<Block<'a>>,
    /// Style for normal items
    style: Style,
    /// Style for selected item
    selected_style: Style,
    /// Style for expanded indicator
    expand_style: Style,
    /// Style for suffix (branch name)
    suffix_style: Style,
}

impl<'a> TreeView<'a> {
    pub fn new(nodes: &'a [TreeNode]) -> Self {
        Self {
            nodes,
            block: None,
            style: Style::default(),
            selected_style: Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            expand_style: Style::default().fg(Color::Yellow),
            suffix_style: Style::default().fg(Color::DarkGray),
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Get all visible nodes flattened
    fn visible_nodes(&self) -> Vec<&TreeNode> {
        self.nodes.iter().flat_map(|n| n.flatten()).collect()
    }
}

impl StatefulWidget for TreeView<'_> {
    type State = TreeViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Render block if present
        let inner = if let Some(block) = &self.block {
            let inner = block.inner(area);
            block.clone().render(area, buf);
            inner
        } else {
            area
        };

        if inner.width < 3 || inner.height < 1 {
            return;
        }

        let visible = self.visible_nodes();
        let visible_count = visible.len();

        if visible_count == 0 {
            return;
        }

        // Ensure selected is in bounds
        if state.selected >= visible_count {
            state.selected = visible_count.saturating_sub(1);
        }

        // Ensure selected item is visible using visual rows (not node count)
        // This accounts for spacer lines before depth-0 nodes and two-line workspaces
        let viewport_height = inner.height as usize;

        // Calculate visual row position of selected item
        let selected_visual_start: usize = visible[..state.selected]
            .iter()
            .map(|n| n.visual_height())
            .sum();
        let selected_visual_end = selected_visual_start + visible[state.selected].visual_height();

        // Calculate visual row position of current offset
        let offset_visual_start: usize = visible[..state.offset]
            .iter()
            .map(|n| n.visual_height())
            .sum();

        // Adjust offset if selected is above viewport
        if selected_visual_start < offset_visual_start {
            // Find the node whose visual start is at or before selected_visual_start
            let mut cumulative = 0;
            for (i, node) in visible.iter().enumerate() {
                if cumulative + node.visual_height() > selected_visual_start {
                    state.offset = i;
                    break;
                }
                cumulative += node.visual_height();
            }
        }
        // Adjust offset if selected is below viewport (use >= for inclusive check
        // to ensure multi-line items are fully visible, not partially clipped)
        else if selected_visual_end >= offset_visual_start + viewport_height {
            // Find the smallest offset where selected fits in viewport
            let target_start = selected_visual_end.saturating_sub(viewport_height);
            let mut cumulative = 0;
            for (i, node) in visible.iter().enumerate() {
                if cumulative >= target_start {
                    state.offset = i;
                    break;
                }
                cumulative += node.visual_height();
            }
        }

        // Render visible items (with spacing before top-level items)
        // Track visual rows rendered, not node indices, to properly handle
        // spacers and two-line workspaces
        let mut visual_row: u16 = 0;
        for (node_idx, node) in visible.iter().enumerate().skip(state.offset) {
            // Add blank line before all top-level items
            if node.depth == 0 {
                visual_row += 1;
            }

            let y = inner.y + visual_row;

            // Stop if we've exceeded the visible area
            if y >= inner.y + inner.height {
                break;
            }

            // Build the line
            let indent = "  ".repeat(node.depth);
            let expand_marker = if node.is_leaf() {
                "  "
            } else if node.expanded {
                "▼ "
            } else {
                "▶ "
            };

            // Style based on node type
            let label_style = if node.is_action() {
                Style::default().fg(Color::Cyan)
            } else {
                self.style
            };

            // Check if this is a workspace with a suffix (renders on 2 lines)
            let is_two_line_workspace =
                node.node_type == NodeType::Workspace && node.suffix.is_some();

            // For two-line workspaces: line 1 = branch (suffix), line 2 = name
            let mut spans = vec![
                Span::raw(indent.clone()),
                Span::styled(expand_marker, self.expand_style),
            ];

            if is_two_line_workspace {
                // First line shows branch (suffix) with the primary label style
                if let Some(suffix) = &node.suffix {
                    spans.push(Span::styled(suffix.as_str(), label_style));
                }

                // Add git stats based on display mode
                match SIDEBAR_GIT_DISPLAY {
                    SidebarGitDisplay::Off => {}
                    SidebarGitDisplay::ColoredDot => {
                        // Colored dot: green=clean, orange=dirty
                        if let Some(ref stats) = node.git_stats {
                            if stats.has_changes() {
                                spans
                                    .push(Span::styled("  ●", Style::default().fg(ACCENT_WARNING)));
                            } else {
                                spans
                                    .push(Span::styled("  ●", Style::default().fg(ACCENT_SUCCESS)));
                            }
                        }
                    }
                    SidebarGitDisplay::InlineStats => {
                        // Inline stats: +12 -4 (omit zeros)
                        if let Some(ref stats) = node.git_stats {
                            if stats.has_changes() {
                                spans.push(Span::styled("  ", Style::default().fg(TEXT_MUTED)));

                                let has_additions = stats.additions > 0;
                                let has_deletions = stats.deletions > 0;

                                if has_additions {
                                    spans.push(Span::styled(
                                        format!("+{}", stats.additions),
                                        Style::default().fg(ACCENT_SUCCESS),
                                    ));
                                }

                                if has_additions && has_deletions {
                                    spans.push(Span::styled(" ", Style::default()));
                                }

                                if has_deletions {
                                    spans.push(Span::styled(
                                        format!("-{}", stats.deletions),
                                        Style::default().fg(ACCENT_ERROR),
                                    ));
                                }
                            }
                        }
                    }
                }
            } else {
                // Single line items show label
                spans.push(Span::styled(&node.label, label_style));
                // Non-workspace nodes show suffix inline
                if let Some(suffix) = &node.suffix {
                    spans.push(Span::styled(format!(" ({})", suffix), self.suffix_style));
                }
            }

            let line = Line::from(spans);

            // Determine how many rows this item takes
            let item_height: u16 = if is_two_line_workspace { 2 } else { 1 };

            // Fill background for selection (both lines if workspace)
            if node_idx == state.selected {
                for row in 0..item_height {
                    let sel_y = y + row;
                    if sel_y < inner.y + inner.height {
                        for x in inner.x..inner.x + inner.width {
                            buf[(x, sel_y)].set_style(self.selected_style);
                        }
                    }
                }
            }

            // Render the main line
            let line_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            line.render(line_area, buf);

            // Render name on second line for workspaces (with suffix_style)
            if is_two_line_workspace {
                let name_y = y + 1;
                if name_y < inner.y + inner.height {
                    // Indent for second line: same indent + expand_marker width + extra spacing
                    let name_indent = format!("{}    ", indent);
                    let name_line = Line::from(vec![
                        Span::raw(name_indent),
                        Span::styled(&node.label, self.suffix_style),
                    ]);
                    let name_area = Rect {
                        x: inner.x,
                        y: name_y,
                        width: inner.width,
                        height: 1,
                    };
                    name_line.render(name_area, buf);
                }
                // Account for the extra line in visual row tracking
                visual_row += 1;
            }

            // Re-apply selection background to ensure it covers the text
            if node_idx == state.selected {
                for row in 0..item_height {
                    let sel_y = y + row;
                    if sel_y < inner.y + inner.height {
                        for x in inner.x..inner.x + inner.width {
                            let cell = &mut buf[(x, sel_y)];
                            cell.set_bg(self.selected_style.bg.unwrap_or(Color::Reset));
                        }
                    }
                }
            }

            // Advance visual row for the base row of this node
            visual_row += 1;
        }
    }
}

/// Data structure to build tree from repositories and workspaces
#[derive(Debug, Clone)]
pub struct SidebarData {
    pub nodes: Vec<TreeNode>,
}

impl Default for SidebarData {
    fn default() -> Self {
        Self::new()
    }
}

impl SidebarData {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Add a repository with its workspaces
    pub fn add_repository(
        &mut self,
        repo_id: Uuid,
        repo_name: &str,
        workspaces: Vec<(Uuid, String, String)>, // (id, name, branch)
    ) {
        let mut repo_node = TreeNode::parent(repo_id, repo_name);

        // Add action node as first child
        let action_node = TreeNode::action(repo_id, ActionType::NewWorkspace);
        repo_node = repo_node.with_child(action_node);

        // Add workspace nodes
        for (ws_id, ws_name, branch) in workspaces {
            let ws_node = TreeNode::leaf(ws_id, ws_name, branch);
            repo_node = repo_node.with_child(ws_node);
        }

        self.nodes.push(repo_node);
    }

    /// Get flattened visible nodes
    pub fn visible_nodes(&self) -> Vec<&TreeNode> {
        self.nodes.iter().flat_map(|n| n.flatten()).collect()
    }

    /// Toggle expand state of a node by index
    pub fn toggle_at(&mut self, index: usize) {
        let mut current = 0;
        for node in &mut self.nodes {
            if current == index {
                node.toggle_expanded();
                return;
            }
            current += 1;
            if node.expanded {
                for child in &mut node.children {
                    if current == index {
                        child.toggle_expanded();
                        return;
                    }
                    current += 1;
                }
            }
        }
    }

    /// Get the node at a given index
    pub fn get_at(&self, index: usize) -> Option<&TreeNode> {
        self.visible_nodes().get(index).copied()
    }

    /// Convert a visual row number to item index, accounting for extra spacing.
    /// The tree view adds blank lines before top-level items and two-line workspaces.
    pub fn index_from_visual_row(&self, visual_row: usize, scroll_offset: usize) -> Option<usize> {
        let visible = self.visible_nodes();
        let mut current_row: usize = 0;

        for (i, node) in visible.iter().enumerate().skip(scroll_offset) {
            // Blank line before top-level items (depth=0)
            if node.depth == 0 {
                current_row += 1;
            }

            // Check if click is on this item's row(s)
            let is_two_line = node.node_type == NodeType::Workspace && node.suffix.is_some();
            let item_height = if is_two_line { 2 } else { 1 };

            if visual_row >= current_row && visual_row < current_row + item_height {
                return Some(i);
            }

            current_row += item_height;
        }

        None
    }

    /// Find the visible index of a repository by its ID
    pub fn find_repo_index(&self, repo_id: Uuid) -> Option<usize> {
        self.visible_nodes()
            .iter()
            .position(|node| node.id == repo_id && node.node_type == NodeType::Repository)
    }

    /// Expand a repository by its ID
    pub fn expand_repo(&mut self, repo_id: Uuid) {
        for node in &mut self.nodes {
            if node.id == repo_id {
                node.expanded = true;
                return;
            }
        }
    }

    /// Get IDs of all expanded repositories
    pub fn expanded_repo_ids(&self) -> Vec<Uuid> {
        self.nodes
            .iter()
            .filter(|node| node.node_type == NodeType::Repository && node.expanded)
            .map(|node| node.id)
            .collect()
    }

    /// Get IDs of all collapsed repositories
    pub fn collapsed_repo_ids(&self) -> Vec<Uuid> {
        self.nodes
            .iter()
            .filter(|node| node.node_type == NodeType::Repository && !node.expanded)
            .map(|node| node.id)
            .collect()
    }

    /// Collapse a repository by its ID
    pub fn collapse_repo(&mut self, repo_id: Uuid) {
        for node in &mut self.nodes {
            if node.id == repo_id {
                node.expanded = false;
                return;
            }
        }
    }

    /// Find and focus on a workspace by its ID.
    /// Expands the parent repository and returns the visible index of the workspace.
    pub fn focus_workspace(&mut self, workspace_id: Uuid) -> Option<usize> {
        // First, find which repository contains this workspace and expand it
        for node in &mut self.nodes {
            if node.node_type == NodeType::Repository {
                for child in &node.children {
                    if child.id == workspace_id && child.node_type == NodeType::Workspace {
                        // Found the workspace - expand its parent
                        node.expanded = true;
                        break;
                    }
                }
            }
        }

        // Now find the visible index of the workspace
        self.visible_nodes()
            .iter()
            .position(|node| node.id == workspace_id && node.node_type == NodeType::Workspace)
    }

    /// Update git stats for a workspace by its ID
    pub fn update_workspace_git_stats(&mut self, workspace_id: Uuid, stats: GitDiffStats) {
        for node in &mut self.nodes {
            if node.node_type == NodeType::Repository {
                for child in &mut node.children {
                    if child.id == workspace_id && child.node_type == NodeType::Workspace {
                        child.git_stats = Some(stats);
                        return;
                    }
                }
            }
        }
    }

    /// Update branch name for a workspace by its ID
    pub fn update_workspace_branch(&mut self, workspace_id: Uuid, branch: String) {
        for node in &mut self.nodes {
            if node.node_type == NodeType::Repository {
                for child in &mut node.children {
                    if child.id == workspace_id && child.node_type == NodeType::Workspace {
                        child.suffix = Some(branch);
                        return;
                    }
                }
            }
        }
    }
}
