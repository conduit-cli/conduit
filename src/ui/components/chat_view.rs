use ansi_to_tui::IntoText;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    render_minimal_scrollbar,
    theme::{
        ACCENT_ERROR, ACCENT_SUCCESS, BG_BASE, DIFF_ADD, DIFF_REMOVE, TOOL_BLOCK_BG, TOOL_COMMAND,
        TOOL_COMMENT, TOOL_OUTPUT,
    },
    ChatMessage, MarkdownRenderer, MessageRole, ScrollbarMetrics, TurnSummary,
};

mod chat_view_cache;

// =============================================================================
// Tool Block Builder - Opencode-style tool rendering
// =============================================================================

/// Helper for building Opencode-style tool blocks with consistent styling.
/// Creates lines with ┃ prefix and full-width background.
struct ToolBlockBuilder {
    width: usize,
    block_style: Style,
    bg_style: Style,
}

impl ToolBlockBuilder {
    fn new(width: usize) -> Self {
        Self {
            width,
            // Use conversation background color as foreground so ┃ blends with surrounding area
            block_style: Style::default().fg(BG_BASE).bg(TOOL_BLOCK_BG),
            bg_style: Style::default().bg(TOOL_BLOCK_BG),
        }
    }

    /// Create a line with ┃ prefix and full-width background
    fn line(&self, spans: Vec<Span<'static>>) -> Line<'static> {
        // Note: "┃" is a box-drawing character with ambiguous width.
        // We treat it as width 1, plus 2 spaces = 3 total prefix width.
        let prefix_width = 3; // "┃" (1) + "  " (2)

        let content_width: usize = spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();

        let total_used = prefix_width + content_width;
        // Add 1 extra for edge padding to ensure full coverage
        let padding_needed = self.width.saturating_sub(total_used).saturating_add(1);

        let mut line_spans = vec![
            Span::styled("┃", self.block_style),
            Span::styled("  ", self.bg_style),
        ];
        line_spans.extend(spans);

        line_spans.push(Span::styled(" ".repeat(padding_needed), self.bg_style));

        Line::from(line_spans)
    }

    /// Create an empty line for padding (fills entire width)
    fn empty_line(&self) -> Line<'static> {
        let prefix_width = 3; // "┃" (1) + "  " (2)
                              // Add 1 extra for edge padding to ensure full coverage
        let padding = self.width.saturating_sub(prefix_width).saturating_add(1);
        Line::from(vec![
            Span::styled("┃", self.block_style),
            Span::styled("  ", self.bg_style),
            Span::styled(" ".repeat(padding), self.bg_style),
        ])
    }

    /// Create a comment line (# prefix, muted color)
    fn comment(&self, text: &str) -> Line<'static> {
        self.line(vec![Span::styled(
            format!("# {}", text),
            Style::default().fg(TOOL_COMMENT).bg(TOOL_BLOCK_BG),
        )])
    }

    /// Create a command line ($ prefix, bright color)
    fn command(&self, text: &str) -> Line<'static> {
        self.line(vec![Span::styled(
            format!("$ {}", text),
            Style::default().fg(TOOL_COMMAND).bg(TOOL_BLOCK_BG),
        )])
    }

    /// Create an output line (normal color)
    fn output(&self, text: &str) -> Line<'static> {
        self.line(vec![Span::styled(
            text.to_string(),
            Style::default().fg(TOOL_OUTPUT).bg(TOOL_BLOCK_BG),
        )])
    }

    /// Create a colored output line
    fn output_colored(&self, text: &str, color: Color) -> Line<'static> {
        self.line(vec![Span::styled(
            text.to_string(),
            Style::default().fg(color).bg(TOOL_BLOCK_BG),
        )])
    }

    /// Create a line with custom spans
    fn custom(&self, spans: Vec<Span<'static>>) -> Line<'static> {
        self.line(spans)
    }

    /// Get the background style for use in custom spans
    fn bg_style(&self) -> Style {
        self.bg_style
    }

    /// Get the content width (total width minus prefix)
    fn content_width(&self) -> usize {
        let prefix_width = 3; // "┃  "
        self.width.saturating_sub(prefix_width).max(1)
    }

    /// Wrap text and return multiple lines with the given color
    fn wrapped_output_colored(&self, text: &str, color: Color) -> Vec<Line<'static>> {
        let content_width = self.content_width();
        let style = Style::default().fg(color).bg(TOOL_BLOCK_BG);
        let spans = vec![Span::styled(text.to_string(), style)];
        let wrapped = wrap_spans(spans, content_width);

        wrapped
            .into_iter()
            .map(|line_spans| self.line(line_spans))
            .collect()
    }

    /// Wrap custom spans and return multiple lines
    fn wrapped_custom(&self, spans: Vec<Span<'static>>) -> Vec<Line<'static>> {
        let content_width = self.content_width();
        let wrapped = wrap_spans(spans, content_width);

        wrapped
            .into_iter()
            .map(|line_spans| self.line(line_spans))
            .collect()
    }
}

use self::chat_view_cache::LineCache;

/// Truncate a string to fit within a maximum display width, adding "..." if truncated.
/// Uses unicode display width to handle multi-byte and wide characters correctly.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let ellipsis = "...";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis);

    if max_width <= ellipsis_width {
        return s.chars().take(max_width).collect();
    }

    let current_width = UnicodeWidthStr::width(s);
    if current_width <= max_width {
        return s.to_string();
    }

    let target_width = max_width - ellipsis_width;
    let mut width = 0;
    let mut result = String::new();

    for c in s.chars() {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > target_width {
            break;
        }
        result.push(c);
        width += char_width;
    }

    result.push_str(ellipsis);
    result
}

/// Truncate a string to fit within a maximum display width (no ellipsis).
/// Uses unicode display width to handle multi-byte and wide characters correctly.
fn truncate_to_width_exact(s: &str, max_width: usize) -> String {
    let current_width = UnicodeWidthStr::width(s);
    if current_width <= max_width {
        return s.to_string();
    }

    let mut width = 0;
    let mut result = String::new();

    for c in s.chars() {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > max_width {
            break;
        }
        result.push(c);
        width += char_width;
    }

    result
}

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

    /// Calculate content area with padding for margins and scrollbar
    fn content_area(area: Rect) -> Option<Rect> {
        let content = Rect {
            x: area.x.saturating_add(2),
            y: area.y,
            width: area.width.saturating_sub(4), // 2 left margin + 1 scrollbar + 1 gap
            height: area.height,
        };
        if content.width < 3 || content.height < 1 {
            return None;
        }
        Some(content)
    }

    /// Calculate scrollbar area (rightmost column)
    fn scrollbar_area(area: Rect) -> Rect {
        Rect {
            x: area.x + area.width.saturating_sub(1),
            y: area.y,
            width: 1,
            height: area.height,
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

        // Auto-scroll to bottom only if user is already at bottom
        // When scroll_offset > 0, user has scrolled up - preserve their position
    }

    /// Update the last tool message with new content and exit code.
    /// Returns true if update was successful, false if no matching tool message was found.
    pub fn update_last_tool(&mut self, content: String, exit_code: Option<i32>) -> bool {
        // Find the last tool message
        if let Some(idx) = self
            .messages
            .iter()
            .rposition(|m| m.role == MessageRole::Tool)
        {
            self.messages[idx].content = content;
            self.messages[idx].exit_code = exit_code;

            // Invalidate cache for this message
            if self.cache_width.is_some() {
                self.invalidate_cache_entry(idx);
                self.update_cache_entry(idx, self.cache_width.unwrap());
            }
            true
        } else {
            false
        }
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

            // Auto-scroll to bottom only if user is already at bottom
            // When scroll_offset > 0, user has scrolled up - preserve their position
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

    pub fn set_scroll_from_top(&mut self, offset_from_top: usize, total: usize, visible: usize) {
        let max_scroll = total.saturating_sub(visible);
        self.scroll_offset = max_scroll.saturating_sub(offset_from_top.min(max_scroll));
    }

    pub fn scrollbar_metrics(
        &mut self,
        area: Rect,
        show_thinking_line: bool,
    ) -> Option<ScrollbarMetrics> {
        let content = Self::content_area(area)?;

        self.ensure_cache(content.width);
        self.ensure_flat_cache();

        if let Some(ref buffer) = self.streaming_buffer {
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
        let indicator_len = if show_thinking_line { 2 } else { 0 }; // indicator + blank line

        let total_lines = cached_len + streaming_len + indicator_len;
        let visible_height = content.height as usize;
        if total_lines <= visible_height {
            return None;
        }

        Some(ScrollbarMetrics {
            area: Self::scrollbar_area(area),
            total: total_lines,
            visible: visible_height,
        })
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
    fn format_assistant_message(
        &self,
        msg: &ChatMessage,
        width: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
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

            let line_text: String = content_spans.iter().map(|s| s.content.as_ref()).collect();
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
    fn format_system_message(
        &self,
        msg: &ChatMessage,
        width: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
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
    fn format_error_message(
        &self,
        msg: &ChatMessage,
        width: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        let content_lines: Vec<&str> = msg.content.lines().collect();
        let prefix_first = vec![Span::styled("✗ ", Style::default().fg(Color::Red))];
        let prefix_next = vec![Span::raw("  ")];
        let prefix_first_width = UnicodeWidthStr::width("✗ ");
        let prefix_next_width = UnicodeWidthStr::width("  ");
        let text_style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);

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

    /// Format tool messages using Opencode-style blocks
    fn format_tool_message(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        let tool_name = msg.tool_name.as_deref().unwrap_or("Tool");

        // Special formatting for TodoWrite
        if tool_name == "TodoWrite" {
            return self.format_todowrite(msg, width, lines);
        }

        let tool_args = msg.tool_args.as_deref().unwrap_or("");
        let content_lines: Vec<&str> = msg.content.lines().collect();
        let line_count = content_lines.len();

        // Determine if error
        let is_error = if let Some(code) = msg.exit_code {
            code != 0
        } else {
            msg.content.starts_with("Error:")
        };

        let builder = ToolBlockBuilder::new(width);

        // === Top padding ===
        lines.push(builder.empty_line());

        // === Description line (# comment) ===
        let description = self.get_tool_description(tool_name, tool_args);
        lines.push(builder.comment(&description));

        // === Blank line after description ===
        lines.push(builder.empty_line());

        // === Command line ($ command) ===
        let command_display = self.get_tool_command(tool_name, tool_args, width.saturating_sub(6));
        lines.push(builder.command(&command_display));

        // === Blank line after title section ===
        lines.push(builder.empty_line());

        // === Output content ===
        let line_word = if line_count == 1 { "line" } else { "lines" };
        if msg.is_collapsed {
            // Collapsed: show summary
            let summary = if line_count > 0 {
                format!("▶ {} {} (click to expand)", line_count, line_word)
            } else {
                "▶ No output".to_string()
            };
            lines.push(builder.output(&summary));
        } else {
            // Expanded: show output lines
            let max_display_lines = 50;
            let truncated = line_count > max_display_lines;
            let display_lines = if truncated {
                &content_lines[..max_display_lines]
            } else {
                &content_lines[..]
            };

            for line in display_lines {
                // Check for diff-style lines
                let (line_color, line_text) = if line.starts_with('+') && !line.starts_with("+++") {
                    (DIFF_ADD, line.to_string())
                } else if line.starts_with('-') && !line.starts_with("---") {
                    (DIFF_REMOVE, line.to_string())
                } else if line.starts_with("Error:") || line.contains("error:") {
                    (ACCENT_ERROR, line.to_string())
                } else {
                    // Parse ANSI escape codes
                    let parsed = line.as_bytes().into_text();
                    match parsed {
                        Ok(text) => {
                            // If ANSI parsed successfully, add spans with background
                            let mut spans: Vec<Span<'static>> = text
                                .lines
                                .into_iter()
                                .flat_map(|l| l.spans)
                                .map(|s| {
                                    Span::styled(s.content.into_owned(), s.style.bg(TOOL_BLOCK_BG))
                                })
                                .collect();
                            if spans.is_empty() {
                                spans.push(Span::styled("", builder.bg_style()));
                            }
                            // Wrap ANSI-parsed lines
                            lines.extend(builder.wrapped_custom(spans));
                            continue;
                        }
                        Err(_) => (TOOL_OUTPUT, line.to_string()),
                    }
                };

                // Wrap long lines
                lines.extend(builder.wrapped_output_colored(&line_text, line_color));
            }

            // Truncation notice
            if truncated {
                let remaining = line_count - max_display_lines;
                let more_word = if remaining == 1 { "line" } else { "lines" };
                lines.push(builder.output_colored(
                    &format!("... ({} more {})", remaining, more_word),
                    TOOL_COMMENT,
                ));
            }
        }

        // === Blank line before status ===
        lines.push(builder.empty_line());

        // === Status line ===
        let status_text = if is_error {
            if let Some(code) = msg.exit_code {
                format!("✗ Failed (exit: {})", code)
            } else {
                "✗ Failed".to_string()
            }
        } else if let Some(code) = msg.exit_code {
            format!("✓ Completed (exit: {})", code)
        } else {
            format!("✓ {} {}", line_count, line_word)
        };

        let status_color = if is_error {
            ACCENT_ERROR
        } else {
            ACCENT_SUCCESS
        };
        lines.push(builder.output_colored(&status_text, status_color));

        // === Bottom padding ===
        lines.push(builder.empty_line());
    }

    /// Get a human-readable description for a tool invocation
    fn get_tool_description(&self, tool_name: &str, tool_args: &str) -> String {
        // Try to extract description from JSON args if present
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(tool_args) {
            if let Some(desc) = json.get("description").and_then(|d| d.as_str()) {
                return desc.to_string();
            }
        }

        // Default descriptions by tool type
        match tool_name {
            "Bash" => "Run command".to_string(),
            "Read" => "Read file".to_string(),
            "Write" => "Write file".to_string(),
            "Edit" => "Edit file".to_string(),
            "Glob" => "Find files".to_string(),
            "Grep" => "Search for pattern".to_string(),
            "LS" => "List directory".to_string(),
            "Task" => "Run agent".to_string(),
            _ => tool_name.to_string(),
        }
    }

    /// Get the command/path to display for a tool invocation
    fn get_tool_command(&self, tool_name: &str, tool_args: &str, max_width: usize) -> String {
        // Try to parse as JSON first
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(tool_args) {
            let command = match tool_name {
                "Bash" | "exec_command" | "shell" | "local_shell_call" | "command_execution" => {
                    json.get("command")
                        .and_then(|c| c.as_str())
                        .map(String::from)
                }
                "Read" | "read_file" => {
                    let path = json.get("file_path").and_then(|p| p.as_str()).unwrap_or("");
                    let offset = json.get("offset").and_then(|o| o.as_i64());
                    let limit = json.get("limit").and_then(|l| l.as_i64());
                    if let (Some(off), Some(lim)) = (offset, limit) {
                        Some(format!("{} (lines {}-{})", path, off, off + lim))
                    } else {
                        Some(path.to_string())
                    }
                }
                "Write" | "write_file" | "Edit" => json
                    .get("file_path")
                    .and_then(|p| p.as_str())
                    .map(String::from),
                "Glob" => {
                    let pattern = json.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
                    let path = json.get("path").and_then(|p| p.as_str());
                    if let Some(p) = path {
                        Some(format!("{} in {}", pattern, p))
                    } else {
                        Some(pattern.to_string())
                    }
                }
                "Grep" => {
                    let pattern = json.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
                    let path = json.get("path").and_then(|p| p.as_str()).unwrap_or(".");
                    Some(format!("\"{}\" in {}", pattern, path))
                }
                "Task" => {
                    let prompt = json.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
                    let agent_type = json
                        .get("subagent_type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("agent");
                    Some(format!(
                        "[{}] {}",
                        agent_type,
                        truncate_to_width(prompt, max_width.saturating_sub(15))
                    ))
                }
                _ => None,
            };

            if let Some(cmd) = command {
                return truncate_to_width(&cmd, max_width);
            }
        }

        // Fallback: use raw args (truncated)
        truncate_to_width(tool_args, max_width)
    }

    /// Format TodoWrite tool message
    fn format_todowrite(&self, msg: &ChatMessage, width: usize, lines: &mut Vec<Line<'static>>) {
        let tool_args = msg.tool_args.as_deref().unwrap_or("{}");

        // Parse todos
        let todos: Vec<(String, String)> =
            match serde_json::from_str::<serde_json::Value>(tool_args) {
                Ok(json) => {
                    if let Some(todos_array) = json.get("todos").and_then(|t| t.as_array()) {
                        todos_array
                            .iter()
                            .filter_map(|todo| {
                                let content = todo.get("content").and_then(|c| c.as_str())?;
                                let status = todo
                                    .get("status")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("pending");
                                Some((content.to_string(), status.to_string()))
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
                Err(_) => Vec::new(),
            };

        let total = todos.len();
        let completed = todos.iter().filter(|(_, s)| s == "completed").count();
        let in_progress = todos.iter().filter(|(_, s)| s == "in_progress").count();

        let builder = ToolBlockBuilder::new(width);

        // Top padding
        lines.push(builder.empty_line());

        // Description
        lines.push(builder.comment("Update todo list"));

        // Blank line
        lines.push(builder.empty_line());

        if msg.is_collapsed {
            // Collapsed view
            let summary = format!(
                "▶ {} tasks: {} completed, {} in progress, {} pending",
                total,
                completed,
                in_progress,
                total.saturating_sub(completed).saturating_sub(in_progress)
            );
            lines.push(builder.output(&summary));
        } else {
            // Expanded view - show todo items
            let max_display = 15;
            let display_todos = if todos.len() > max_display {
                &todos[..max_display]
            } else {
                &todos[..]
            };

            for (content, status) in display_todos {
                let (icon, text_color) = match status.as_str() {
                    "completed" => ("✅", TOOL_COMMENT),
                    "in_progress" => ("🔄", TOOL_COMMAND),
                    _ => ("⬜", TOOL_OUTPUT),
                };

                let display_content = truncate_to_width(content, 70);
                lines.push(builder.custom(vec![
                    Span::styled(format!("{} ", icon), builder.bg_style()),
                    Span::styled(
                        display_content,
                        Style::default().fg(text_color).bg(TOOL_BLOCK_BG),
                    ),
                ]));
            }

            if todos.len() > max_display {
                let remaining = todos.len() - max_display;
                lines.push(
                    builder.output_colored(&format!("... (+{} more)", remaining), TOOL_COMMENT),
                );
            }
        }

        // Status line
        let status_text = format!("{}/{} completed", completed, total);
        let status_color = if completed == total && total > 0 {
            ACCENT_SUCCESS
        } else if in_progress > 0 {
            Color::Yellow
        } else {
            ACCENT_SUCCESS
        };

        lines.push(builder.empty_line());
        lines.push(builder.output_colored(&status_text, status_color));

        // Bottom padding
        lines.push(builder.empty_line());
    }

    /// Format turn summary message
    fn format_summary_message(
        &self,
        msg: &ChatMessage,
        width: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        if let Some(ref summary) = msg.summary {
            lines.push(Line::from(Span::raw("")));
            lines.push(self.render_summary_divider(summary, width));
            lines.push(Line::from(Span::raw("")));
        }
    }

    fn render_summary_divider(&self, summary: &TurnSummary, width: usize) -> Line<'static> {
        let duration = summary.format_duration();
        let input_tokens = TurnSummary::format_tokens(summary.input_tokens);
        let output_tokens = TurnSummary::format_tokens(summary.output_tokens);
        let mut text = format!("─ ⏱ {duration} │ ↓{input_tokens} ↑{output_tokens} ");
        let target_width = width.max(1);
        let current_width = UnicodeWidthStr::width(text.as_str());
        if current_width < target_width {
            text.push_str(&"─".repeat(target_width - current_width));
        } else if current_width > target_width {
            // Use display-width-aware truncation for proper UTF-8/wide char handling
            text = truncate_to_width_exact(&text, target_width);
        }
        Line::from(Span::styled(text, Style::default().fg(Color::DarkGray)))
    }

    /// Render the chat view
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.render_with_indicator(area, buf, None);
    }

    /// Render the chat view with an optional thinking indicator
    pub fn render_with_indicator(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        thinking_line: Option<Line<'static>>,
    ) {
        let Some(content) = Self::content_area(area) else {
            return;
        };

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
        let indicator_len = if thinking_line.is_some() { 2 } else { 0 }; // indicator + blank line

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
                visible_lines.extend(cached_streaming[range_start..range_end].iter().cloned());
            }
        }

        // Thinking indicator + blank line for spacing from input box
        if let Some(indicator) = thinking_line {
            let indicator_index = streaming_end;
            let blank_index = streaming_end + 1;
            if start_line <= indicator_index && end_line > indicator_index {
                visible_lines.push(indicator);
            }
            if start_line <= blank_index && end_line > blank_index {
                visible_lines.push(Line::from(""));
            }
        }
        Paragraph::new(visible_lines).render(content, buf);

        render_minimal_scrollbar(
            Self::scrollbar_area(area),
            buf,
            total_lines,
            visible_height,
            scroll_from_top,
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
        spans.push(Span::styled(buffer, current_style.unwrap_or_default()));
    }

    spans
}

impl Default for ChatView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_scrolls_to_bottom_when_already_at_bottom() {
        let mut view = ChatView::new();
        view.push(ChatMessage::user("First message"));
        assert_eq!(view.scroll_offset, 0);

        view.push(ChatMessage::assistant("Response"));
        assert_eq!(
            view.scroll_offset, 0,
            "Should stay at bottom when already at bottom"
        );
    }

    #[test]
    fn test_push_preserves_scroll_when_user_scrolled_up() {
        let mut view = ChatView::new();
        view.push(ChatMessage::user("Message 1"));
        view.push(ChatMessage::assistant("Response 1"));

        // User scrolls up
        view.scroll_up(5);
        assert_eq!(view.scroll_offset, 5);

        // New message arrives
        view.push(ChatMessage::assistant("Response 2"));

        // Scroll position should be preserved (not reset to 0)
        assert!(
            view.scroll_offset > 0,
            "Scroll position should be preserved when user has scrolled up, got {}",
            view.scroll_offset
        );
    }

    #[test]
    fn test_finalize_streaming_preserves_scroll_when_user_scrolled_up() {
        let mut view = ChatView::new();
        view.push(ChatMessage::user("Question"));

        // Start streaming
        view.stream_append("Streaming content...");

        // User scrolls up during streaming
        view.scroll_up(3);
        assert_eq!(view.scroll_offset, 3);

        // Finalize streaming
        view.finalize_streaming();

        // Scroll should be preserved
        assert!(
            view.scroll_offset > 0,
            "Scroll position should be preserved after finalize_streaming, got {}",
            view.scroll_offset
        );
    }

    #[test]
    fn test_finalize_streaming_stays_at_bottom_when_at_bottom() {
        let mut view = ChatView::new();
        view.stream_append("Streaming...");
        assert_eq!(view.scroll_offset, 0);

        view.finalize_streaming();
        assert_eq!(
            view.scroll_offset, 0,
            "Should stay at bottom when already at bottom"
        );
    }

    #[test]
    fn test_tool_message_block_style() {
        let mut view = ChatView::new();

        // Add a Bash tool message
        let tool_msg = ChatMessage::tool_with_exit(
            "Bash",
            r#"{"command": "ls -la", "description": "List files"}"#,
            "total 0\ndrwxr-xr-x  2 user staff 64 Jan  1 00:00 .\ndrwxr-xr-x 10 user staff 320 Jan  1 00:00 ..",
            Some(0),
        );
        view.push(tool_msg);

        // The view should have the tool message
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].role, MessageRole::Tool);
    }

    #[test]
    fn test_tool_command_parsing_bash() {
        let view = ChatView::new();
        let result = view.get_tool_command(
            "Bash",
            r#"{"command": "cargo test", "description": "Run tests"}"#,
            100,
        );
        assert_eq!(result, "cargo test");
    }

    #[test]
    fn test_tool_command_parsing_read() {
        let view = ChatView::new();
        let result = view.get_tool_command(
            "Read",
            r#"{"file_path": "/path/to/file.rs", "offset": 10, "limit": 50}"#,
            100,
        );
        assert_eq!(result, "/path/to/file.rs (lines 10-60)");
    }

    #[test]
    fn test_tool_command_parsing_grep() {
        let view = ChatView::new();
        let result =
            view.get_tool_command("Grep", r#"{"pattern": "fn main", "path": "src/"}"#, 100);
        assert_eq!(result, "\"fn main\" in src/");
    }

    #[test]
    fn test_tool_description_with_custom() {
        let view = ChatView::new();
        let result = view.get_tool_description(
            "Bash",
            r#"{"command": "ls", "description": "List directory contents"}"#,
        );
        assert_eq!(result, "List directory contents");
    }

    #[test]
    fn test_tool_description_default() {
        let view = ChatView::new();
        let result = view.get_tool_description("Read", r#"{"file_path": "/path/to/file"}"#);
        assert_eq!(result, "Read file");
    }
}
