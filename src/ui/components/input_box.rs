use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};
use std::collections::HashMap;
use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{bg_highlight, input_bg, render_minimal_scrollbar, text_primary, ScrollbarMetrics};
use crate::ui::clipboard_paste::normalize_pasted_path;

const LARGE_PASTE_CHAR_THRESHOLD: usize = 1000;
const HISTORY_MAX: usize = 1000;

#[derive(Debug, Clone)]
struct VisualLine {
    start: usize,
    end: usize,
    prefix_width: u16,
}

#[derive(Debug, Clone)]
pub struct InputSubmit {
    pub text: String,
    pub image_paths: Vec<PathBuf>,
    pub image_placeholders: Vec<String>,
}

#[derive(Debug, Clone)]
struct AttachedImage {
    placeholder: String,
    path: PathBuf,
}

/// Text input component with cursor and history
pub struct InputBox {
    /// Current input text
    input: String,
    /// Cursor position (byte offset)
    cursor_pos: usize,
    /// Command history
    history: Vec<String>,
    /// Current history index (-1 = current input)
    history_index: Option<usize>,
    /// Saved input when navigating history
    saved_input: String,
    /// Whether the input is focused
    focused: bool,
    /// Scroll offset for when content exceeds visible area
    scroll_offset: usize,
    /// Last content width used for wrapping (excludes scrollbar)
    last_content_width: Option<u16>,
    /// Large paste placeholders → actual content
    pending_pastes: Vec<(String, String)>,
    /// Counter for large paste placeholders by size
    large_paste_counters: HashMap<usize, usize>,
    /// Attached images tracked by placeholder
    attached_images: Vec<AttachedImage>,
    /// Selection anchor (byte offset)
    selection_anchor: Option<usize>,
    /// Selection head (byte offset)
    selection_head: Option<usize>,
    /// Whether shell input mode is active (triggered by leading '!')
    shell_mode: bool,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            focused: true,
            scroll_offset: 0,
            last_content_width: None,
            pending_pastes: Vec::new(),
            large_paste_counters: HashMap::new(),
            attached_images: Vec::new(),
            selection_anchor: None,
            selection_head: None,
            shell_mode: false,
        }
    }

    /// Get current input text
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Check whether the input is in shell mode (starts with '!').
    pub fn is_shell_mode(&self) -> bool {
        self.shell_mode
    }

    pub fn set_shell_mode(&mut self, shell_mode: bool) {
        self.shell_mode = shell_mode;
    }

    /// Get expanded input with large paste placeholders resolved.
    pub fn expanded_input(&self) -> String {
        let mut expanded = self.input.clone();
        for (placeholder, actual) in &self.pending_pastes {
            if expanded.contains(placeholder) {
                expanded = expanded.replace(placeholder, actual);
            }
        }
        expanded
    }

    /// Snapshot command history for persistence.
    pub fn history_snapshot(&self) -> Vec<String> {
        if self.history.len() <= HISTORY_MAX {
            return self.history.clone();
        }
        let start = self.history.len().saturating_sub(HISTORY_MAX);
        self.history[start..].to_vec()
    }

    /// Restore command history after session load.
    pub fn set_history(&mut self, history: Vec<String>) {
        if history.len() <= HISTORY_MAX {
            self.history = history;
        } else {
            let start = history.len().saturating_sub(HISTORY_MAX);
            self.history = history[start..].to_vec();
        }
        self.history_index = None;
        self.saved_input.clear();
    }

    /// Snapshot attached images to preserve placeholders when editing.
    pub fn attachments_snapshot(&self) -> Vec<(PathBuf, String)> {
        self.attached_images
            .iter()
            .map(|img| (img.path.clone(), img.placeholder.clone()))
            .collect()
    }

    /// Set input text
    pub fn set_input(&mut self, text: String) {
        self.input = text;
        self.cursor_pos = self.input.len();
        self.pending_pastes.clear();
        self.attached_images.clear();
        self.clear_selection();
    }

    /// Set input text with attached images restored.
    pub fn set_input_with_attachments(
        &mut self,
        text: String,
        attachments: Vec<(PathBuf, String)>,
    ) {
        self.input = text;
        self.cursor_pos = self.input.len();
        self.pending_pastes.clear();
        self.attached_images = attachments
            .into_iter()
            .map(|(path, placeholder)| AttachedImage { placeholder, path })
            .collect();
        self.clear_selection();
    }

    /// Clear input
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
        self.history_index = None;
        self.scroll_offset = 0;
        self.pending_pastes.clear();
        self.attached_images.clear();
        self.clear_selection();
    }

    /// Add text to history without submitting.
    /// Expands pending pastes and removes image placeholders to match submit() behavior.
    pub fn add_to_history(&mut self, text: &str) {
        // Expand any pending large pastes
        let mut expanded = text.to_string();
        for (placeholder, actual) in &self.pending_pastes {
            if expanded.contains(placeholder) {
                expanded = expanded.replace(placeholder, actual);
            }
        }

        // Remove image placeholders since the images won't be submitted
        for img in &self.attached_images {
            expanded = expanded.replace(&img.placeholder, "");
        }

        // Only add non-whitespace entries (matches submit() behavior)
        // Use trim_end() to preserve leading whitespace while removing trailing noise
        if !expanded.trim().is_empty() {
            self.push_history(expanded.trim_end().to_string());
        }
    }

    /// Submit input and add to history
    pub fn submit(&mut self) -> InputSubmit {
        let input = std::mem::take(&mut self.input);
        self.cursor_pos = 0;
        self.history_index = None;
        self.scroll_offset = 0;
        self.clear_selection();

        let mut expanded = input;
        for (placeholder, actual) in &self.pending_pastes {
            if expanded.contains(placeholder) {
                expanded = expanded.replace(placeholder, actual);
            }
        }
        self.pending_pastes.clear();

        let (image_paths, image_placeholders) = self.take_attached_images(&expanded);

        // Add to history with trim_end() to preserve leading whitespace
        if !expanded.trim().is_empty() {
            self.push_history(expanded.trim_end().to_string());
        }

        InputSubmit {
            text: expanded,
            image_paths,
            image_placeholders,
        }
    }

    /// Count the number of logical lines in the input
    pub fn line_count(&self) -> usize {
        self.input.split('\n').count().max(1)
    }

    /// Calculate the desired height for the input box (content + padding)
    pub fn desired_height(&self, max_height: u16, width: u16) -> u16 {
        let content_lines = self.visual_line_count(width) as u16;
        // +2 for top and bottom padding
        let desired = content_lines + 2;
        // Minimum of 3 (1 line + padding), maximum of max_height
        desired.clamp(3, max_height)
    }

    /// Ensure cursor line is visible, adjusting scroll if needed
    fn ensure_cursor_visible(&mut self, cursor_line: usize, visible_lines: usize) {
        if cursor_line < self.scroll_offset {
            self.scroll_offset = cursor_line;
        } else if cursor_line >= self.scroll_offset + visible_lines {
            self.scroll_offset = cursor_line.saturating_sub(visible_lines - 1);
        }
    }

    fn push_history(&mut self, entry: String) {
        self.history.push(entry);
        if self.history.len() > HISTORY_MAX {
            let overflow = self.history.len() - HISTORY_MAX;
            self.history.drain(0..overflow);
        }
    }

    /// Get the line number where the cursor is (0-indexed)
    fn cursor_line(&self) -> usize {
        self.input[..self.cursor_pos].matches('\n').count()
    }

    fn visual_line_count(&self, width: u16) -> usize {
        self.build_visual_lines(width).len().max(1)
    }

    fn build_visual_lines(&self, width: u16) -> Vec<VisualLine> {
        if width == 0 {
            return vec![VisualLine {
                start: 0,
                end: 0,
                prefix_width: 0,
            }];
        }

        let mut visual = Vec::new();
        let mut base_offset = 0usize;
        let content_width = width as usize;

        for (line_idx, line) in self.input.split('\n').enumerate() {
            let _is_first_line = line_idx == 0;
            let first_prefix = "  ";
            let cont_prefix = "  ";
            let prefix_width = UnicodeWidthStr::width(first_prefix);
            let wrap_width = content_width.saturating_sub(prefix_width);
            let segments = wrap_line_segments(line, wrap_width);

            for (seg_idx, (start, end)) in segments.into_iter().enumerate() {
                let prefix = if seg_idx == 0 {
                    first_prefix
                } else {
                    cont_prefix
                };
                let prefix_width = UnicodeWidthStr::width(prefix) as u16;
                visual.push(VisualLine {
                    start: base_offset + start,
                    end: base_offset + end,
                    prefix_width,
                });
            }

            base_offset += line.len() + 1; // +1 for newline
        }

        if visual.is_empty() {
            visual.push(VisualLine {
                start: 0,
                end: 0,
                prefix_width: 0,
            });
        }

        visual
    }

    fn cursor_visual_index(&self, lines: &[VisualLine]) -> usize {
        if lines.is_empty() {
            return 0;
        }

        for (i, line) in lines.iter().enumerate() {
            if self.cursor_pos >= line.start && self.cursor_pos <= line.end {
                return i;
            }
        }

        lines.len().saturating_sub(1)
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let (anchor, head) = self.selection_anchor.zip(self.selection_head)?;
        if anchor == head {
            return None;
        }
        Some((anchor.min(head), anchor.max(head)))
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_head = None;
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        self.input.replace_range(start..end, "");
        self.cursor_pos = start.min(self.input.len());
        self.clear_selection();
        true
    }

    fn collapse_selection_to_start(&mut self) -> bool {
        let Some((start, _)) = self.selection_range() else {
            return false;
        };
        self.cursor_pos = start.min(self.input.len());
        self.clear_selection();
        true
    }

    fn collapse_selection_to_end(&mut self) -> bool {
        let Some((_, end)) = self.selection_range() else {
            return false;
        };
        self.cursor_pos = end.min(self.input.len());
        self.clear_selection();
        true
    }

    /// Insert character at cursor
    pub fn insert_char(&mut self, c: char) {
        self.delete_selection();
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    /// Insert a string at cursor
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.delete_selection();
        self.input.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
    }

    /// Insert newline
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Delete character before cursor
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor_pos > 0 {
            // Find the previous character boundary
            let prev_pos = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.remove(prev_pos);
            self.cursor_pos = prev_pos;
        }
    }

    /// Delete character at cursor
    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor_pos < self.input.len() {
            self.input.remove(self.cursor_pos);
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        if self.collapse_selection_to_start() {
            return;
        }
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        if self.collapse_selection_to_end() {
            return;
        }
        if self.cursor_pos < self.input.len() {
            self.cursor_pos = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
        }
    }

    /// Move cursor up one line. Returns true if moved, false if already at top.
    pub fn move_up(&mut self) -> bool {
        if self.collapse_selection_to_start() {
            return true;
        }
        let lines: Vec<&str> = self.input.split('\n').collect();
        if lines.len() <= 1 {
            return false; // Single line, can't move up
        }

        // Find current line and column
        let (current_line, current_col) = self.cursor_line_and_column();

        if current_line == 0 {
            return false; // Already at first line
        }

        // Move to previous line, same column (or end if shorter)
        let target_line = current_line - 1;
        let target_line_len = lines[target_line].len();
        let target_col = current_col.min(target_line_len);

        // Calculate new cursor position
        let mut new_pos = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == target_line {
                new_pos += target_col;
                break;
            }
            new_pos += line.len() + 1; // +1 for newline
        }

        self.cursor_pos = new_pos;
        true
    }

    /// Move cursor down one line. Returns true if moved, false if already at bottom.
    pub fn move_down(&mut self) -> bool {
        if self.collapse_selection_to_end() {
            return true;
        }
        let lines: Vec<&str> = self.input.split('\n').collect();
        if lines.len() <= 1 {
            return false; // Single line, can't move down
        }

        // Find current line and column
        let (current_line, current_col) = self.cursor_line_and_column();

        if current_line >= lines.len() - 1 {
            return false; // Already at last line
        }

        // Move to next line, same column (or end if shorter)
        let target_line = current_line + 1;
        let target_line_len = lines[target_line].len();
        let target_col = current_col.min(target_line_len);

        // Calculate new cursor position
        let mut new_pos = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == target_line {
                new_pos += target_col;
                break;
            }
            new_pos += line.len() + 1; // +1 for newline
        }

        self.cursor_pos = new_pos;
        true
    }

    /// Get the current line number and column (both 0-indexed)
    fn cursor_line_and_column(&self) -> (usize, usize) {
        let before_cursor = &self.input[..self.cursor_pos];
        let lines: Vec<&str> = before_cursor.split('\n').collect();
        let line = lines.len() - 1;
        let col = lines.last().map(|l| l.len()).unwrap_or(0);
        (line, col)
    }

    /// Check if cursor is on the first line
    pub fn is_cursor_on_first_line(&self) -> bool {
        self.cursor_line() == 0
    }

    /// Check if cursor is on the last line
    pub fn is_cursor_on_last_line(&self) -> bool {
        let total_lines = self.line_count();
        self.cursor_line() >= total_lines - 1
    }

    /// Move cursor to start
    pub fn move_start(&mut self) {
        if self.collapse_selection_to_start() {
            return;
        }
        if let Some(width) = self.last_content_width {
            let visual_lines = self.build_visual_lines(width);
            let line_idx = self.cursor_visual_index(&visual_lines);
            if let Some(line) = visual_lines.get(line_idx) {
                self.cursor_pos = line.start.min(self.input.len());
                return;
            }
        }

        // Fallback: start of current logical line
        if let Some(prev_newline) = self.input[..self.cursor_pos].rfind('\n') {
            self.cursor_pos = prev_newline + 1;
        } else {
            self.cursor_pos = 0;
        }
    }

    /// Move cursor to end
    pub fn move_end(&mut self) {
        if self.collapse_selection_to_end() {
            return;
        }
        self.cursor_pos = self.input.len();
    }

    /// Move cursor left by one word
    pub fn move_word_left(&mut self) {
        if self.collapse_selection_to_start() {
            return;
        }
        if self.cursor_pos == 0 {
            return;
        }

        let before_cursor = &self.input[..self.cursor_pos];

        // Skip any trailing whitespace
        let trimmed = before_cursor.trim_end();
        if trimmed.is_empty() {
            self.cursor_pos = 0;
            return;
        }

        // Find the start of the current word
        let word_start = trimmed
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);

        self.cursor_pos = word_start;
    }

    /// Move cursor right by one word
    pub fn move_word_right(&mut self) {
        if self.collapse_selection_to_end() {
            return;
        }
        if self.cursor_pos >= self.input.len() {
            return;
        }

        let after_cursor = &self.input[self.cursor_pos..];

        // Skip current word characters
        let skip_word = after_cursor
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after_cursor.len());

        // Skip whitespace after word
        let remaining = &after_cursor[skip_word..];
        let skip_space = remaining
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(remaining.len());

        self.cursor_pos += skip_word + skip_space;
    }

    /// Delete word before cursor (Ctrl+W)
    pub fn delete_word_back(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor_pos == 0 {
            return;
        }

        let before_cursor = &self.input[..self.cursor_pos];

        // Skip trailing whitespace
        let trimmed_len = before_cursor.trim_end().len();

        if trimmed_len == 0 {
            // Only whitespace before cursor
            self.input = self.input[self.cursor_pos..].to_string();
            self.cursor_pos = 0;
            return;
        }

        // Find word boundary
        let word_start = before_cursor[..trimmed_len]
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);

        let new_before = &self.input[..word_start];
        let after = &self.input[self.cursor_pos..];
        self.input = format!("{}{}", new_before, after);
        self.cursor_pos = word_start;
    }

    /// Delete from cursor to start of line (Ctrl+U)
    pub fn delete_to_start(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor_pos == 0 {
            return;
        }
        self.input = self.input[self.cursor_pos..].to_string();
        self.cursor_pos = 0;
    }

    /// Delete from cursor to end of line (Ctrl+K)
    pub fn delete_to_end(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.input.truncate(self.cursor_pos);
    }

    /// Navigate to previous history entry
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                self.saved_input = std::mem::take(&mut self.input);
                self.history_index = Some(self.history.len() - 1);
            }
            Some(0) => {
                // Already at oldest, do nothing
                return;
            }
            Some(i) => {
                self.history_index = Some(i - 1);
            }
        }

        if let Some(i) = self.history_index {
            self.input = self.history[i].clone();
            self.cursor_pos = self.input.len();
            self.clear_selection();
        }
    }

    /// Navigate to next history entry
    pub fn history_next(&mut self) {
        match self.history_index {
            None => {
                // Not in history mode
                return;
            }
            Some(i) if i >= self.history.len() - 1 => {
                // Return to current input
                self.history_index = None;
                self.input = std::mem::take(&mut self.saved_input);
            }
            Some(i) => {
                self.history_index = Some(i + 1);
                self.input = self.history[i + 1].clone();
            }
        }
        self.cursor_pos = self.input.len();
        self.clear_selection();
    }

    /// Set focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Check if input is empty
    pub fn is_empty(&self) -> bool {
        self.input.trim().is_empty()
    }

    pub fn handle_paste(&mut self, pasted: String) {
        let char_count = pasted.chars().count();
        if char_count > LARGE_PASTE_CHAR_THRESHOLD {
            let placeholder = self.next_large_paste_placeholder(char_count);
            self.insert_str(&placeholder);
            self.pending_pastes.push((placeholder, pasted));
        } else if char_count > 1 && self.handle_paste_image_path(&pasted) {
            self.insert_str(" ");
        } else {
            self.insert_str(&pasted);
        }
    }

    pub fn attach_image(&mut self, path: PathBuf, width: u32, height: u32) {
        let file_label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        let base_placeholder = format!("{file_label} {width}x{height}");
        let placeholder = self.next_image_placeholder(&base_placeholder);
        self.insert_str(&placeholder);
        self.attached_images
            .push(AttachedImage { placeholder, path });
    }

    fn take_attached_images(&mut self, text: &str) -> (Vec<PathBuf>, Vec<String>) {
        let mut images = Vec::new();
        let mut placeholders = Vec::new();
        for img in self.attached_images.drain(..) {
            if text.contains(&img.placeholder) {
                images.push(img.path);
                placeholders.push(img.placeholder);
            }
        }
        (images, placeholders)
    }

    fn handle_paste_image_path(&mut self, pasted: &str) -> bool {
        let Some(path_buf) = normalize_pasted_path(pasted) else {
            return false;
        };

        match image::image_dimensions(&path_buf) {
            Ok((w, h)) => {
                self.attach_image(path_buf, w, h);
                true
            }
            Err(_) => false,
        }
    }

    fn next_large_paste_placeholder(&mut self, char_count: usize) -> String {
        let base = format!("[Pasted Content {char_count} chars]");
        let next_suffix = self.large_paste_counters.entry(char_count).or_insert(0);
        *next_suffix += 1;
        if *next_suffix == 1 {
            base
        } else {
            format!("{base} #{next_suffix}")
        }
    }

    fn next_image_placeholder(&mut self, base: &str) -> String {
        let text = &self.input;
        let mut suffix = 1;
        loop {
            let placeholder = if suffix == 1 {
                format!("[{base}]")
            } else {
                format!("[{base} #{suffix}]")
            };
            if !text.contains(&placeholder) {
                return placeholder;
            }
            suffix += 1;
        }
    }

    /// Get cursor position for rendering, accounting for scroll offset
    pub fn cursor_position(&self, area: Rect, scroll_offset: usize) -> (u16, u16) {
        if area.height < 3 || area.width == 0 {
            return (area.x, area.y);
        }

        let padding_top: u16 = 1;
        let padding_bottom: u16 = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return (area.x, area.y + padding_top);
        }

        let visible_lines = content_height as usize;
        let visual_lines_full = self.build_visual_lines(area.width);
        let show_scrollbar = visual_lines_full.len() > visible_lines;
        let content_width = area
            .width
            .saturating_sub(if show_scrollbar { 1 } else { 0 });
        if content_width == 0 {
            return (area.x, area.y + padding_top);
        }
        let visual_lines = if show_scrollbar {
            self.build_visual_lines(content_width)
        } else {
            visual_lines_full
        };
        if visual_lines.is_empty() {
            return (area.x, area.y + padding_top);
        }

        let cursor_line = self.cursor_visual_index(&visual_lines);
        let line = &visual_lines[cursor_line];
        let cursor_pos = self.cursor_pos.clamp(line.start, line.end);
        let segment = &self.input[line.start..cursor_pos];
        let segment_width = UnicodeWidthStr::width(segment) as u16;
        let cursor_x = area.x + line.prefix_width + segment_width;
        let max_x = area.x + content_width.saturating_sub(1);
        let visible_y = cursor_line.saturating_sub(scroll_offset);
        (cursor_x.min(max_x), area.y + padding_top + visible_y as u16)
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn set_scroll_offset(&mut self, offset: usize, total: usize, visible: usize) {
        let max_scroll = total.saturating_sub(visible);
        self.scroll_offset = offset.min(max_scroll);
    }

    pub fn scrollbar_metrics(&mut self, area: Rect) -> Option<ScrollbarMetrics> {
        if area.height < 3 || area.width == 0 {
            return None;
        }

        let padding_top = 1;
        let padding_bottom = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return None;
        }

        let visible_lines = content_height as usize;
        let base_width = area.width;
        let visual_lines_full = self.build_visual_lines(base_width);
        let show_scrollbar = visual_lines_full.len() > visible_lines;
        if !show_scrollbar {
            return None;
        }

        let content_width = area.width.saturating_sub(1);
        if content_width == 0 {
            return None;
        }

        let visual_lines = self.build_visual_lines(content_width);
        let total_lines = visual_lines.len();
        if total_lines <= visible_lines {
            return None;
        }

        self.last_content_width = Some(content_width);

        Some(ScrollbarMetrics {
            area: Rect {
                x: area.x + area.width.saturating_sub(1),
                y: area.y + padding_top,
                width: 1,
                height: content_height,
            },
            total: total_lines,
            visible: visible_lines,
        })
    }

    fn cursor_pos_from_point(&self, click_x: u16, click_y: u16, area: Rect) -> Option<usize> {
        if area.height < 3 || area.width < 4 {
            return None;
        }

        let padding_top: u16 = 1;
        let padding_bottom: u16 = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return None;
        }

        let visible_lines = content_height as usize;
        let base_width = area.width;
        let visual_lines_full = self.build_visual_lines(base_width);
        let show_scrollbar = visual_lines_full.len() > visible_lines;
        let content_width = base_width.saturating_sub(if show_scrollbar { 1 } else { 0 });
        if content_width == 0 {
            return None;
        }

        let content_x = area.x;
        let content_y = area.y + padding_top;

        // Check if click is within the content area
        if click_x < content_x
            || click_y < content_y
            || click_x >= content_x + content_width
            || click_y >= content_y + content_height
        {
            return None;
        }

        let relative_x = click_x - content_x;
        let relative_y = (click_y - content_y) as usize;

        let visual_lines = if show_scrollbar {
            self.build_visual_lines(content_width)
        } else {
            visual_lines_full
        };

        let target_line = relative_y + self.scroll_offset;
        if target_line >= visual_lines.len() {
            return None;
        }

        let line = &visual_lines[target_line];
        if relative_x < line.prefix_width {
            return Some(line.start.min(self.input.len()));
        }

        let target_x = (relative_x - line.prefix_width) as usize;
        let segment = &self.input[line.start..line.end];
        let byte_offset = byte_offset_for_x(segment, target_x);
        Some((line.start + byte_offset).min(self.input.len()))
    }

    /// Set cursor position from a mouse click
    pub fn set_cursor_from_click(&mut self, click_x: u16, click_y: u16, area: Rect) {
        if let Some(pos) = self.cursor_pos_from_point(click_x, click_y, area) {
            self.cursor_pos = pos;
            self.clear_selection();
        }
    }

    pub fn begin_selection(&mut self, click_x: u16, click_y: u16, area: Rect) -> bool {
        let Some(pos) = self.cursor_pos_from_point(click_x, click_y, area) else {
            return false;
        };
        self.cursor_pos = pos;
        self.selection_anchor = Some(pos);
        self.selection_head = None;
        true
    }

    pub fn update_selection(&mut self, click_x: u16, click_y: u16, area: Rect) -> bool {
        if self.selection_anchor.is_none() {
            return false;
        }
        let Some(pos) = self.cursor_pos_from_point(click_x, click_y, area) else {
            return false;
        };
        self.cursor_pos = pos;
        self.selection_head = Some(pos);
        if area.height >= 3 && area.width > 0 {
            let padding_top = 1;
            let padding_bottom = 1;
            let content_height = area.height.saturating_sub(padding_top + padding_bottom);
            let visible_lines = content_height as usize;
            let base_width = area.width;
            let visual_lines_full = self.build_visual_lines(base_width);
            let show_scrollbar = visual_lines_full.len() > visible_lines;
            let content_width = base_width.saturating_sub(if show_scrollbar { 1 } else { 0 });
            if content_width > 0 {
                let visual_lines = if show_scrollbar {
                    self.build_visual_lines(content_width)
                } else {
                    visual_lines_full
                };
                let cursor_line = self.cursor_visual_index(&visual_lines);
                self.ensure_cursor_visible(cursor_line, visible_lines.max(1));
            }
        }
        true
    }

    pub fn finalize_selection(&mut self) -> bool {
        let has_selection = self.selection_range().is_some();
        if self.selection_head.is_none() || self.selection_anchor == self.selection_head {
            self.clear_selection();
            return false;
        }
        has_selection
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        Some(self.input[start..end].to_string())
    }

    /// Render the input box
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_bg(input_bg());
            }
        }

        if area.height < 3 || area.width == 0 {
            return;
        }

        let padding_top = 1;
        let padding_bottom = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return;
        }

        let visible_lines = content_height as usize;
        let base_width = area.width;
        let visual_lines_full = self.build_visual_lines(base_width);
        let show_scrollbar = visual_lines_full.len() > visible_lines;
        let content_width = area
            .width
            .saturating_sub(if show_scrollbar { 1 } else { 0 });
        if content_width == 0 {
            return;
        }

        let visual_lines = if show_scrollbar {
            self.build_visual_lines(content_width)
        } else {
            visual_lines_full
        };
        let total_lines = visual_lines.len();
        let cursor_line = self.cursor_visual_index(&visual_lines);

        // Ensure cursor is visible
        self.ensure_cursor_visible(cursor_line, visible_lines);

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(visible_lines);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        let base_style = Style::default().fg(text_primary()).bg(input_bg());
        let selection_style = Style::default().fg(text_primary()).bg(bg_highlight());
        let selection = self.selection_range();

        let mut display_lines: Vec<Line> = Vec::with_capacity(visible_lines);
        for line in visual_lines
            .iter()
            .skip(self.scroll_offset)
            .take(visible_lines)
        {
            let mut spans: Vec<Span> = Vec::new();
            let prefix = " ".repeat(line.prefix_width as usize);
            spans.push(Span::styled(prefix, base_style));

            let segment = &self.input[line.start..line.end];
            if let Some((sel_start, sel_end)) = selection {
                let overlap_start = sel_start.max(line.start);
                let overlap_end = sel_end.min(line.end);
                if overlap_start < overlap_end {
                    let local_start = overlap_start - line.start;
                    let local_end = overlap_end - line.start;
                    let before = &segment[..local_start];
                    let selected = &segment[local_start..local_end];
                    let after = &segment[local_end..];
                    if !before.is_empty() {
                        spans.push(Span::styled(before.to_string(), base_style));
                    }
                    if !selected.is_empty() {
                        spans.push(Span::styled(selected.to_string(), selection_style));
                    }
                    if !after.is_empty() {
                        spans.push(Span::styled(after.to_string(), base_style));
                    }
                } else {
                    spans.push(Span::styled(segment.to_string(), base_style));
                }
            } else {
                spans.push(Span::styled(segment.to_string(), base_style));
            }

            display_lines.push(Line::from(spans));
        }

        let paragraph = Paragraph::new(display_lines).wrap(Wrap { trim: false });

        let content_area = Rect {
            x: area.x,
            y: area.y + padding_top,
            width: content_width,
            height: content_height,
        };

        paragraph.render(content_area, buf);

        // Render scrollbar
        render_minimal_scrollbar(
            Rect {
                x: area.x + area.width.saturating_sub(1),
                y: area.y + padding_top,
                width: 1,
                height: content_height,
            },
            buf,
            total_lines,
            visible_lines,
            self.scroll_offset,
        );
    }
}

fn wrap_line_segments(line: &str, max_width: usize) -> Vec<(usize, usize)> {
    if line.is_empty() {
        return vec![(0, 0)];
    }
    if max_width == 0 {
        return vec![(0, 0)];
    }

    #[derive(Clone, Copy)]
    struct CharInfo {
        ch: char,
        width: usize,
        byte_idx: usize,
        byte_len: usize,
    }

    let mut segments = Vec::new();
    let mut current: Vec<CharInfo> = Vec::new();
    let mut line_width = 0usize;
    let mut last_break: Option<(usize, usize)> = None; // (index in current, width at break)

    let flush_segment =
        |current: &mut Vec<CharInfo>, split_idx: usize, segments: &mut Vec<(usize, usize)>| {
            if split_idx == 0 || current.is_empty() {
                return;
            }
            let seg_end_idx = split_idx.saturating_sub(1);
            let seg_start = current[0].byte_idx;
            let seg_end = current[seg_end_idx].byte_idx + current[seg_end_idx].byte_len;
            segments.push((seg_start, seg_end));

            let mut remainder = current.split_off(split_idx);
            while !remainder.is_empty() && remainder[0].ch.is_whitespace() {
                remainder.remove(0);
            }
            *current = remainder;
        };

    let recompute_state = |current: &Vec<CharInfo>| -> (usize, Option<(usize, usize)>) {
        let mut width = 0usize;
        let mut last = None;
        for (i, info) in current.iter().enumerate() {
            width += info.width;
            if info.ch.is_whitespace() {
                last = Some((i + 1, width));
            }
        }
        (width, last)
    };

    for (byte_idx, ch) in line.char_indices() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        current.push(CharInfo {
            ch,
            width,
            byte_idx,
            byte_len: ch.len_utf8(),
        });
        line_width += width;

        if ch.is_whitespace() {
            last_break = Some((current.len(), line_width));
        }

        if line_width > max_width && !current.is_empty() {
            if let Some((break_idx, _)) = last_break {
                if break_idx > 0 {
                    flush_segment(&mut current, break_idx, &mut segments);
                    let (w, lb) = recompute_state(&current);
                    line_width = w;
                    last_break = lb;
                    continue;
                }
            }

            let split_idx = if current.len() > 1 {
                current.len() - 1
            } else {
                1
            };
            flush_segment(&mut current, split_idx, &mut segments);
            let (w, lb) = recompute_state(&current);
            line_width = w;
            last_break = lb;
        }
    }

    if !current.is_empty() {
        let seg_start = current[0].byte_idx;
        let last = current.last().unwrap();
        let seg_end = last.byte_idx + last.byte_len;
        segments.push((seg_start, seg_end));
    }

    segments
}

fn byte_offset_for_x(text: &str, target_x: usize) -> usize {
    let mut visual_x = 0usize;
    let mut byte_offset = 0usize;

    for (idx, ch) in text.char_indices() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if visual_x >= target_x {
            byte_offset = idx;
            return byte_offset;
        }
        visual_x += ch_width;
        byte_offset = idx + ch.len_utf8();
    }

    if visual_x < target_x {
        text.len()
    } else {
        byte_offset
    }
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}
