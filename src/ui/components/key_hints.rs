//! Shared key hint rendering utilities.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{accent_primary, text_bright, text_faint, text_muted, text_secondary};

#[derive(Debug, Clone, Copy)]
pub struct KeyHintBarStyle {
    pub key_style: Style,
    pub action_style: Style,
    pub separator: Option<(&'static str, Style)>,
    pub item_gap: &'static str,
    pub key_prefix: &'static str,
    pub key_suffix: &'static str,
    pub leading: &'static str,
    pub trailing: &'static str,
    pub alignment: Alignment,
    pub background: Option<Color>,
}

impl KeyHintBarStyle {
    pub fn instruction_bar() -> Self {
        Self {
            key_style: Style::default().fg(accent_primary()),
            action_style: Style::default().fg(text_secondary()),
            separator: Some((" │ ", Style::default().fg(text_faint()))),
            item_gap: "",
            key_prefix: "",
            key_suffix: "",
            leading: "",
            trailing: "",
            alignment: Alignment::Center,
            background: None,
        }
    }

    pub fn footer_bar(key_bg: Color, footer_bg: Color) -> Self {
        Self {
            key_style: Style::default().fg(text_secondary()).bg(key_bg),
            action_style: Style::default().fg(text_muted()),
            separator: None,
            item_gap: "   ",
            key_prefix: " ",
            key_suffix: " ",
            leading: " ",
            trailing: "",
            alignment: Alignment::Left,
            background: Some(footer_bg),
        }
    }

    /// Minimal footer style - just text with bright keys and muted descriptions
    pub fn minimal_footer() -> Self {
        Self {
            key_style: Style::default().fg(text_bright()),
            action_style: Style::default().fg(text_muted()),
            separator: None,
            item_gap: "   ",
            key_prefix: "",
            key_suffix: "",
            leading: "",
            trailing: "  ",
            alignment: Alignment::Right,
            background: None,
        }
    }
}

pub fn render_key_hints(
    area: Rect,
    buf: &mut Buffer,
    hints: &[(&str, &str)],
    style: KeyHintBarStyle,
) {
    let mut spans = Vec::new();

    if !style.leading.is_empty() {
        spans.push(Span::raw(style.leading));
    }

    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            if let Some((sep, sep_style)) = style.separator {
                spans.push(Span::styled(sep, sep_style));
            } else if !style.item_gap.is_empty() {
                spans.push(Span::raw(style.item_gap));
            }
        }

        let key_text = format!("{}{}{}", style.key_prefix, key, style.key_suffix);
        spans.push(Span::styled(key_text, style.key_style));
        spans.push(Span::styled(format!(" {}", action), style.action_style));
    }

    if !style.trailing.is_empty() {
        spans.push(Span::raw(style.trailing));
    }

    let line = Line::from(spans);
    let mut paragraph = Paragraph::new(line).alignment(style.alignment);
    if let Some(bg) = style.background {
        paragraph = paragraph.style(Style::default().bg(bg));
    }

    paragraph.render(area, buf);
}

/// Render key hints responsively, removing hints from the LEFT when they don't fit.
/// Returns the width of the rendered hints (for layout calculations).
pub fn render_key_hints_responsive(
    area: Rect,
    buf: &mut Buffer,
    hints: &[(&str, &str)],
    style: KeyHintBarStyle,
    max_width: Option<u16>,
) -> u16 {
    let available_width = max_width.unwrap_or(area.width) as usize;

    // Calculate width of each hint (including gap/separator)
    let hint_widths: Vec<usize> = hints
        .iter()
        .enumerate()
        .map(|(i, (key, action))| {
            let key_text = format!("{}{}{}", style.key_prefix, key, style.key_suffix);
            let action_text = format!(" {}", action);
            let base_width = key_text.len() + action_text.len();

            if i > 0 {
                if style.separator.is_some() {
                    base_width + 3 // " │ " is 3 chars
                } else {
                    base_width + style.item_gap.len()
                }
            } else {
                base_width
            }
        })
        .collect();

    // Calculate fixed overhead (leading + trailing)
    let overhead = style.leading.len() + style.trailing.len();

    // Find the starting index - remove hints from LEFT until it fits
    let mut start_index = 0;
    let mut total_width: usize = hint_widths.iter().sum::<usize>() + overhead;

    while total_width > available_width && start_index < hints.len() {
        // Remove leftmost hint (use saturating_sub to prevent overflow)
        total_width = total_width.saturating_sub(hint_widths[start_index]);
        // If this wasn't the first hint, we also need to remove separator/gap from next hint
        if start_index + 1 < hints.len() {
            let gap_width = if style.separator.is_some() {
                3
            } else {
                style.item_gap.len()
            };
            total_width = total_width.saturating_sub(gap_width);
        }
        start_index += 1;
    }

    // Build spans for remaining hints
    let remaining_hints = &hints[start_index..];
    let mut spans = Vec::new();

    if !style.leading.is_empty() {
        spans.push(Span::raw(style.leading));
    }

    for (i, (key, action)) in remaining_hints.iter().enumerate() {
        if i > 0 {
            if let Some((sep, sep_style)) = style.separator {
                spans.push(Span::styled(sep, sep_style));
            } else if !style.item_gap.is_empty() {
                spans.push(Span::raw(style.item_gap));
            }
        }

        let key_text = format!("{}{}{}", style.key_prefix, key, style.key_suffix);
        spans.push(Span::styled(key_text, style.key_style));
        spans.push(Span::styled(format!(" {}", action), style.action_style));
    }

    if !style.trailing.is_empty() {
        spans.push(Span::raw(style.trailing));
    }

    // Calculate actual width of what we're rendering
    let actual_width: u16 = spans.iter().map(|s| s.width() as u16).sum();

    // Render right-aligned
    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).alignment(Alignment::Right);

    paragraph.render(area, buf);

    actual_width
}
