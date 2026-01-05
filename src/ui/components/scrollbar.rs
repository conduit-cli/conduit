//! Shared scrollbar rendering utilities.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use ratatui::prelude::StatefulWidget;

#[derive(Debug, Clone, Copy)]
pub struct ScrollbarSymbols {
    pub begin: Option<&'static str>,
    pub end: Option<&'static str>,
    pub track: Option<&'static str>,
    pub thumb: Option<&'static str>,
}

impl ScrollbarSymbols {
    pub fn standard() -> Self {
        Self {
            begin: Some("▲"),
            end: Some("▼"),
            track: Some("│"),
            thumb: Some("█"),
        }
    }

    pub fn arrows() -> Self {
        Self {
            begin: Some("↑"),
            end: Some("↓"),
            track: None,
            thumb: None,
        }
    }
}

impl Default for ScrollbarSymbols {
    fn default() -> Self {
        Self::standard()
    }
}

/// Render a vertical scrollbar on the right edge if needed.
pub fn render_vertical_scrollbar(
    area: Rect,
    buf: &mut Buffer,
    total: usize,
    visible: usize,
    offset: usize,
    symbols: ScrollbarSymbols,
) {
    if total <= visible {
        return;
    }

    let max_scroll = total.saturating_sub(visible);

    let mut scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    if let Some(begin) = symbols.begin {
        scrollbar = scrollbar.begin_symbol(Some(begin));
    }
    if let Some(end) = symbols.end {
        scrollbar = scrollbar.end_symbol(Some(end));
    }
    if let Some(track) = symbols.track {
        scrollbar = scrollbar.track_symbol(Some(track));
    }
    if let Some(thumb) = symbols.thumb {
        scrollbar = scrollbar.thumb_symbol(thumb);
    }

    let mut scrollbar_state = ScrollbarState::new(max_scroll).position(offset);
    scrollbar.render(area, buf, &mut scrollbar_state);
}
