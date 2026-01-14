//! File viewer component for displaying file contents with scrolling
//!
//! This component renders a file's contents with optional line numbers
//! and a scrollbar.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::{render_minimal_scrollbar, text_muted, text_primary};
use crate::ui::file_viewer::FileViewerSession;

/// Renders a file viewer with line numbers and scrolling
pub struct FileViewerView<'a> {
    session: &'a FileViewerSession,
}

impl<'a> FileViewerView<'a> {
    pub fn new(session: &'a FileViewerSession) -> Self {
        Self { session }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let visible_height = area.height as usize;

        // Calculate width needed for line numbers
        let line_num_width = if self.session.show_line_numbers {
            // Calculate width needed for the largest line number
            let max_line = self.session.total_lines;
            let digits = if max_line == 0 {
                1
            } else {
                (max_line as f64).log10().floor() as usize + 1
            };
            digits + 3 // digits + " │ "
        } else {
            0
        };

        // Reserve 1 column for scrollbar
        let content_width = (area.width as usize).saturating_sub(line_num_width + 1);
        let scrollbar_x = area.x + area.width - 1;

        // Get visible lines
        let lines = self.session.visible_lines(visible_height);

        for (i, line_content) in lines.iter().enumerate() {
            let y = area.y + i as u16;
            if y >= area.y + area.height {
                break;
            }

            let line_num = self.session.scroll_offset + i + 1;
            let mut spans = Vec::new();

            // Line number
            if self.session.show_line_numbers {
                let num_str = format!(
                    "{:>width$} │ ",
                    line_num,
                    width = line_num_width.saturating_sub(3)
                );
                spans.push(Span::styled(num_str, Style::default().fg(text_muted())));
            }

            // Line content (truncated if necessary)
            let display_content = if line_content.width() > content_width && content_width > 1 {
                let mut width = 0;
                let truncated: String = line_content
                    .chars()
                    .take_while(|c| {
                        let char_width = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                        if width + char_width <= content_width.saturating_sub(1) {
                            width += char_width;
                            true
                        } else {
                            false
                        }
                    })
                    .collect();
                format!("{}…", truncated)
            } else {
                line_content.clone()
            };

            spans.push(Span::styled(
                display_content,
                Style::default().fg(text_primary()),
            ));

            let line = Line::from(spans);
            let line_area = Rect {
                x: area.x,
                y,
                width: area.width.saturating_sub(1), // Leave room for scrollbar
                height: 1,
            };
            Paragraph::new(line).render(line_area, buf);
        }

        // Render scrollbar if there's content to scroll
        if self.session.total_lines > 0 {
            let scrollbar_area = Rect {
                x: scrollbar_x,
                y: area.y,
                width: 1,
                height: area.height,
            };
            render_minimal_scrollbar(
                scrollbar_area,
                buf,
                self.session.total_lines,
                visible_height,
                self.session.scroll_offset,
            );
        }
    }
}
