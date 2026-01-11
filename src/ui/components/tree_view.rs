//! Tree view widget for repository/workspace navigation

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use crate::git::GitDiffStats;

use super::{accent_error, accent_success, pr_open_bg, selected_bg, text_muted};

/// Enable mock PR display for layout testing.
/// WARNING: This is for local development only - set to `true` to test layout
/// with fake PR/git data, but must remain `false` in committed code.
const MOCK_SIDEBAR_PR_DISPLAY: bool = false;

// Layout constants for tree view rendering
// Note: DEPTH_0 constant defined for documentation completeness but not currently used
#[allow(dead_code)]
/// Indent width for depth-0 nodes (repositories): " " = 1 space
const DEPTH_0_INDENT_WIDTH: usize = 1;
/// Indent width for depth-1 nodes (workspaces, actions): "  " = 2 spaces
const DEPTH_1_INDENT_WIDTH: usize = 2;
/// Width of expand marker space ("▶ ", "▼ ", or "  "): 2 chars
const EXPAND_MARKER_WIDTH: usize = 2;
/// Total indent width for workspace name line (depth-1 indent + expand marker space)
const WORKSPACE_NAME_LINE_INDENT: usize = DEPTH_1_INDENT_WIDTH + EXPAND_MARKER_WIDTH;

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
/// TODO: Make configurable at runtime via TreeViewState if user-toggle is desired
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
    /// Currently hovered workspace ID (for showing expanded name on hover)
    pub hovered_workspace_id: Option<Uuid>,
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

    /// Clear hover state
    pub fn clear_hover(&mut self) {
        self.hovered_workspace_id = None;
    }

    /// Set hovered workspace
    pub fn set_hover(&mut self, workspace_id: Uuid) {
        self.hovered_workspace_id = Some(workspace_id);
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
                .bg(selected_bg())
                .add_modifier(Modifier::BOLD),
            expand_style: Style::default().fg(Color::Yellow),
            suffix_style: Style::default().fg(text_muted()),
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

    pub fn suffix_style(mut self, style: Style) -> Self {
        self.suffix_style = style;
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
            // Repos (depth=0): " " (1 space) - moves them 1 char right
            // Children (depth=1): "  " (2 spaces) - aligns nicely under repo names
            let indent = if node.depth == 0 {
                " ".to_string()
            } else {
                "  ".to_string()
            };
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
                // Truncate long branch names with "…/suffix" format
                if let Some(suffix) = &node.suffix {
                    // Use centralized constant for indent width (depth-1 indent + expand marker)
                    let available =
                        (inner.width as usize).saturating_sub(WORKSPACE_NAME_LINE_INDENT);
                    let branch_display = truncate_branch_name(suffix, available);
                    spans.push(Span::styled(branch_display, label_style));
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
                    // Build right-side content: git stats + PR badge
                    let right_spans = build_right_side_spans(node);
                    let right_width: usize = right_spans.iter().map(|s| s.width()).sum();

                    // Calculate available space for workspace name using shared helper
                    let total_width = inner.width as usize;
                    let (available_for_name, _, _) = calculate_workspace_name_bounds(
                        total_width,
                        right_width,
                        node.label.chars().count(),
                    );

                    // Check if name needs truncation
                    let name_is_truncated = node.label.chars().count() > available_for_name;

                    // Check if this workspace is being hovered AND name is truncated
                    let is_hovered = state.hovered_workspace_id == Some(node.id);
                    let show_expanded = is_hovered && name_is_truncated;

                    // When hovered AND truncated, show full name (overflows into right side area)
                    // Otherwise show normal (truncated if needed, or full if it fits)
                    let name_display = if show_expanded {
                        // Show full name when hovered and truncated
                        node.label.clone()
                    } else if name_is_truncated {
                        let truncated: String = node
                            .label
                            .chars()
                            .take(available_for_name.saturating_sub(1))
                            .collect();
                        format!("{}…", truncated)
                    } else {
                        node.label.clone()
                    };

                    // Left side: indent + workspace name
                    // Use constant for indent width (matches WORKSPACE_NAME_LINE_INDENT)
                    let name_indent = " ".repeat(WORKSPACE_NAME_LINE_INDENT);
                    let left_spans = vec![
                        Span::raw(name_indent),
                        Span::styled(name_display.clone(), self.suffix_style),
                    ];
                    let left_width: usize = left_spans.iter().map(|s| s.width()).sum();

                    // Render left side (full width when expanded to cover right side)
                    let render_width = if show_expanded {
                        inner.width
                    } else {
                        left_width as u16
                    };
                    let left_line = Line::from(left_spans);
                    let left_area = Rect {
                        x: inner.x,
                        y: name_y,
                        width: render_width,
                        height: 1,
                    };
                    left_line.render(left_area, buf);

                    // Render right side (right-aligned) - hide only when expanded
                    if !show_expanded && !right_spans.is_empty() && right_width < total_width {
                        let right_x = inner.x + (total_width - right_width) as u16;
                        let right_line = Line::from(right_spans);
                        buf.set_line(right_x, name_y, &right_line, right_width as u16);
                    }
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
        tracing::debug!(
            repo_id = %repo_id,
            repo_name = repo_name,
            workspace_count = workspaces.len(),
            "Adding repository to sidebar"
        );

        let mut repo_node = TreeNode::parent(repo_id, repo_name);

        // Add action node as first child
        let action_node = TreeNode::action(repo_id, ActionType::NewWorkspace);
        repo_node = repo_node.with_child(action_node);

        // Add workspace nodes
        for (ws_id, ws_name, branch) in &workspaces {
            tracing::debug!(
                workspace_id = %ws_id,
                workspace_name = ws_name,
                branch = branch,
                has_branch = !branch.is_empty(),
                "Adding workspace to sidebar"
            );
            let ws_node = TreeNode::leaf(*ws_id, ws_name.clone(), branch.clone());
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
        tracing::debug!(
            workspace_id = %workspace_id,
            additions = stats.additions,
            deletions = stats.deletions,
            "Attempting to update workspace git stats"
        );

        for node in &mut self.nodes {
            if node.node_type == NodeType::Repository {
                tracing::debug!(
                    repo_id = %node.id,
                    repo_name = %node.label,
                    child_count = node.children.len(),
                    "Searching repository for workspace"
                );
                for child in &mut node.children {
                    tracing::debug!(
                        child_id = %child.id,
                        child_name = %child.label,
                        child_type = ?child.node_type,
                        target_workspace_id = %workspace_id,
                        "Checking child node"
                    );
                    if child.id == workspace_id && child.node_type == NodeType::Workspace {
                        child.git_stats = Some(stats);
                        tracing::info!(
                            workspace_id = %workspace_id,
                            "Successfully updated git stats for workspace"
                        );
                        return;
                    }
                }
            }
        }
        tracing::warn!(
            workspace_id = %workspace_id,
            "Workspace not found in sidebar - git stats update failed"
        );
    }

    /// Find the workspace ID if the given position is hovering over the workspace name text.
    /// Only triggers if hovering over the visible name portion (not git stats or PR badge).
    ///
    /// - `visual_row`: row within the tree view (0-indexed from tree start)
    /// - `x_in_tree`: x position within the tree inner area (0-indexed)
    /// - `scroll_offset`: current scroll offset
    /// - `inner_width`: width of the tree inner area
    pub fn workspace_at_name_line(
        &self,
        visual_row: usize,
        x_in_tree: usize,
        scroll_offset: usize,
        inner_width: usize,
    ) -> Option<Uuid> {
        let visible = self.visible_nodes();
        let mut current_row: usize = 0;

        for node in visible.iter().skip(scroll_offset) {
            // Blank line before top-level items (depth=0)
            if node.depth == 0 {
                current_row += 1;
            }

            let is_two_line = node.node_type == NodeType::Workspace && node.suffix.is_some();

            if is_two_line {
                // First row is branch line, second row is name line
                let name_line_row = current_row + 1;
                if visual_row == name_line_row {
                    // Calculate name bounds using shared helper (same logic as render)
                    let right_spans = build_right_side_spans(node);
                    let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
                    let (_, name_start, name_width) = calculate_workspace_name_bounds(
                        inner_width,
                        right_width,
                        node.label.chars().count(),
                    );
                    let name_end = name_start + name_width;

                    // Check if x is within the name text area
                    if x_in_tree >= name_start && x_in_tree < name_end {
                        return Some(node.id);
                    }
                    return None; // On this row but not over the name
                }
                current_row += 2;
            } else {
                current_row += 1;
            }

            // Stop if we've passed the target row
            if current_row > visual_row + 1 {
                break;
            }
        }

        None
    }

    /// Update branch name for a workspace by its ID
    ///
    /// Pass `None` to clear the branch (e.g., for detached HEAD state)
    pub fn update_workspace_branch(&mut self, workspace_id: Uuid, branch: Option<String>) {
        tracing::debug!(
            workspace_id = %workspace_id,
            branch = ?branch,
            "Attempting to update workspace branch"
        );

        for node in &mut self.nodes {
            if node.node_type == NodeType::Repository {
                for child in &mut node.children {
                    if child.id == workspace_id && child.node_type == NodeType::Workspace {
                        let old_suffix = child.suffix.clone();
                        child.suffix = branch.clone();
                        tracing::info!(
                            workspace_id = %workspace_id,
                            old_branch = ?old_suffix,
                            new_branch = ?branch,
                            "Updated workspace branch in sidebar"
                        );
                        return;
                    }
                }
            }
        }
    }
}

/// Truncate a string to fit within max_width display columns, appending ellipsis if truncated.
/// Uses unicode display width for proper handling of wide characters.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    // Reserve 1 column for ellipsis
    let target_width = max_width.saturating_sub(1);
    let mut result = String::new();
    let mut current_width = 0;
    for ch in s.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > target_width {
            break;
        }
        result.push(ch);
        current_width += ch_width;
    }
    result.push('…');
    result
}

/// Truncate a branch name to fit available width, using "…/suffix" format
/// E.g., "fcoury/very-long-branch-name" → "…/very-long-branch-name" → "…/very-long-bra…"
/// Uses unicode display width for proper handling of wide characters.
fn truncate_branch_name(branch: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if branch.width() <= max_width {
        return branch.to_string();
    }

    // Try to preserve the part after the last slash
    if let Some(slash_pos) = branch.rfind('/') {
        let suffix = &branch[slash_pos + 1..];
        let prefix_with_ellipsis = format!("…/{}", suffix);

        if prefix_with_ellipsis.width() <= max_width {
            return prefix_with_ellipsis;
        }

        // Still too long, truncate the suffix part too
        // "…/" takes 2 columns, trailing "…" takes 1 column
        let available_for_suffix = max_width.saturating_sub(3); // "…/" + "…" = 3 columns
        if available_for_suffix > 0 {
            let mut truncated_suffix = String::new();
            let mut current_width = 0;
            for ch in suffix.chars() {
                let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if current_width + ch_width > available_for_suffix {
                    break;
                }
                truncated_suffix.push(ch);
                current_width += ch_width;
            }
            return format!("…/{}…", truncated_suffix);
        }
    }

    // No slash or very limited space - just truncate with ellipsis
    truncate_to_width(branch, max_width)
}

/// Calculate the available width for a workspace name and its displayed bounds.
/// Returns (available_for_name, name_start, name_width) where:
/// - available_for_name: max chars that can fit before right-side content
/// - name_start: x position where name text begins
/// - name_width: actual displayed width of name (may be truncated)
fn calculate_workspace_name_bounds(
    inner_width: usize,
    right_side_width: usize,
    name_char_count: usize,
) -> (usize, usize, usize) {
    let available_for_name =
        inner_width.saturating_sub(WORKSPACE_NAME_LINE_INDENT + right_side_width + 1); // +1 for gap
    let name_width = name_char_count.min(available_for_name);
    (available_for_name, WORKSPACE_NAME_LINE_INDENT, name_width)
}

/// Build the right-side spans for a workspace line (git stats + PR badge)
fn build_right_side_spans(node: &TreeNode) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    // Use mock data if enabled, otherwise use real data
    let (additions, deletions, pr_number, pr_passing) = if MOCK_SIDEBAR_PR_DISPLAY {
        // Mock: small realistic values
        (1, 1, Some(42u32), true)
    } else {
        // Real data from node
        let stats = node.git_stats.as_ref();
        let additions = stats.map(|s| s.additions).unwrap_or(0);
        let deletions = stats.map(|s| s.deletions).unwrap_or(0);
        // TODO: Add real PR data to TreeNode when wired up
        (additions, deletions, None, false)
    };

    let has_git_changes = additions > 0 || deletions > 0;
    let has_pr = pr_number.is_some();

    // Git stats: +N -N
    if has_git_changes {
        if additions > 0 {
            spans.push(Span::styled(
                format!("+{}", additions),
                Style::default().fg(accent_success()),
            ));
        }
        if additions > 0 && deletions > 0 {
            spans.push(Span::styled(" ", Style::default()));
        }
        if deletions > 0 {
            spans.push(Span::styled(
                format!("-{}", deletions),
                Style::default().fg(accent_error()),
            ));
        }
    }

    // Space between git stats and PR (no separator)
    if has_git_changes && has_pr {
        spans.push(Span::styled(" ", Style::default()));
    }

    // PR badge: #123✓ with colored background
    if let Some(pr_num) = pr_number {
        let check_icon = if pr_passing { "✓" } else { "✗" };
        let bg_color = if pr_passing {
            pr_open_bg() // Green-ish for passing
        } else {
            accent_error() // Red for failing
        };
        spans.push(Span::styled(
            format!(" #{}{} ", pr_num, check_icon),
            Style::default().bg(bg_color).fg(Color::White),
        ));
    }

    // Trailing space for padding
    if !spans.is_empty() {
        spans.push(Span::raw(" "));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_sidebar() -> SidebarData {
        let mut sidebar = SidebarData::new();
        let repo_id = Uuid::new_v4();
        // Workspace with a long name that will be truncated
        let ws_id = Uuid::new_v4();
        sidebar.add_repository(
            repo_id,
            "test-repo",
            vec![(ws_id, "my-workspace-name".to_string(), "main".to_string())],
        );
        sidebar
    }

    #[test]
    fn test_workspace_at_name_line_layout() {
        // This test verifies the visual row calculation
        // Layout with one repo + one workspace:
        // Row 0: (blank line before repo)
        // Row 1: "test-repo" (repository)
        // Row 2: "+ New workspace" (action)
        // Row 3: "main" (workspace branch line)
        // Row 4: "my-workspace-name" (workspace name line) <-- this is the target

        let sidebar = create_test_sidebar();
        let scroll_offset = 0;
        let inner_width: usize = 30;

        // Print the visual structure for debugging
        let visible = sidebar.visible_nodes();
        println!("Visible nodes:");
        for (i, node) in visible.iter().enumerate() {
            let is_two_line = node.node_type == NodeType::Workspace && node.suffix.is_some();
            println!(
                "  [{}] depth={}, type={:?}, label='{}', suffix={:?}, two_line={}",
                i, node.depth, node.node_type, node.label, node.suffix, is_two_line
            );
        }

        // Calculate expected visual rows
        let mut current_row = 0;
        let mut name_line_row = None;
        let mut workspace_id = None;

        for node in visible.iter() {
            if node.depth == 0 {
                current_row += 1; // blank line before repo
            }
            let is_two_line = node.node_type == NodeType::Workspace && node.suffix.is_some();
            if is_two_line {
                // branch line is current_row, name line is current_row + 1
                name_line_row = Some(current_row + 1);
                workspace_id = Some(node.id);
                current_row += 2;
            } else {
                current_row += 1;
            }
        }

        println!("Expected name line row: {:?}", name_line_row);
        println!("Workspace ID: {:?}", workspace_id);

        // Test: hovering on the name line at x=4 (start of name after indent)
        let result = sidebar.workspace_at_name_line(
            name_line_row.unwrap(),
            4, // x position right at indent start
            scroll_offset,
            inner_width,
        );
        println!(
            "Result for row={}, x=4: {:?}",
            name_line_row.unwrap(),
            result
        );
        assert_eq!(
            result, workspace_id,
            "Should find workspace when hovering on name"
        );
    }

    #[test]
    fn test_workspace_at_name_line_x_bounds() {
        let sidebar = create_test_sidebar();
        let scroll_offset = 0;
        let inner_width: usize = 30;

        // Get workspace ID and calculate expected bounds
        let visible = sidebar.visible_nodes();
        let ws_node = visible
            .iter()
            .find(|n| n.node_type == NodeType::Workspace)
            .unwrap();
        let ws_id = ws_node.id;

        // Calculate expected name bounds (same logic as workspace_at_name_line)
        // Calculate expected bounds using the shared helper
        let right_spans = build_right_side_spans(ws_node);
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let (_, name_start, name_width) = calculate_workspace_name_bounds(
            inner_width,
            right_width,
            ws_node.label.chars().count(),
        );
        let name_end = name_start + name_width;

        // Find the name line row (should be row 4 based on layout)
        // Row 0: blank, Row 1: repo, Row 2: action, Row 3: branch, Row 4: name
        let name_line_row = 4;

        // Before name area - should not match
        assert!(
            sidebar
                .workspace_at_name_line(name_line_row, 0, scroll_offset, inner_width)
                .is_none(),
            "x=0 (before indent) should not match"
        );
        assert!(
            sidebar
                .workspace_at_name_line(name_line_row, 3, scroll_offset, inner_width)
                .is_none(),
            "x=3 (in indent) should not match"
        );

        // In name area - should match
        assert_eq!(
            sidebar.workspace_at_name_line(name_line_row, name_start, scroll_offset, inner_width),
            Some(ws_id),
            "x={} (at name start) should match",
            name_start
        );
        assert_eq!(
            sidebar.workspace_at_name_line(
                name_line_row,
                name_start + 1,
                scroll_offset,
                inner_width
            ),
            Some(ws_id),
            "x={} (in name) should match",
            name_start + 1
        );

        // Past name area - should not match
        assert!(
            sidebar
                .workspace_at_name_line(name_line_row, name_end, scroll_offset, inner_width)
                .is_none(),
            "x={} (past name end) should not match",
            name_end
        );
    }

    #[test]
    fn test_workspace_at_name_line_calculates_bounds_correctly() {
        let sidebar = create_test_sidebar();
        let scroll_offset = 0;
        let inner_width: usize = 30;
        let name_line_row = 4;

        // Get the workspace node to check its label length
        let visible = sidebar.visible_nodes();
        let ws_node = visible
            .iter()
            .find(|n| n.node_type == NodeType::Workspace)
            .unwrap();

        println!(
            "Workspace label: '{}' (len={})",
            ws_node.label,
            ws_node.label.chars().count()
        );

        // Calculate expected bounds using the shared helper
        let right_spans = build_right_side_spans(ws_node);
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let (available_for_name, name_start, name_width) = calculate_workspace_name_bounds(
            inner_width,
            right_width,
            ws_node.label.chars().count(),
        );

        println!(
            "inner_width={}, name_start={}, right_width={}",
            inner_width, name_start, right_width
        );
        println!(
            "available_for_name={}, name_width={}",
            available_for_name, name_width
        );
        println!(
            "Expected name bounds: [{}, {})",
            name_start,
            name_start + name_width
        );

        // x at name_start should be at start of name
        let result_at_start =
            sidebar.workspace_at_name_line(name_line_row, name_start, scroll_offset, inner_width);
        assert!(
            result_at_start.is_some(),
            "x={} should be at name start",
            name_start
        );

        // x just before name_start should fail
        let result_before = sidebar.workspace_at_name_line(
            name_line_row,
            name_start.saturating_sub(1),
            scroll_offset,
            inner_width,
        );
        assert!(
            result_before.is_none(),
            "x={} should be in indent, not name",
            name_start.saturating_sub(1)
        );
    }

    #[test]
    fn test_workspace_at_name_line_wider_sidebar() {
        // Test with a more realistic wider sidebar (40 chars)
        let sidebar = create_test_sidebar();
        let scroll_offset = 0;
        let inner_width: usize = 40;
        let name_line_row = 4;

        // Get the workspace node
        let visible = sidebar.visible_nodes();
        let ws_node = visible
            .iter()
            .find(|n| n.node_type == NodeType::Workspace)
            .unwrap();

        // Calculate expected bounds using the shared helper
        let right_spans = build_right_side_spans(ws_node);
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let (available_for_name, name_start, name_width) = calculate_workspace_name_bounds(
            inner_width,
            right_width,
            ws_node.label.chars().count(),
        );

        println!("\n=== WIDER SIDEBAR TEST (40 chars) ===");
        println!(
            "inner_width={}, name_start={}, right_width={}",
            inner_width, name_start, right_width
        );
        println!(
            "available_for_name={}, name_width={}",
            available_for_name, name_width
        );
        println!(
            "Expected name bounds: [{}, {})",
            name_start,
            name_start + name_width
        );

        // With wider sidebar, we should have more hover area
        let name_end = name_start + name_width;

        // Test all positions
        for x in 0..inner_width {
            let result =
                sidebar.workspace_at_name_line(name_line_row, x, scroll_offset, inner_width);
            let expected = x >= name_start && x < name_end;
            if result.is_some() != expected {
                println!(
                    "MISMATCH at x={}: got {:?}, expected {}",
                    x,
                    result.is_some(),
                    expected
                );
            }
        }

        // Verify specific positions
        assert!(
            sidebar
                .workspace_at_name_line(name_line_row, name_start, scroll_offset, inner_width)
                .is_some(),
            "x={} should be in name area",
            name_start
        );
        assert!(
            sidebar
                .workspace_at_name_line(name_line_row, name_end - 1, scroll_offset, inner_width)
                .is_some(),
            "x={} should be in name area",
            name_end - 1
        );
        assert!(
            sidebar
                .workspace_at_name_line(name_line_row, name_end, scroll_offset, inner_width)
                .is_none(),
            "x={} should be outside name area",
            name_end
        );
    }

    /// Ensure MOCK_SIDEBAR_PR_DISPLAY is false in committed code.
    /// This test will fail CI if someone accidentally commits with the flag enabled.
    #[test]
    fn test_mock_sidebar_pr_display_is_disabled() {
        assert!(
            !MOCK_SIDEBAR_PR_DISPLAY,
            "MOCK_SIDEBAR_PR_DISPLAY must be false in committed code"
        );
    }

    #[test]
    fn test_truncate_branch_name_fits() {
        // Branch fits - no truncation
        assert_eq!(truncate_branch_name("main", 10), "main");
        assert_eq!(truncate_branch_name("feature/test", 20), "feature/test");
    }

    #[test]
    fn test_truncate_branch_name_with_slash() {
        // Branch with slash - preserve suffix after slash
        // max_width 20, "…/" takes 2 chars, leaving 18 for suffix
        // but we need 1 char for trailing "…", so 17 chars from suffix
        assert_eq!(
            truncate_branch_name("fcoury/very-long-branch-name", 20),
            "…/very-long-branch-…" // 20 chars total: 1 + 1 + 17 + 1
        );
        // Suffix fits with ellipsis prefix (input exceeds max)
        assert_eq!(
            truncate_branch_name("username/short", 10), // 14 chars, needs truncation
            "…/short"                                   // 7 chars (fits because suffix is short)
        );
        // Exact fit - no truncation needed
        assert_eq!(
            truncate_branch_name("user/short", 10), // 10 chars = max_width, no truncation
            "user/short"
        );
    }

    #[test]
    fn test_truncate_branch_name_no_slash() {
        // No slash - simple truncation with ellipsis
        assert_eq!(
            truncate_branch_name("very-long-branch-name-without-slash", 15),
            "very-long-bran…"
        );
    }

    #[test]
    fn test_truncate_branch_name_edge_cases() {
        // Empty string
        assert_eq!(truncate_branch_name("", 10), "");
        // Single character
        assert_eq!(truncate_branch_name("a", 10), "a");
        // Exact fit
        assert_eq!(truncate_branch_name("exact", 5), "exact");
        // One over
        assert_eq!(truncate_branch_name("exactx", 5), "exac…");
        // Very small max_width
        assert_eq!(truncate_branch_name("test/branch", 3), "te…");
        // Zero max_width returns empty string
        assert_eq!(truncate_branch_name("any/branch", 0), "");
    }

    #[test]
    fn test_truncate_branch_name_wide_chars() {
        // Wide characters (CJK) take 2 display columns each
        // "日本語" = 3 chars, 6 columns
        assert_eq!(truncate_branch_name("日本語", 6), "日本語"); // Fits exactly
        assert_eq!(truncate_branch_name("日本語", 5), "日本…"); // 2+2+1 = 5 columns
        assert_eq!(truncate_branch_name("日本語", 4), "日…"); // 2+1 = 3 columns (can't fit 日本)

        // Mixed ASCII and wide chars
        // "user/日本語" = "user/" (5 cols) + "日本語" (6 cols) = 11 cols total
        assert_eq!(truncate_branch_name("user/日本語", 11), "user/日本語"); // Fits
        assert_eq!(truncate_branch_name("user/日本語", 8), "…/日本語"); // 1 + 1 + 6 = 8 cols
        assert_eq!(truncate_branch_name("user/日本語", 7), "…/日本…"); // 1 + 1 + 4 + 1 = 7 cols
    }
}
