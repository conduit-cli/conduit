use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use super::theme::MARKDOWN_CODE_BG;

/// Custom markdown renderer with table support
pub struct MarkdownRenderer {
    /// Base style for text
    base_style: Style,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            base_style: Style::default().fg(Color::White),
        }
    }

    /// Render markdown string to ratatui Text
    pub fn render(&self, markdown: &str) -> Text<'static> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);

        let parser = Parser::new_ext(markdown, options);
        let events: Vec<Event> = parser.collect();

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut style_stack: Vec<Style> = vec![self.base_style];

        // Table state
        let mut in_table = false;
        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut current_cell = String::new();
        let mut table_alignments: Vec<pulldown_cmark::Alignment> = Vec::new();

        // List state
        let mut list_depth: usize = 0;
        let mut ordered_list_counters: Vec<u64> = Vec::new();

        // Code block state
        let mut in_code_block = false;
        let mut code_block_content = String::new();

        for event in events {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => {}
                    Tag::Heading { level, .. } => {
                        let style = self.heading_style(level);
                        style_stack.push(style);
                    }
                    Tag::BlockQuote(_) => {
                        current_spans
                            .push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
                        style_stack.push(
                            Style::default()
                                .fg(Color::Gray)
                                .add_modifier(Modifier::ITALIC),
                        );
                    }
                    Tag::CodeBlock(_) => {
                        in_code_block = true;
                        code_block_content.clear();
                    }
                    Tag::List(start) => {
                        list_depth += 1;
                        if let Some(n) = start {
                            ordered_list_counters.push(n);
                        } else {
                            ordered_list_counters.push(0); // 0 = unordered
                        }
                    }
                    Tag::Item => {
                        let indent = "  ".repeat(list_depth.saturating_sub(1));
                        let bullet = if let Some(&counter) = ordered_list_counters.last() {
                            if counter == 0 {
                                format!("{}• ", indent)
                            } else {
                                let idx = ordered_list_counters.last_mut().unwrap();
                                let bullet = format!("{}{}. ", indent, idx);
                                *idx += 1;
                                bullet
                            }
                        } else {
                            format!("{}• ", indent)
                        };
                        current_spans.push(Span::styled(bullet, Style::default().fg(Color::Cyan)));
                    }
                    Tag::Emphasis => {
                        let current = *style_stack.last().unwrap_or(&self.base_style);
                        style_stack.push(current.add_modifier(Modifier::ITALIC));
                    }
                    Tag::Strong => {
                        let current = *style_stack.last().unwrap_or(&self.base_style);
                        style_stack.push(current.add_modifier(Modifier::BOLD));
                    }
                    Tag::Strikethrough => {
                        let current = *style_stack.last().unwrap_or(&self.base_style);
                        style_stack.push(current.add_modifier(Modifier::CROSSED_OUT));
                    }
                    Tag::Link { .. } => {
                        style_stack.push(
                            Style::default()
                                .fg(Color::Blue)
                                .add_modifier(Modifier::UNDERLINED),
                        );
                    }
                    Tag::Table(alignments) => {
                        in_table = true;
                        table_rows.clear();
                        table_alignments = alignments;
                    }
                    Tag::TableHead => {
                        current_row.clear();
                    }
                    Tag::TableRow => {
                        current_row.clear();
                    }
                    Tag::TableCell => {
                        current_cell.clear();
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Paragraph => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        lines.push(Line::from("")); // Empty line after paragraph
                    }
                    TagEnd::Heading(_) => {
                        style_stack.pop();
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        lines.push(Line::from(""));
                    }
                    TagEnd::BlockQuote(_) => {
                        style_stack.pop();
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        // Render code block with background
                        let code_style = Style::default().fg(Color::Green).bg(MARKDOWN_CODE_BG);

                        lines.push(Line::from(Span::styled(
                            "```",
                            Style::default().fg(Color::DarkGray),
                        )));
                        for code_line in code_block_content.lines() {
                            lines.push(Line::from(Span::styled(
                                format!(" {} ", code_line),
                                code_style,
                            )));
                        }
                        lines.push(Line::from(Span::styled(
                            "```",
                            Style::default().fg(Color::DarkGray),
                        )));
                        lines.push(Line::from(""));
                    }
                    TagEnd::List(_) => {
                        list_depth = list_depth.saturating_sub(1);
                        ordered_list_counters.pop();
                        if list_depth == 0 {
                            lines.push(Line::from(""));
                        }
                    }
                    TagEnd::Item => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                    }
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                        style_stack.pop();
                    }
                    TagEnd::Table => {
                        in_table = false;
                        // Render the table
                        self.render_table(&table_rows, &table_alignments, &mut lines);
                        table_rows.clear();
                        lines.push(Line::from(""));
                    }
                    TagEnd::TableHead => {
                        if !current_row.is_empty() {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    TagEnd::TableRow => {
                        if !current_row.is_empty() {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    TagEnd::TableCell => {
                        current_row.push(std::mem::take(&mut current_cell));
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if in_code_block {
                        code_block_content.push_str(&text);
                    } else if in_table {
                        current_cell.push_str(&text);
                    } else {
                        let style = *style_stack.last().unwrap_or(&self.base_style);
                        current_spans.push(Span::styled(text.to_string(), style));
                    }
                }
                Event::Code(code) => {
                    if in_table {
                        current_cell.push('`');
                        current_cell.push_str(&code);
                        current_cell.push('`');
                    } else {
                        current_spans.push(Span::styled(
                            format!("`{}`", code),
                            Style::default()
                                .fg(Color::Yellow)
                                .bg(Color::Rgb(40, 40, 40)),
                        ));
                    }
                }
                Event::SoftBreak => {
                    if in_table {
                        current_cell.push(' ');
                    } else {
                        current_spans.push(Span::raw(" "));
                    }
                }
                Event::HardBreak => {
                    if !in_table {
                        lines.push(Line::from(std::mem::take(&mut current_spans)));
                    }
                }
                Event::Rule => {
                    lines.push(Line::from(Span::styled(
                        "─".repeat(40),
                        Style::default().fg(Color::DarkGray),
                    )));
                    lines.push(Line::from(""));
                }
                Event::TaskListMarker(checked) => {
                    let marker = if checked { "☑ " } else { "☐ " };
                    current_spans.push(Span::styled(
                        marker,
                        Style::default().fg(if checked { Color::Green } else { Color::Gray }),
                    ));
                }
                _ => {}
            }
        }

        // Flush any remaining spans
        if !current_spans.is_empty() {
            lines.push(Line::from(current_spans));
        }

        Text::from(lines)
    }

    fn heading_style(&self, level: HeadingLevel) -> Style {
        match level {
            HeadingLevel::H1 => Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            HeadingLevel::H2 => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            HeadingLevel::H3 => Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            HeadingLevel::H4 => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            HeadingLevel::H5 => Style::default().fg(Color::Green),
            HeadingLevel::H6 => Style::default().fg(Color::Gray),
        }
    }

    fn render_table(
        &self,
        rows: &[Vec<String>],
        alignments: &[pulldown_cmark::Alignment],
        lines: &mut Vec<Line<'static>>,
    ) {
        if rows.is_empty() {
            return;
        }

        // Calculate column widths
        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut col_widths: Vec<usize> = vec![0; num_cols];

        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Ensure minimum width
        for w in &mut col_widths {
            *w = (*w).max(3);
        }

        let border_style = Style::default().fg(Color::DarkGray);
        let header_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let cell_style = Style::default().fg(Color::White);

        // Top border
        let top_border = self.table_border(&col_widths, '┌', '┬', '┐', '─');
        lines.push(Line::from(Span::styled(top_border, border_style)));

        for (row_idx, row) in rows.iter().enumerate() {
            let mut spans = Vec::new();
            spans.push(Span::styled("│", border_style));

            for (col_idx, cell) in row.iter().enumerate() {
                let width = col_widths.get(col_idx).copied().unwrap_or(3);
                let alignment = alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(pulldown_cmark::Alignment::None);

                let padded = self.align_text(cell, width, alignment);
                let style = if row_idx == 0 {
                    header_style
                } else {
                    cell_style
                };

                spans.push(Span::styled(format!(" {} ", padded), style));
                spans.push(Span::styled("│", border_style));
            }

            // Pad missing columns
            for col_idx in row.len()..num_cols {
                let width = col_widths.get(col_idx).copied().unwrap_or(3);
                spans.push(Span::styled(
                    format!(" {:width$} ", "", width = width),
                    cell_style,
                ));
                spans.push(Span::styled("│", border_style));
            }

            lines.push(Line::from(spans));

            // Header separator
            if row_idx == 0 && rows.len() > 1 {
                let sep = self.table_border(&col_widths, '├', '┼', '┤', '─');
                lines.push(Line::from(Span::styled(sep, border_style)));
            }
        }

        // Bottom border
        let bottom_border = self.table_border(&col_widths, '└', '┴', '┘', '─');
        lines.push(Line::from(Span::styled(bottom_border, border_style)));
    }

    fn table_border(
        &self,
        widths: &[usize],
        left: char,
        mid: char,
        right: char,
        fill: char,
    ) -> String {
        let mut result = String::new();
        result.push(left);

        for (i, &width) in widths.iter().enumerate() {
            #[allow(clippy::manual_repeat_n)]
            result.extend(std::iter::repeat(fill).take(width + 2)); // +2 for padding
            if i < widths.len() - 1 {
                result.push(mid);
            }
        }

        result.push(right);
        result
    }

    fn align_text(&self, text: &str, width: usize, alignment: pulldown_cmark::Alignment) -> String {
        let text_len = text.chars().count();
        if text_len >= width {
            return text.chars().take(width).collect();
        }

        let padding = width - text_len;
        match alignment {
            pulldown_cmark::Alignment::Left | pulldown_cmark::Alignment::None => {
                format!("{}{}", text, " ".repeat(padding))
            }
            pulldown_cmark::Alignment::Right => {
                format!("{}{}", " ".repeat(padding), text)
            }
            pulldown_cmark::Alignment::Center => {
                let left_pad = padding / 2;
                let right_pad = padding - left_pad;
                format!("{}{}{}", " ".repeat(left_pad), text, " ".repeat(right_pad))
            }
        }
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_table() {
        let md = r#"
| Name | Age |
|------|-----|
| Alice | 30 |
| Bob | 25 |
"#;
        let renderer = MarkdownRenderer::new();
        let text = renderer.render(md);
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn test_code_block() {
        let md = r#"
```rust
fn main() {
    println!("Hello");
}
```
"#;
        let renderer = MarkdownRenderer::new();
        let text = renderer.render(md);
        assert!(!text.lines.is_empty());
    }
}
