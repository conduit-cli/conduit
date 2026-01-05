use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{
        Block, Borders, Paragraph, Widget, Wrap,
    },
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{render_vertical_scrollbar, ScrollbarSymbols};
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
        }
    }

    /// Get current input text
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Set input text
    pub fn set_input(&mut self, text: String) {
        self.input = text;
        self.cursor_pos = self.input.len();
    }

    /// Clear input
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
        self.history_index = None;
        self.scroll_offset = 0;
    }

    /// Submit input and add to history
    pub fn submit(&mut self) -> String {
        let input = std::mem::take(&mut self.input);
        self.cursor_pos = 0;
        self.history_index = None;
        self.scroll_offset = 0;

        if !input.trim().is_empty() {
            self.history.push(input.clone());
        }

        input
    }

    /// Count the number of lines in the input
    pub fn line_count(&self) -> usize {
        self.input.split('\n').count().max(1)
    }

    /// Calculate the desired height for the input box (content + borders)
    pub fn desired_height(&self, max_height: u16) -> u16 {
        let content_lines = self.line_count() as u16;
        // +2 for top and bottom borders
        let desired = content_lines + 2;
        // Minimum of 3 (1 line + borders), maximum of max_height
        desired.clamp(3, max_height)
    }

    /// Ensure cursor line is visible, adjusting scroll if needed
    fn ensure_cursor_visible(&mut self, visible_lines: usize) {
        let cursor_line = self.cursor_line();

        // If cursor is above visible area, scroll up
        if cursor_line < self.scroll_offset {
            self.scroll_offset = cursor_line;
        }
        // If cursor is below visible area, scroll down
        else if cursor_line >= self.scroll_offset + visible_lines {
            self.scroll_offset = cursor_line.saturating_sub(visible_lines - 1);
        }
    }

    /// Get the line number where the cursor is (0-indexed)
    fn cursor_line(&self) -> usize {
        self.input[..self.cursor_pos].matches('\n').count()
    }

    /// Insert character at cursor
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    /// Insert newline
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Delete character before cursor
    pub fn backspace(&mut self) {
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
        if self.cursor_pos < self.input.len() {
            self.input.remove(self.cursor_pos);
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
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
        self.cursor_pos = 0;
    }

    /// Move cursor to end
    pub fn move_end(&mut self) {
        self.cursor_pos = self.input.len();
    }

    /// Move cursor left by one word
    pub fn move_word_left(&mut self) {
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
        if self.cursor_pos == 0 {
            return;
        }
        self.input = self.input[self.cursor_pos..].to_string();
        self.cursor_pos = 0;
    }

    /// Delete from cursor to end of line (Ctrl+K)
    pub fn delete_to_end(&mut self) {
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
    }

    /// Set focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Check if input is empty
    pub fn is_empty(&self) -> bool {
        self.input.trim().is_empty()
    }

    /// Get cursor position for rendering, accounting for scroll offset
    pub fn cursor_position(&self, area: Rect, scroll_offset: usize) -> (u16, u16) {
        // Calculate cursor position accounting for multi-line
        let text_before_cursor = &self.input[..self.cursor_pos];
        let lines: Vec<&str> = text_before_cursor.split('\n').collect();

        let absolute_y = lines.len() - 1;
        let visible_y = absolute_y.saturating_sub(scroll_offset);
        let x = lines.last().map(|l| l.width() as u16).unwrap_or(0);

        // Account for border; prompt "> " only on first line (when not scrolled)
        let prompt_offset = if absolute_y == 0 && scroll_offset == 0 { 2 } else { 0 };
        (area.x + 1 + prompt_offset + x, area.y + 1 + visible_y as u16)
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Set cursor position from a mouse click
    pub fn set_cursor_from_click(&mut self, click_x: u16, click_y: u16, area: Rect) {
        // Calculate position relative to inner area (accounting for border)
        let inner_x = area.x + 1;
        let inner_y = area.y + 1;

        // Check if click is within the inner area
        if click_x < inner_x || click_y < inner_y {
            return;
        }

        let relative_x = (click_x - inner_x) as usize;
        let relative_y = (click_y - inner_y) as usize;

        // Calculate which line was clicked (accounting for scroll)
        let target_line = relative_y + self.scroll_offset;

        let lines: Vec<&str> = self.input.split('\n').collect();
        let total_lines = lines.len();

        // Clamp to valid line range
        let target_line = target_line.min(total_lines.saturating_sub(1));

        // Account for prompt "> " on the first visible line when not scrolled
        let effective_x = if target_line == 0 {
            // First line has "> " prefix (2 chars)
            relative_x.saturating_sub(2)
        } else {
            relative_x
        };

        // Find byte offset for the target position
        let line = lines.get(target_line).unwrap_or(&"");

        // Convert visual x position to byte offset within the line
        // We need to account for Unicode character widths
        let mut byte_offset_in_line = 0;
        let mut visual_x = 0;

        for (idx, ch) in line.char_indices() {
            if visual_x >= effective_x {
                byte_offset_in_line = idx;
                break;
            }
            visual_x += ch.width().unwrap_or(1);
            byte_offset_in_line = idx + ch.len_utf8();
        }

        // If we went past all characters, set to end of line
        if visual_x < effective_x {
            byte_offset_in_line = line.len();
        }

        // Calculate absolute byte offset
        let mut cursor_pos = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == target_line {
                cursor_pos += byte_offset_in_line;
                break;
            }
            cursor_pos += line.len() + 1; // +1 for newline
        }

        // Clamp to valid range
        self.cursor_pos = cursor_pos.min(self.input.len());
    }

    /// Render the input box
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.focused {
                Color::Cyan
            } else {
                Color::DarkGray
            }))
            .title(" Input ");

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let visible_lines = inner.height as usize;
        let total_lines = self.line_count();

        // Ensure cursor is visible
        self.ensure_cursor_visible(visible_lines);

        // Clamp scroll offset
        let max_scroll = total_lines.saturating_sub(visible_lines);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Build lines with prompt on first line
        let lines: Vec<&str> = self.input.split('\n').collect();
        let mut display_lines: Vec<String> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            if i == 0 {
                display_lines.push(format!("> {}", line));
            } else {
                display_lines.push(line.to_string());
            }
        }

        // Apply scroll offset
        let visible_display: Vec<String> = display_lines
            .into_iter()
            .skip(self.scroll_offset)
            .take(visible_lines)
            .collect();

        let display_text = visible_display.join("\n");

        let paragraph = Paragraph::new(display_text)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false });

        paragraph.render(inner, buf);

        // Render scrollbar if content exceeds visible area
        render_vertical_scrollbar(
            Rect {
                x: inner.x + inner.width,
                y: inner.y,
                width: 1,
                height: inner.height,
            },
            buf,
            total_lines,
            visible_lines,
            self.scroll_offset,
            ScrollbarSymbols::arrows(),
        );
    }
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}
