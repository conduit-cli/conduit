//! Shared scrollbar rendering utilities.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
};

use super::{BG_ELEVATED, BG_TERMINAL, TEXT_FAINT};

#[derive(Debug, Clone, Copy)]
pub struct ScrollbarMetrics {
    pub area: Rect,
    pub total: usize,
    pub visible: usize,
}

/// Render a minimal vertical scrollbar using only background colors.
/// The thumb is a solid color, the track is a subtle background.
/// Uses `▀` at top and `▄` at bottom as caps that light up when thumb reaches edges.
pub fn render_minimal_scrollbar(
    area: Rect,
    buf: &mut Buffer,
    total: usize,
    visible: usize,
    offset: usize,
) {
    render_minimal_scrollbar_styled(area, buf, total, visible, offset, BG_ELEVATED, TEXT_FAINT)
}

/// Render a minimal vertical scrollbar with custom colors.
/// - track_color: background color for the track/gutter
/// - thumb_color: color for the scrollbar thumb
pub fn render_minimal_scrollbar_styled(
    area: Rect,
    buf: &mut Buffer,
    total: usize,
    visible: usize,
    offset: usize,
    track_color: Color,
    thumb_color: Color,
) {
    if area.height < 3 || area.width == 0 {
        // Need at least 3 rows: top cap, middle, bottom cap
        return;
    }

    let top_y = area.y;
    let bottom_y = area.y + area.height - 1;
    let track_start = area.y + 1;
    let track_height = (area.height - 2) as usize; // Exclude top and bottom caps

    // If content fits, render empty track with inactive caps
    if total <= visible {
        // Top cap: ▀ has fg=top half, bg=bottom half
        // We want top half = terminal bg (outside), bottom half = track (inside)
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, top_y)];
            cell.set_char('▀');
            cell.set_fg(BG_TERMINAL);
            cell.set_bg(track_color);
        }

        // Track
        for y in track_start..bottom_y {
            for x in area.x..area.x + area.width {
                let cell = &mut buf[(x, y)];
                cell.set_char(' ');
                cell.set_bg(track_color);
            }
        }

        // Bottom cap: ▄ has fg=bottom half, bg=top half
        // We want bottom half = terminal bg (outside), top half = track (inside)
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, bottom_y)];
            cell.set_char('▄');
            cell.set_fg(BG_TERMINAL);
            cell.set_bg(track_color);
        }
        return;
    }

    let max_scroll = total.saturating_sub(visible);

    // Calculate thumb size (proportional to visible/total ratio, minimum 1)
    let thumb_height = ((visible as f64 / total as f64) * track_height as f64)
        .round()
        .max(1.0) as usize;
    let thumb_height = thumb_height.min(track_height);

    // Calculate thumb position within track
    let scroll_range = track_height.saturating_sub(thumb_height);
    let thumb_start = if max_scroll > 0 {
        (offset as f64 / max_scroll as f64 * scroll_range as f64).round() as usize
    } else {
        0
    };
    let thumb_end = thumb_start + thumb_height;

    // Check if thumb is at edges
    let at_top = offset == 0;
    let at_bottom = offset >= max_scroll;

    // Top cap: ▀ has fg=top half (outside), bg=bottom half (inside)
    // When at top, the inside (bg) lights up with thumb color
    for x in area.x..area.x + area.width {
        let cell = &mut buf[(x, top_y)];
        cell.set_char('▀');
        cell.set_fg(BG_TERMINAL);
        cell.set_bg(if at_top { thumb_color } else { track_color });
    }

    // Track and thumb
    for (i, y) in (track_start..bottom_y).enumerate() {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            if i >= thumb_start && i < thumb_end {
                cell.set_bg(thumb_color);
            } else {
                cell.set_bg(track_color);
            }
        }
    }

    // Bottom cap: ▄ has fg=bottom half (outside), bg=top half (inside)
    // When at bottom, the inside (bg) lights up with thumb color
    for x in area.x..area.x + area.width {
        let cell = &mut buf[(x, bottom_y)];
        cell.set_char('▄');
        cell.set_fg(BG_TERMINAL);
        cell.set_bg(if at_bottom { thumb_color } else { track_color });
    }
}

pub fn scrollbar_offset_from_point(y: u16, area: Rect, total: usize, visible: usize) -> usize {
    let max_scroll = total.saturating_sub(visible);
    if max_scroll == 0 || area.height == 0 {
        return 0;
    }

    let track_len = area.height.saturating_sub(1) as usize;
    if track_len == 0 {
        return 0;
    }

    let rel = y.saturating_sub(area.y) as usize;
    let rel = rel.min(track_len);
    (rel * max_scroll + track_len / 2) / track_len
}
