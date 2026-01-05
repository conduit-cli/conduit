use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{
        Clear, Paragraph, Widget, Wrap,
    },
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{render_vertical_scrollbar, ScrollbarSymbols, INPUT_BG};

#[derive(Debug, Clone)]
struct VisualLine {
    text: String,
    start: usize,
    end: usize,
    prefix_width: u16,
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
                text: String::new(),
                start: 0,
                end: 0,
                prefix_width: 0,
            }];
        }

        let mut visual = Vec::new();
        let mut base_offset = 0usize;
        let content_width = width as usize;

        for (line_idx, line) in self.input.split('\n').enumerate() {
            let is_first_line = line_idx == 0;
            let first_prefix = if is_first_line { "> " } else { "  " };
            let cont_prefix = "  ";
            let prefix_width = UnicodeWidthStr::width(first_prefix);
            let wrap_width = content_width.saturating_sub(prefix_width);
            let segments = wrap_line_segments(line, wrap_width);

            for (seg_idx, (start, end)) in segments.into_iter().enumerate() {
                let prefix = if seg_idx == 0 { first_prefix } else { cont_prefix };
                let prefix_width = UnicodeWidthStr::width(prefix) as u16;
                let segment_text = if start <= end && end <= line.len() {
                    &line[start..end]
                } else {
                    ""
                };
                visual.push(VisualLine {
                    text: format!("{}{}", prefix, segment_text),
                    start: base_offset + start,
                    end: base_offset + end,
                    prefix_width,
                });
            }

            base_offset += line.len() + 1; // +1 for newline
        }

        if visual.is_empty() {
            visual.push(VisualLine {
                text: String::new(),
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
        let content_width = area.width.saturating_sub(if show_scrollbar { 1 } else { 0 });
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
        (
            cursor_x.min(max_x),
            area.y + padding_top + visible_y as u16,
        )
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Set cursor position from a mouse click
    pub fn set_cursor_from_click(&mut self, click_x: u16, click_y: u16, area: Rect) {
        if area.height < 3 || area.width == 0 {
            return;
        }

        let padding_top: u16 = 1;
        let padding_bottom: u16 = 1;
        let content_height = area.height.saturating_sub(padding_top + padding_bottom);
        if content_height == 0 {
            return;
        }

        let visible_lines = content_height as usize;
        let visual_lines_full = self.build_visual_lines(area.width);
        let show_scrollbar = visual_lines_full.len() > visible_lines;
        let content_width = area.width.saturating_sub(if show_scrollbar { 1 } else { 0 });
        if content_width == 0 {
            return;
        }

        let content_x = area.x;
        let content_y = area.y + padding_top;

        // Check if click is within the content area
        if click_x < content_x
            || click_y < content_y
            || click_x >= content_x + content_width
            || click_y >= content_y + content_height
        {
            return;
        }

        let relative_x = (click_x - content_x) as u16;
        let relative_y = (click_y - content_y) as usize;

        let visual_lines = if show_scrollbar {
            self.build_visual_lines(content_width)
        } else {
            visual_lines_full
        };

        let target_line = relative_y + self.scroll_offset;
        if target_line >= visual_lines.len() {
            return;
        }

        let line = &visual_lines[target_line];
        if relative_x < line.prefix_width {
            self.cursor_pos = line.start.min(self.input.len());
            return;
        }

        let target_x = (relative_x - line.prefix_width) as usize;
        let segment = &self.input[line.start..line.end];
        let byte_offset = byte_offset_for_x(segment, target_x);
        self.cursor_pos = (line.start + byte_offset).min(self.input.len());
    }

    /// Render the input box
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_bg(INPUT_BG);
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
        let content_width = area.width.saturating_sub(if show_scrollbar { 1 } else { 0 });
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

        let display_text = visual_lines
            .iter()
            .skip(self.scroll_offset)
            .take(visible_lines)
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let paragraph = Paragraph::new(display_text)
            .style(Style::default().fg(Color::White).bg(INPUT_BG))
            .wrap(Wrap { trim: false });

        let content_area = Rect {
            x: area.x,
            y: area.y + padding_top,
            width: content_width,
            height: content_height,
        };

        paragraph.render(content_area, buf);

        // Render scrollbar if content exceeds visible area
        if total_lines > visible_lines {
            render_vertical_scrollbar(
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
                ScrollbarSymbols::arrows(),
            );
        }
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

    let flush_segment = |current: &mut Vec<CharInfo>, split_idx: usize, segments: &mut Vec<(usize, usize)>| {
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

            let split_idx = if current.len() > 1 { current.len() - 1 } else { 1 };
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
