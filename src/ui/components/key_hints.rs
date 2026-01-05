//! Shared key hint rendering utilities.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::{ACCENT_PRIMARY, TEXT_FAINT, TEXT_MUTED, TEXT_SECONDARY};

#[derive(Debug, Clone, Copy)]
pub struct KeyHintBarStyle {
    pub key_style: Style,
    pub action_style: Style,
    pub separator: Option<(&'static str, Style)>,
    pub item_gap: &'static str,
    pub key_prefix: &'static str,
    pub key_suffix: &'static str,
    pub leading: &'static str,
    pub alignment: Alignment,
    pub background: Option<Color>,
}

impl KeyHintBarStyle {
    pub fn instruction_bar() -> Self {
        Self {
            key_style: Style::default().fg(ACCENT_PRIMARY),
            action_style: Style::default().fg(TEXT_SECONDARY),
            separator: Some((" │ ", Style::default().fg(TEXT_FAINT))),
            item_gap: "",
            key_prefix: "",
            key_suffix: "",
            leading: "",
            alignment: Alignment::Center,
            background: None,
        }
    }

    pub fn footer_bar(key_bg: Color, footer_bg: Color) -> Self {
        Self {
            key_style: Style::default().fg(TEXT_SECONDARY).bg(key_bg),
            action_style: Style::default().fg(TEXT_MUTED),
            separator: None,
            item_gap: "   ",
            key_prefix: " ",
            key_suffix: " ",
            leading: " ",
            alignment: Alignment::Left,
            background: Some(footer_bg),
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

    let line = Line::from(spans);
    let mut paragraph = Paragraph::new(line).alignment(style.alignment);
    if let Some(bg) = style.background {
        paragraph = paragraph.style(Style::default().bg(bg));
    }

    paragraph.render(area, buf);
}
