use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    accent_primary, accent_success, accent_warning, bg_elevated, tab_bar_bg, text_muted,
    text_primary, text_secondary,
};
/// Spinner animation frames (Braille Dots B)
const SPINNER_FRAMES: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBarHitTarget {
    None,
    Tab(usize),
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, Clone)]
struct TabBarItem {
    index: usize,
    start: usize,
    end: usize,
}

struct TabBarLayout {
    spans: Vec<Span<'static>>,
    items: Vec<TabBarItem>,
    total_width: usize,
    visible_width: usize,
    indicator_width: usize,
    indicator_side_width: usize,
    max_scroll: usize,
    scroll_offset: usize,
}

impl TabBarLayout {
    fn item_at_offset(&self, offset: usize) -> TabBarHitTarget {
        if offset >= self.total_width {
            return TabBarHitTarget::None;
        }

        for item in &self.items {
            if offset >= item.start && offset < item.end {
                return TabBarHitTarget::Tab(item.index);
            }
        }

        TabBarHitTarget::None
    }
}

/// Tab bar component for switching between sessions
pub struct TabBar {
    tabs: Vec<String>,
    active: usize,
    focused: bool,
    /// PR numbers for each tab (None = no PR)
    /// TODO: Remove if sidebar PR display is not implemented, or use for future tab tooltip
    #[allow(dead_code)]
    pr_numbers: Vec<Option<u32>>,
    /// Whether each tab is currently processing (agent working)
    processing_flags: Vec<bool>,
    /// Whether each tab has unread content (new messages arrived while not focused)
    attention_flags: Vec<bool>,
    /// Whether each tab is awaiting user response (inline prompt active)
    awaiting_response_flags: Vec<bool>,
    /// Current spinner frame index
    spinner_frame: usize,
    /// Horizontal scroll offset in columns
    scroll_offset: usize,
}

impl TabBar {
    pub fn new(tabs: Vec<String>, active: usize) -> Self {
        let tab_count = tabs.len();
        Self {
            tabs,
            active,
            focused: true,
            pr_numbers: vec![None; tab_count],
            processing_flags: vec![false; tab_count],
            attention_flags: vec![false; tab_count],
            awaiting_response_flags: vec![false; tab_count],
            spinner_frame: 0,
            scroll_offset: 0,
        }
    }

    /// Set whether the tab bar is focused
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set tab states (PR numbers, processing, attention, awaiting response flags)
    pub fn with_tab_states(
        mut self,
        pr_numbers: Vec<Option<u32>>,
        processing: Vec<bool>,
        attention: Vec<bool>,
        awaiting_response: Vec<bool>,
    ) -> Self {
        self.pr_numbers = pr_numbers;
        self.processing_flags = processing;
        self.attention_flags = attention;
        self.awaiting_response_flags = awaiting_response;
        self
    }

    /// Set horizontal scroll offset for the tab bar
    pub fn with_scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = offset;
        self
    }

    /// Set current spinner frame
    pub fn with_spinner_frame(mut self, frame: usize) -> Self {
        self.spinner_frame = frame;
        self
    }

    /// Get spinner character for current frame
    fn spinner_char(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()]
    }

    /// Ensure the active tab is visible within the given width.
    pub fn adjust_scroll_to_active(&self, area_width: u16) -> usize {
        let layout = self.layout(area_width);
        let Some(active_item) = layout.items.iter().find(|item| item.index == self.active) else {
            return 0;
        };

        let mut scroll = layout.scroll_offset;
        if active_item.start < scroll {
            scroll = active_item.start;
        } else if active_item.end > scroll.saturating_add(layout.visible_width) {
            scroll = active_item.end.saturating_sub(layout.visible_width);
        }

        scroll.min(layout.max_scroll)
    }

    /// Scroll left by one tab.
    pub fn scroll_left(&self, area_width: u16) -> usize {
        let layout = self.layout(area_width);
        if layout.max_scroll == 0 {
            return layout.scroll_offset;
        }

        let offset = layout.scroll_offset;
        let target = layout
            .items
            .iter()
            .rev()
            .find(|item| item.start < offset)
            .map(|item| item.start)
            .unwrap_or(0);

        target.min(layout.max_scroll)
    }

    /// Scroll right by one tab.
    pub fn scroll_right(&self, area_width: u16) -> usize {
        let layout = self.layout(area_width);
        if layout.max_scroll == 0 {
            return layout.scroll_offset;
        }

        let right_edge = layout.scroll_offset.saturating_add(layout.visible_width);
        let target = layout
            .items
            .iter()
            .find(|item| item.end > right_edge)
            .map(|item| item.start)
            .unwrap_or(layout.scroll_offset);

        target.min(layout.max_scroll)
    }

    /// Max scroll offset for the current layout.
    pub fn max_scroll(&self, area_width: u16) -> usize {
        self.layout(area_width).max_scroll
    }

    /// Hit test the tab bar at the given x position.
    pub fn hit_test(&self, area: Rect, x: u16) -> TabBarHitTarget {
        let layout = self.layout(area.width);
        let relative_x = x.saturating_sub(area.x) as usize;

        if layout.indicator_width > 0 && area.width as usize > layout.indicator_width {
            if relative_x < layout.indicator_side_width {
                return if layout.scroll_offset > 0 {
                    TabBarHitTarget::ScrollLeft
                } else {
                    TabBarHitTarget::None
                };
            }
            if relative_x >= area.width as usize - layout.indicator_side_width {
                return if layout.scroll_offset < layout.max_scroll {
                    TabBarHitTarget::ScrollRight
                } else {
                    TabBarHitTarget::None
                };
            }

            let content_x = relative_x.saturating_sub(layout.indicator_side_width);
            if content_x >= layout.visible_width {
                return TabBarHitTarget::None;
            }
            let offset = layout.scroll_offset.saturating_add(content_x);
            return layout.item_at_offset(offset);
        }

        let offset = layout.scroll_offset.saturating_add(relative_x);
        layout.item_at_offset(offset)
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let layout = self.layout(area.width);
        let visible_spans = if layout.visible_width == 0 {
            Vec::new()
        } else {
            slice_spans(&layout.spans, layout.scroll_offset, layout.visible_width)
        };

        let mut spans = Vec::new();
        if layout.indicator_width > 0 {
            let enabled = Style::default().fg(text_secondary());
            let disabled = Style::default().fg(text_muted());
            let left_label = if layout.scroll_offset > 0 {
                " ‹"
            } else {
                "  "
            };
            spans.push(Span::styled(
                left_label,
                if layout.scroll_offset > 0 {
                    enabled
                } else {
                    disabled
                },
            ));
        }

        spans.extend(visible_spans);

        if layout.indicator_width > 0 {
            let enabled = Style::default().fg(text_secondary());
            let disabled = Style::default().fg(text_muted());
            let right_label = if layout.scroll_offset < layout.max_scroll {
                "› "
            } else {
                "  "
            };
            spans.push(Span::styled(
                right_label,
                if layout.scroll_offset < layout.max_scroll {
                    enabled
                } else {
                    disabled
                },
            ));
        }

        // Render the tab line on the first row
        let tab_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(tab_bar_bg()));
        paragraph.render(tab_area, buf);
    }

    fn layout(&self, area_width: u16) -> TabBarLayout {
        let (spans, items, total_width) = self.build_items();
        let area_width = area_width as usize;
        let has_overflow = total_width > area_width;
        let indicator_side_width = if has_overflow && area_width > 4 { 2 } else { 0 };
        let indicator_width = indicator_side_width * 2;
        let visible_width = area_width.saturating_sub(indicator_width);
        let max_scroll = total_width.saturating_sub(visible_width);
        let scroll_offset = self.scroll_offset.min(max_scroll);

        TabBarLayout {
            spans,
            items,
            total_width,
            visible_width,
            indicator_width,
            indicator_side_width,
            max_scroll,
            scroll_offset,
        }
    }

    fn build_items(&self) -> (Vec<Span<'static>>, Vec<TabBarItem>, usize) {
        let mut spans = Vec::new();
        let mut items = Vec::new();
        let mut current_x = 0usize;

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active;
            let is_processing = self.processing_flags.get(i).copied().unwrap_or(false);
            let needs_attention = self.attention_flags.get(i).copied().unwrap_or(false);
            let awaiting_response = self
                .awaiting_response_flags
                .get(i)
                .copied()
                .unwrap_or(false);

            let mut tab_width = 0usize;

            let active_bg_style = if is_active {
                Style::default().bg(bg_elevated())
            } else {
                Style::default()
            };

            let indicator_span = if is_active && self.focused {
                Span::styled(" ▸ ", active_bg_style.fg(accent_primary()))
            } else if is_active {
                Span::styled("   ", active_bg_style)
            } else {
                Span::raw("   ")
            };
            tab_width += span_width(&indicator_span);
            spans.push(indicator_span);

            // Priority: awaiting_response > processing > needs_attention
            if awaiting_response {
                // Show orange dot when awaiting user response (inline prompt active)
                let awaiting_span = Span::styled("● ", active_bg_style.fg(accent_warning()));
                tab_width += span_width(&awaiting_span);
                spans.push(awaiting_span);
            } else if is_processing {
                let spinner_span = Span::styled(
                    format!("{} ", self.spinner_char()),
                    active_bg_style.fg(accent_warning()),
                );
                tab_width += span_width(&spinner_span);
                spans.push(spinner_span);
            } else if needs_attention {
                let attention_span = Span::styled("● ", active_bg_style.fg(accent_success()));
                tab_width += span_width(&attention_span);
                spans.push(attention_span);
            }

            let tab_style = if is_active {
                if self.focused {
                    active_bg_style
                        .fg(text_primary())
                        .add_modifier(Modifier::BOLD)
                } else {
                    active_bg_style.fg(text_secondary())
                }
            } else {
                Style::default().fg(text_muted())
            };

            let tab_text = format!("[{}] {}", i + 1, tab);
            let tab_span = Span::styled(tab_text, tab_style);
            tab_width += span_width(&tab_span);
            spans.push(tab_span);

            let trailing_span = if is_active {
                Span::styled("  ", active_bg_style)
            } else {
                Span::raw("  ")
            };
            tab_width += span_width(&trailing_span);
            spans.push(trailing_span);

            items.push(TabBarItem {
                index: i,
                start: current_x,
                end: current_x + tab_width,
            });
            current_x += tab_width;
        }

        (spans, items, current_x)
    }
}

fn span_width(span: &Span<'_>) -> usize {
    UnicodeWidthStr::width(span.content.as_ref())
}

fn slice_spans(spans: &[Span<'static>], start: usize, width: usize) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let mut remaining_start = start;
    let mut remaining_width = width;
    let mut result = Vec::new();

    for span in spans {
        if remaining_width == 0 {
            break;
        }

        let span_text = span.content.as_ref();
        let span_width = UnicodeWidthStr::width(span_text);
        if remaining_start >= span_width {
            remaining_start -= span_width;
            continue;
        }

        let slice_start = remaining_start;
        let slice_width = remaining_width.min(span_width.saturating_sub(slice_start));
        let slice_text = slice_text_by_width(span_text, slice_start, slice_width);
        let actual_width = UnicodeWidthStr::width(slice_text.as_str());
        if actual_width > 0 {
            result.push(Span::styled(slice_text, span.style));
            remaining_width = remaining_width.saturating_sub(actual_width);
        }
        remaining_start = 0;
    }

    result
}

fn slice_text_by_width(text: &str, start: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let mut result = String::new();
    let mut position = 0usize;
    let mut remaining = width;

    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if position + ch_width <= start {
            position += ch_width;
            continue;
        }

        if position < start {
            position += ch_width;
            continue;
        }

        if ch_width > remaining {
            break;
        }

        result.push(ch);
        remaining = remaining.saturating_sub(ch_width);
        position += ch_width;

        if remaining == 0 {
            break;
        }
    }

    result
}
