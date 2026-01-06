//! Types for the raw events view.

use std::time::Instant;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;

/// State for the event detail panel
#[derive(Debug, Clone, Default)]
pub struct EventDetailState {
    /// Whether the detail panel is visible
    pub visible: bool,
    /// Index of the event being viewed
    pub event_index: usize,
    /// Scroll offset within the detail content
    pub scroll_offset: usize,
}

impl EventDetailState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the detail panel for a specific event
    pub fn open(&mut self, event_index: usize) {
        self.visible = true;
        self.event_index = event_index;
        self.scroll_offset = 0;
    }

    /// Close the detail panel
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Toggle the detail panel visibility
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.scroll_offset = 0;
    }

    /// Sync to a new event index (resets scroll)
    pub fn sync_to_event(&mut self, event_index: usize) {
        if self.event_index != event_index {
            self.event_index = event_index;
            self.scroll_offset = 0;
        }
    }

    /// Scroll up by n lines
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines (clamped to max)
    pub fn scroll_down(&mut self, n: usize, content_height: usize, visible_height: usize) {
        let max_scroll = content_height.saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
    }

    /// Scroll up by a page
    pub fn page_up(&mut self, visible_height: usize) {
        self.scroll_up(visible_height.saturating_sub(2));
    }

    /// Scroll down by a page
    pub fn page_down(&mut self, visible_height: usize, content_height: usize) {
        self.scroll_down(visible_height.saturating_sub(2), content_height, visible_height);
    }

    /// Jump to top
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Jump to bottom
    pub fn scroll_to_bottom(&mut self, content_height: usize, visible_height: usize) {
        self.scroll_offset = content_height.saturating_sub(visible_height);
    }
}

/// Minimum width for split layout (below this, use overlay)
pub const DETAIL_PANEL_BREAKPOINT: u16 = 100;

/// Direction of the event (sent to agent or received from agent)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDirection {
    Sent,
    Received,
}

impl EventDirection {
    pub fn symbol(&self) -> &'static str {
        match self {
            EventDirection::Sent => "→",
            EventDirection::Received => "←",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            EventDirection::Sent => Color::Green,
            EventDirection::Received => Color::Cyan,
        }
    }
}

/// A single raw event entry
#[derive(Debug, Clone)]
pub struct RawEventEntry {
    /// When the event occurred (relative to session start)
    pub timestamp: Instant,
    /// Direction of the event
    pub direction: EventDirection,
    /// Event type name
    pub event_type: String,
    /// Raw JSON value
    pub raw_json: Value,
    /// Session start time (for relative timestamp display)
    pub session_start: Instant,
}

impl RawEventEntry {
    pub fn new(
        direction: EventDirection,
        event_type: impl Into<String>,
        raw_json: Value,
        session_start: Instant,
    ) -> Self {
        Self {
            timestamp: Instant::now(),
            direction,
            event_type: event_type.into(),
            raw_json,
            session_start,
        }
    }

    /// Format timestamp as MM:SS.mmm relative to session start
    pub fn format_timestamp(&self) -> String {
        let elapsed = self.timestamp.duration_since(self.session_start);
        let total_secs = elapsed.as_secs();
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        let millis = elapsed.subsec_millis();
        format!("{:02}:{:02}.{:03}", mins, secs, millis)
    }

    /// Render as compact single line with optional prefix
    pub fn render_compact(&self, prefix: &str, style: Style) -> Line<'static> {
        let timestamp = self.format_timestamp();
        let summary = self.compact_summary();

        Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("[{}] ", timestamp),
                style.fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{} ", self.direction.symbol()),
                style.fg(self.direction.color()),
            ),
            Span::styled(
                format!("{}: ", self.event_type),
                style
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(summary, style.fg(Color::White)),
        ])
    }

    /// Generate a compact one-line summary of the event
    fn compact_summary(&self) -> String {
        match &self.raw_json {
            Value::Object(map) => {
                // Try to extract meaningful fields
                let mut parts = Vec::new();

                // Common fields to show
                for key in &[
                    "prompt",
                    "text",
                    "message",
                    "content",
                    "error",
                    "result",
                    "command",
                    "file_path",
                    "tool_name",
                    "session_id",
                ] {
                    if let Some(val) = map.get(*key) {
                        let val_str = match val {
                            Value::String(s) => {
                                if s.chars().count() > 50 {
                                    let truncated: String = s.chars().take(47).collect();
                                    format!("\"{}...\"", truncated)
                                } else {
                                    format!("\"{}\"", s)
                                }
                            }
                            Value::Bool(b) => b.to_string(),
                            Value::Number(n) => n.to_string(),
                            _ => continue,
                        };
                        parts.push(format!("{}={}", key, val_str));
                        if parts.len() >= 3 {
                            break;
                        }
                    }
                }

                if parts.is_empty() {
                    // Fallback: show first few keys
                    let keys: Vec<_> = map.keys().take(3).map(|k| k.as_str()).collect();
                    format!("{{{}}}", keys.join(", "))
                } else {
                    parts.join(", ")
                }
            }
            Value::String(s) => {
                if s.chars().count() > 60 {
                    let truncated: String = s.chars().take(57).collect();
                    format!("\"{}...\"", truncated)
                } else {
                    format!("\"{}\"", s)
                }
            }
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => "(data)".to_string(),
        }
    }

    /// Render JSON as highlighted lines, optionally truncating to max_lines.
    pub fn render_json_lines(&self, max_lines: usize) -> Vec<Line<'static>> {
        let Ok(pretty) = serde_json::to_string_pretty(&self.raw_json) else {
            return vec![Line::from(Span::styled(
                "(invalid json)",
                Style::default().fg(Color::Red),
            ))];
        };

        let mut lines: Vec<Line<'static>> = pretty
            .lines()
            .map(|line| Line::from(Self::highlight_json_line(line)))
            .collect();

        if max_lines > 0 && lines.len() > max_lines {
            let remaining = lines.len().saturating_sub(max_lines);
            lines.truncate(max_lines);
            lines.push(Line::from(Span::styled(
                format!("… {} more lines", remaining),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines
    }

    /// Highlight a single line of JSON for display.
    pub fn highlight_json_line(line: &str) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let mut pos = 0;
        let bytes = line.as_bytes();
        let mut in_string = false;

        while pos < line.len() {
            if !in_string {
                // Find next quote
                let mut next_quote = None;
                let mut idx = pos;
                while idx < line.len() {
                    if bytes[idx] == b'"' && !Self::is_escaped(&line[pos..idx]) {
                        next_quote = Some(idx);
                        break;
                    }
                    idx += 1;
                }

                if let Some(q) = next_quote {
                    if q > pos {
                        spans.extend(Self::highlight_non_string(&line[pos..q]));
                    }
                    in_string = true;
                    pos = q;
                } else {
                    spans.extend(Self::highlight_non_string(&line[pos..]));
                    break;
                }
            } else {
                // We are at the opening quote
                let start = pos;
                let mut end = pos + 1;
                while end < line.len() {
                    if bytes[end] == b'"' && !Self::is_escaped(&line[start..end]) {
                        end += 1;
                        break;
                    }
                    end += 1;
                }

                let token = &line[start..end.min(line.len())];
                let mut is_key = false;
                // Look ahead for ':' to determine key vs value.
                let mut look = end;
                while look < line.len() && line.as_bytes()[look].is_ascii_whitespace() {
                    look += 1;
                }
                if look < line.len() && line.as_bytes()[look] == b':' {
                    is_key = true;
                }

                let style = if is_key {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Green)
                };
                spans.push(Span::styled(token.to_string(), style));

                pos = end;
                in_string = false;
            }
        }

        spans
    }

    fn highlight_non_string(segment: &str) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let mut current = String::new();

        for ch in segment.chars() {
            if ch.is_alphanumeric() || ch == '.' || ch == '-' {
                current.push(ch);
                continue;
            }

            if !current.is_empty() {
                let style = Self::get_value_style(&current);
                spans.push(Span::styled(std::mem::take(&mut current), style));
            }

            let style = match ch {
                '{' | '}' | '[' | ']' | ',' | ':' => Style::default().fg(Color::DarkGray),
                _ if ch.is_whitespace() => Style::default(),
                _ => Style::default().fg(Color::White),
            };
            spans.push(Span::styled(ch.to_string(), style));
        }

        if !current.is_empty() {
            let style = Self::get_value_style(&current);
            spans.push(Span::styled(current, style));
        }

        spans
    }

    /// Check if the current position is escaped (odd number of trailing backslashes)
    fn is_escaped(current: &str) -> bool {
        let trailing_backslashes = current.chars().rev().take_while(|&c| c == '\\').count();
        trailing_backslashes % 2 == 1
    }

    /// Get style for a JSON value based on its type
    fn get_value_style(value: &str) -> Style {
        let trimmed = value.trim();
        if trimmed == "true" || trimmed == "false" {
            Style::default().fg(Color::Magenta)
        } else if trimmed == "null" {
            Style::default().fg(Color::DarkGray)
        } else if trimmed.parse::<f64>().is_ok() {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        }
    }
}
