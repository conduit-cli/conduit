use ansi_to_tui::IntoText;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Widget,
    },
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    render_vertical_scrollbar, ChatMessage, MarkdownRenderer, MessageRole, ScrollbarSymbols,
};

mod chat_view_cache;

use self::chat_view_cache::LineCache;

/// Chat view component displaying message history
pub struct ChatView {
    /// All messages in the chat
    messages: Vec<ChatMessage>,
    /// Scroll offset (0 = bottom, increases upward)
    scroll_offset: usize,
    /// Currently streaming message buffer
    streaming_buffer: Option<String>,
    /// Cached rendered lines per message
    line_cache: LineCache,
    /// Width the cache was built for (invalidate on change)
    cache_width: Option<u16>,
    /// Flattened cache of all message lines
    flat_cache: Vec<Line<'static>>,
    /// Width the flattened cache was built for
    flat_cache_width: Option<u16>,
    /// Whether the flattened cache needs rebuilding
    flat_cache_dirty: bool,
    /// Cached lines for current streaming message
    streaming_cache: Option<Vec<Line<'static>>>,
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            streaming_buffer: None,
            line_cache: LineCache::default(),
            cache_width: None,
            flat_cache: Vec::new(),
            flat_cache_width: None,
            flat_cache_dirty: true,
            streaming_cache: None,
        }
    }

    /// Add a message to the chat
    pub fn push(&mut self, message: ChatMessage) {
        // If we were streaming, finalize it
        if self.streaming_buffer.is_some() {
            self.finalize_streaming();
        }

        // Update previous message's spacing if needed (it may have changed)
        if !self.messages.is_empty() && self.cache_width.is_some() {
            let prev_idx = self.messages.len() - 1;
            self.invalidate_cache_entry(prev_idx);
            self.update_cache_entry(prev_idx, self.cache_width.unwrap());
        }

        self.messages.push(message);

        // Add cache entry for new message if cache is active
        if let Some(width) = self.cache_width {
            let idx = self.messages.len() - 1;
            self.update_cache_entry(idx, width);
        }

        // Auto-scroll to bottom on new message
        self.scroll_offset = 0;
    }

    /// Start or append to streaming message
    pub fn stream_append(&mut self, text: &str) {
        match &mut self.streaming_buffer {
            Some(buffer) => {
                buffer.push_str(text);
            }
            None => {
                self.streaming_buffer = Some(text.to_string());
            }
        }
        // Invalidate streaming cache so it gets rebuilt on next render
        self.streaming_cache = None;
    }

    /// Finalize streaming message and add to history
    pub fn finalize_streaming(&mut self) {
        if let Some(content) = self.streaming_buffer.take() {
            // Clear streaming cache
            self.streaming_cache = None;

            // Update previous message's spacing if needed
            if !self.messages.is_empty() && self.cache_width.is_some() {
                let prev_idx = self.messages.len() - 1;
                self.invalidate_cache_entry(prev_idx);
                self.update_cache_entry(prev_idx, self.cache_width.unwrap());
            }

            self.messages.push(ChatMessage::assistant(content));

            // Add cache entry for new message
            if let Some(width) = self.cache_width {
                let idx = self.messages.len() - 1;
                self.update_cache_entry(idx, width);
            }

            self.scroll_offset = 0;
        }
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
        self.streaming_buffer = None;
        self.scroll_offset = 0;
        // Clear all caches
        self.line_cache = LineCache::default();
        self.flat_cache.clear();
        self.flat_cache_width = self.cache_width;
        self.flat_cache_dirty = false;
        self.streaming_cache = None;
        // Keep cache_width so we don't have to recalculate on next render
    }

    /// Scroll up by n lines
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    /// Scroll down by n lines
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        // Will be clamped during render
        self.scroll_offset = usize::MAX;
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get message count
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.streaming_buffer.is_none()
    }

    /// Get all messages (for debug dump)
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Get streaming buffer (for debug dump)
    pub fn streaming_buffer(&self) -> Option<&str> {
        self.streaming_buffer.as_deref()
    }

    /// Toggle collapsed state for a tool message at the given index
    pub fn toggle_tool_at(&mut self, index: usize) {
        if let Some(msg) = self.messages.get_mut(index) {
            if msg.role == MessageRole::Tool {
                msg.is_collapsed = !msg.is_collapsed;
                // Invalidate and update cache for this message
                if let Some(width) = self.cache_width {
                    self.invalidate_cache_entry(index);
                    self.update_cache_entry(index, width);
                }
            }
        }
    }

    /// Collapse all tool messages
    pub fn collapse_all_tools(&mut self) {
        let mut changed_indices = Vec::new();
        for (i, msg) in self.messages.iter_mut().enumerate() {
            if msg.role == MessageRole::Tool && !msg.is_collapsed {
                msg.is_collapsed = true;
                changed_indices.push(i);
            }
        }
        // Update cache for changed messages
        if let Some(width) = self.cache_width {
            for idx in changed_indices {
                self.invalidate_cache_entry(idx);
                self.update_cache_entry(idx, width);
            }
        }
    }

    /// Expand all tool messages
    pub fn expand_all_tools(&mut self) {
        let mut changed_indices = Vec::new();
        for (i, msg) in self.messages.iter_mut().enumerate() {
            if msg.role == MessageRole::Tool && msg.is_collapsed {
                msg.is_collapsed = false;
                changed_indices.push(i);
            }
        }
        // Update cache for changed messages
        if let Some(width) = self.cache_width {
            for idx in changed_indices {
                self.invalidate_cache_entry(idx);
                self.update_cache_entry(idx, width);
            }
        }
    }

    /// Get indices of all tool messages
    pub fn tool_message_indices(&self) -> Vec<usize> {
        self.messages
            .iter()
            .enumerate()
            .filter_map(|(i, msg)| {
                if msg.role == MessageRole::Tool {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    fn format_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        match msg.role {
            MessageRole::Tool => self.format_tool_message(msg, width, lines),
            MessageRole::User => self.format_user_message(msg, width, lines),
            MessageRole::Assistant => self.format_assistant_message(msg, width, lines),
            MessageRole::System => self.format_system_message(msg, width, lines),
            MessageRole::Error => self.format_error_message(msg, width, lines),
            MessageRole::Summary => self.format_summary_message(msg, width, lines),
        }
    }

    /// Format user messages with chevron prefix
    fn format_user_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        let content_lines: Vec<&str> = msg.content.lines().collect();
        let prefix_first = vec![Span::styled("❯ ", Style::default().fg(Color::Green))];
        let prefix_next = vec![Span::raw("  ")];
        let prefix_first_width = UnicodeWidthStr::width("❯ ");
        let prefix_next_width = UnicodeWidthStr::width("  ");
        let text_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);

        for (i, line) in content_lines.iter().enumerate() {
            let content_spans = vec![Span::styled(line.to_string(), text_style)];
            let (prefix, prefix_width) = if i == 0 {
                (prefix_first.clone(), prefix_first_width)
            } else {
                (prefix_next.clone(), prefix_next_width)
            };
            let content_width = width.saturating_sub(prefix_width).max(1);
            let wrapped = wrap_spans(content_spans, content_width);
            for (idx, wrapped_spans) in wrapped.into_iter().enumerate() {
                let mut line_spans = if idx == 0 {
                    prefix.clone()
                } else {
                    prefix_next.clone()
                };
                line_spans.extend(wrapped_spans);
                lines.push(Line::from(line_spans));
            }
        }
    }

    /// Format assistant messages - flowing text with markdown
    fn format_assistant_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        if msg.content.is_empty() {
            return;
        }

        // Parse markdown with custom renderer
        let renderer = MarkdownRenderer::new();
        let md_text = renderer.render(&msg.content);

        let bullet_prefix = vec![Span::raw("• ")];
        let continuation_prefix = vec![Span::raw("  ")];
        let bullet_width = UnicodeWidthStr::width("• ");
        let continuation_width = UnicodeWidthStr::width("  ");

        let mut first_content_line = true;
        for line in md_text.lines {
            if line.spans.is_empty() {
                lines.push(Line::from(""));
                continue;
            }

            let content_spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|s| {
                    // Apply a slightly dimmer style for assistant text
                    let mut style = s.style;
                    if style.fg.is_none() {
                        style = style.fg(Color::Rgb(220, 220, 220)); // Slightly dimmer white
                    }
                    Span::styled(s.content.into_owned(), style)
                })
                .collect();

            let line_text: String = content_spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            let trimmed = line_text.trim_start();
            let is_list_item = trimmed.starts_with("• ")
                || trimmed.starts_with("- ")
                || trimmed
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && trimmed.get(1..2) == Some(".")
                    && trimmed.get(2..3) == Some(" ");

            let (first_prefix, first_prefix_width) = if first_content_line && !is_list_item {
                (bullet_prefix.clone(), bullet_width)
            } else {
                (continuation_prefix.clone(), continuation_width)
            };

            let content_width = width.saturating_sub(first_prefix_width).max(1);
            let wrapped = wrap_spans(content_spans, content_width);
            for (idx, wrapped_spans) in wrapped.into_iter().enumerate() {
                let prefix = if idx == 0 {
                    first_prefix.clone()
                } else {
                    continuation_prefix.clone()
                };
                let mut line_spans = prefix;
                line_spans.extend(wrapped_spans);
                lines.push(Line::from(line_spans));
            }

            if first_content_line {
                first_content_line = false;
            }
        }

        // Add streaming indicator if still streaming
        if msg.is_streaming {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "…",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ]));
        }
    }

    /// Format system messages with info symbol
    fn format_system_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        let content_lines: Vec<&str> = msg.content.lines().collect();
        let prefix_first = vec![Span::styled("ℹ ", Style::default().fg(Color::Blue))];
        let prefix_next = vec![Span::raw("  ")];
        let prefix_first_width = UnicodeWidthStr::width("ℹ ");
        let prefix_next_width = UnicodeWidthStr::width("  ");
        let text_style = Style::default().fg(Color::Blue);

        for (i, line) in content_lines.iter().enumerate() {
            let content_spans = vec![Span::styled(line.to_string(), text_style)];
            let (prefix, prefix_width) = if i == 0 {
                (prefix_first.clone(), prefix_first_width)
            } else {
                (prefix_next.clone(), prefix_next_width)
            };
            let content_width = width.saturating_sub(prefix_width).max(1);
            let wrapped = wrap_spans(content_spans, content_width);
            for (idx, wrapped_spans) in wrapped.into_iter().enumerate() {
                let mut line_spans = if idx == 0 {
                    prefix.clone()
                } else {
                    prefix_next.clone()
                };
                line_spans.extend(wrapped_spans);
                lines.push(Line::from(line_spans));
            }
        }
    }

    /// Format error messages with X symbol
    fn format_error_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        let content_lines: Vec<&str> = msg.content.lines().collect();
        let prefix_first = vec![Span::styled("✗ ", Style::default().fg(Color::Red))];
        let prefix_next = vec![Span::raw("  ")];
        let prefix_first_width = UnicodeWidthStr::width("✗ ");
        let prefix_next_width = UnicodeWidthStr::width("  ");
        let text_style = Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD);

        for (i, line) in content_lines.iter().enumerate() {
            let content_spans = vec![Span::styled(line.to_string(), text_style)];
            let (prefix, prefix_width) = if i == 0 {
                (prefix_first.clone(), prefix_first_width)
            } else {
                (prefix_next.clone(), prefix_next_width)
            };
            let content_width = width.saturating_sub(prefix_width).max(1);
            let wrapped = wrap_spans(content_spans, content_width);
            for (idx, wrapped_spans) in wrapped.into_iter().enumerate() {
                let mut line_spans = if idx == 0 {
                    prefix.clone()
                } else {
                    prefix_next.clone()
                };
                line_spans.extend(wrapped_spans);
                lines.push(Line::from(line_spans));
            }
        }
    }

    /// Get icon for tool type
    fn tool_icon(tool_name: &str) -> &'static str {
        match tool_name {
            "Bash" => "⚡",
            "Read" => "📄",
            "Write" => "💾",
            "Edit" => "✏️",
            "Glob" => "🔍",
            "Grep" => "🔎",
            "LS" => "📂",
            "Task" => "🤖",
            "TodoWrite" => "📋",
            _ => "🔧",
        }
    }

    /// Get status icon for todo items
    fn todo_status_icon(status: &str) -> &'static str {
        match status {
            "completed" => "✅",
            "in_progress" => "🔄",
            "pending" | _ => "⬜",
        }
    }

    /// Format TodoWrite tool as a checkbox list
    fn format_todowrite_message(&self, msg: &ChatMessage, lines: &mut Vec<Line<'static>>) {
        let tool_args = msg.tool_args.as_deref().unwrap_or("{}");

        // Try to parse the todos from arguments
        let todos: Vec<(String, String)> = match serde_json::from_str::<serde_json::Value>(tool_args) {
            Ok(json) => {
                if let Some(todos_array) = json.get("todos").and_then(|t| t.as_array()) {
                    todos_array
                        .iter()
                        .filter_map(|todo| {
                            let content = todo.get("content").and_then(|c| c.as_str())?;
                            let status = todo.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
                            Some((content.to_string(), status.to_string()))
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        };

        // Calculate stats
        let total = todos.len();
        let completed = todos.iter().filter(|(_, s)| s == "completed").count();
        let in_progress = todos.iter().filter(|(_, s)| s == "in_progress").count();

        // Header
        let header_stats = if total > 0 {
            format!("{}/{} completed", completed, total)
        } else {
            "No tasks".to_string()
        };

        lines.push(Line::from(vec![
            Span::styled(
                "┌─ 📋 Todo List ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("─ {} ", header_stats),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        if todos.is_empty() {
            // No todos parsed - show raw content
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    "(Could not parse todo list)",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else if msg.is_collapsed {
            // Collapsed view - show summary
            let summary = format!(
                "{} total: {} completed, {} in progress, {} pending",
                total,
                completed,
                in_progress,
                total - completed - in_progress
            );
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::Cyan)),
                Span::styled("▶ ", Style::default().fg(Color::DarkGray)),
                Span::styled(summary, Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            // Expanded view - show all todos
            let max_display = 15; // Limit displayed items
            let display_todos = if todos.len() > max_display {
                &todos[..max_display]
            } else {
                &todos[..]
            };

            for (content, status) in display_todos {
                let icon = Self::todo_status_icon(status);
                let text_color = match status.as_str() {
                    "completed" => Color::DarkGray,
                    "in_progress" => Color::Yellow,
                    _ => Color::White,
                };

                // Truncate long content
                let display_content = if content.len() > 70 {
                    format!("{}...", &content[..67])
                } else {
                    content.clone()
                };

                lines.push(Line::from(vec![
                    Span::styled("│  ", Style::default().fg(Color::Cyan)),
                    Span::raw(format!("{} ", icon)),
                    Span::styled(display_content, Style::default().fg(text_color)),
                ]));
            }

            // Show truncation notice
            if todos.len() > max_display {
                let remaining = todos.len() - max_display;
                let remaining_pending = todos[max_display..]
                    .iter()
                    .filter(|(_, s)| s != "completed")
                    .count();
                let note = if remaining_pending > 0 {
                    format!("... (+{} more, {} pending)", remaining, remaining_pending)
                } else {
                    format!("... (+{} more)", remaining)
                };
                lines.push(Line::from(vec![
                    Span::styled("│  ", Style::default().fg(Color::Cyan)),
                    Span::styled("   ", Style::default()),
                    Span::styled(note, Style::default().fg(Color::DarkGray)),
                ]));
            }
        }

        // Footer
        let status_icon = if completed == total && total > 0 {
            ("✓", "All done", Color::Green)
        } else if in_progress > 0 {
            ("●", "In progress", Color::Yellow)
        } else {
            ("✓", "Updated", Color::Green)
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("└─ {} ", status_icon.0),
                Style::default().fg(status_icon.2),
            ),
            Span::styled(
                status_icon.1,
                Style::default()
                    .fg(status_icon.2)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    /// Format tool messages as rich cards
    fn format_tool_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        let tool_name = msg.tool_name.as_deref().unwrap_or("Tool");

        // Special formatting for TodoWrite
        if tool_name == "TodoWrite" {
            return self.format_todowrite_message(msg, lines);
        }

        let tool_args = msg.tool_args.as_deref().unwrap_or("");
        let icon = Self::tool_icon(tool_name);
        let content_lines: Vec<&str> = msg.content.lines().collect();
        let line_count = content_lines.len();

        // Determine success/error state
        let (is_error, status_icon, status_color) = if let Some(code) = msg.exit_code {
            if code == 0 {
                (false, "✓", Color::Green)
            } else {
                (true, "✗", Color::Red)
            }
        } else if msg.content.starts_with("Error:") {
            (true, "✗", Color::Red)
        } else {
            (false, "✓", Color::Green)
        };

        // Truncate args if too long
        let args_display = if tool_args.len() > 60 {
            format!("{}...", &tool_args[..57])
        } else {
            tool_args.to_string()
        };

        // Header: ┌─ 🔧 ToolName ─────────────────
        let header_text = format!("┌─ {} {} ", icon, tool_name);
        let mut header_spans = vec![
            Span::styled(
                header_text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ];

        // Add args on same line if short, otherwise on next line
        if !args_display.is_empty() && args_display.len() <= 40 {
            header_spans.push(Span::styled(
                format!("─ {} ", args_display),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(header_spans));

        // Args on separate line if long
        if !args_display.is_empty() && args_display.len() > 40 {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("$ {}", args_display),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }

        // If collapsed, show summary only
        if msg.is_collapsed {
            let summary = if line_count > 0 {
                let first_line = content_lines[0];
                let preview = if first_line.len() > 50 {
                    format!("{}...", &first_line[..47])
                } else {
                    first_line.to_string()
                };
                format!("{} ({} lines)", preview, line_count)
            } else {
                "No output".to_string()
            };

            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::Cyan)),
                Span::styled("▶ ", Style::default().fg(Color::DarkGray)),
                Span::styled(summary, Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            // Show full output with connectors
            let max_lines = 50; // Limit displayed lines
            let truncated = line_count > max_lines;
            let display_lines = if truncated {
                &content_lines[..max_lines]
            } else {
                &content_lines[..]
            };

            let prefix = vec![
                Span::styled("│ ", Style::default().fg(Color::Cyan)),
                Span::raw("  "),
            ];
            let prefix_width = UnicodeWidthStr::width("│ ") + UnicodeWidthStr::width("  ");
            let content_width = width.saturating_sub(prefix_width).max(1);

            for line in display_lines {
                // Parse ANSI escape codes in the line
                let parsed_text = line.as_bytes().into_text();
                let content_spans: Vec<Span<'static>> = match parsed_text {
                    Ok(text) => text
                        .lines
                        .into_iter()
                        .flat_map(|l| l.spans)
                        .map(|s| Span::styled(s.content.into_owned(), s.style))
                        .collect(),
                    Err(_) => {
                        vec![Span::styled(line.to_string(), Style::default().fg(Color::White))]
                    }
                };

                let wrapped = wrap_spans(content_spans, content_width);
                for wrapped_spans in wrapped {
                    let mut line_spans = prefix.clone();
                    line_spans.extend(wrapped_spans);
                    lines.push(Line::from(line_spans));
                }
            }

            // Show truncation notice
            if truncated {
                lines.push(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("  ... ({} more lines)", line_count - max_lines),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        // Footer with status
        let exit_info = if let Some(code) = msg.exit_code {
            format!(" (exit: {})", code)
        } else {
            String::new()
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("└─ {} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled(
                if is_error { "Failed" } else { "Completed" },
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(exit_info, Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(" ─ {} lines", line_count),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    /// Format turn summary message
    fn format_summary_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        if let Some(ref summary) = msg.summary {
            lines.push(summary.render(width));
        }
    }

    /// Render the chat view
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.render_with_indicator(area, buf, None, None);
    }

    /// Render the chat view with an optional thinking indicator and PR badge
    pub fn render_with_indicator(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        thinking_line: Option<Line<'static>>,
        pr_number: Option<u32>,
    ) {
        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Chat ");

        let inner = block.inner(area);
        let content = Rect {
            x: inner.x.saturating_add(1),
            y: inner.y,
            width: inner.width.saturating_sub(3),
            height: inner.height,
        };
        block.render(area, buf);

        // Render PR badge in top-right corner if PR exists
        if let Some(pr_num) = pr_number {
            let badge = format!(" PR #{} ", pr_num);
            let badge_width = badge.len() as u16;
            // Position: top-right corner inside the border
            // area.x + area.width - 1 is the right border
            // So we want to start at: area.x + area.width - 1 - badge_width
            let badge_x = area.x + area.width.saturating_sub(badge_width + 2);
            let badge_y = area.y;

            let badge_span = Span::styled(
                badge,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(139, 233, 253)) // Cyan/teal color
                    .add_modifier(Modifier::BOLD),
            );
            buf.set_span(badge_x, badge_y, &badge_span, badge_width);
        }

        if content.width < 3 || content.height < 1 {
            return;
        }

        // Ensure cache is valid for current width
        self.ensure_cache(content.width);
        self.ensure_flat_cache();

        // Handle streaming buffer (not cached with messages, has its own cache)
        if let Some(ref buffer) = self.streaming_buffer {
            // Check if streaming cache needs update
            if self.streaming_cache.is_none() {
                let msg = ChatMessage::streaming(buffer.clone());
                let mut streaming_lines = Vec::new();
                self.format_message(&msg, content.width as usize, &mut streaming_lines);
                self.streaming_cache = Some(streaming_lines);
            }
        }

        let cached_len = self.flat_cache.len();
        let streaming_len = self
            .streaming_cache
            .as_ref()
            .map(|lines| lines.len())
            .unwrap_or(0);
        let indicator_len = if thinking_line.is_some() { 1 } else { 0 };

        let total_lines = cached_len + streaming_len + indicator_len;
        let visible_height = content.height as usize;

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(visible_height);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Convert scroll_offset (from bottom) to scroll position (from top)
        // scroll_offset=0 means show bottom, so we want to scroll to max position
        let scroll_from_top = max_scroll.saturating_sub(self.scroll_offset);

        let start_line = total_lines.saturating_sub(self.scroll_offset + visible_height);
        let end_line = total_lines.saturating_sub(self.scroll_offset);
        let mut visible_lines: Vec<Line<'static>> = Vec::with_capacity(visible_height);

        // Cached lines range
        let cached_end = cached_len;
        if start_line < cached_end {
            let slice_end = end_line.min(cached_end);
            visible_lines.extend(self.flat_cache[start_line..slice_end].iter().cloned());
        }

        // Streaming lines range
        let streaming_start = cached_end;
        let streaming_end = cached_end + streaming_len;
        if streaming_len > 0 && end_line > streaming_start && start_line < streaming_end {
            if let Some(ref cached_streaming) = self.streaming_cache {
                let range_start = start_line.max(streaming_start) - streaming_start;
                let range_end = end_line.min(streaming_end) - streaming_start;
                visible_lines.extend(
                    cached_streaming[range_start..range_end]
                        .iter()
                        .cloned(),
                );
            }
        }

        // Thinking indicator (single line)
        if let Some(indicator) = thinking_line {
            let indicator_index = streaming_end;
            if start_line <= indicator_index && end_line > indicator_index {
                visible_lines.push(indicator);
            }
        }
        Paragraph::new(visible_lines).render(content, buf);

        render_vertical_scrollbar(
            Rect {
                x: inner.x + inner.width.saturating_sub(1),
                y: inner.y,
                width: 1,
                height: inner.height,
            },
            buf,
            total_lines,
            visible_height,
            scroll_from_top,
            ScrollbarSymbols::arrows(),
        );
    }
}

fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
    if spans.is_empty() {
        return vec![Vec::new()];
    }

    if max_width == 0 {
        return vec![Vec::new()];
    }

    let mut chars: Vec<(char, Style)> = Vec::new();
    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            if ch.is_control() {
                continue;
            }
            chars.push((ch, style));
        }
    }

    if chars.is_empty() {
        return vec![Vec::new()];
    }

    let mut lines: Vec<Vec<(char, Style)>> = Vec::new();
    let mut current: Vec<(char, Style)> = Vec::new();
    let mut line_width = 0usize;
    let mut last_break: Option<(usize, usize)> = None;

    for (ch, style) in chars {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);

        if line_width + ch_width > max_width && !current.is_empty() {
            if let Some((break_idx, break_width)) = last_break {
                let next_line = current.split_off(break_idx);
                lines.push(current);
                current = next_line;
                line_width = line_width.saturating_sub(break_width);
                last_break = None;

                let mut width = 0usize;
                for (idx, (c, _)) in current.iter().enumerate() {
                    let w = UnicodeWidthChar::width(*c).unwrap_or(0);
                    width += w;
                    if c.is_whitespace() {
                        last_break = Some((idx + 1, width));
                    }
                }
            } else {
                lines.push(current);
                current = Vec::new();
                line_width = 0;
                last_break = None;
            }
        }

        current.push((ch, style));
        line_width += ch_width;
        if ch.is_whitespace() {
            last_break = Some((current.len(), line_width));
        }
    }

    lines.push(current);

    lines
        .into_iter()
        .map(|line_chars| chars_to_spans(line_chars))
        .collect()
}

fn chars_to_spans(chars: Vec<(char, Style)>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut current_style: Option<Style> = None;

    for (ch, style) in chars {
        if current_style.map(|s| s == style).unwrap_or(false) {
            buffer.push(ch);
        } else {
            if !buffer.is_empty() {
                spans.push(Span::styled(
                    buffer.clone(),
                    current_style.unwrap_or_default(),
                ));
                buffer.clear();
            }
            current_style = Some(style);
            buffer.push(ch);
        }
    }

    if !buffer.is_empty() {
        spans.push(Span::styled(
            buffer,
            current_style.unwrap_or_default(),
        ));
    }

    spans
}

impl Default for ChatView {
    fn default() -> Self {
        Self::new()
    }
}
