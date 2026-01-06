use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Represents a file change with diff stats
#[derive(Debug, Clone)]
pub struct FileChange {
    pub filename: String,
    pub additions: usize,
    pub deletions: usize,
}

/// Summary of a completed turn
#[derive(Debug, Clone, Default)]
pub struct TurnSummary {
    /// Duration in seconds
    pub duration_secs: u64,
    /// Input tokens used
    pub input_tokens: u64,
    /// Output tokens generated
    pub output_tokens: u64,
    /// Files that were modified
    pub files_changed: Vec<FileChange>,
}

impl TurnSummary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set duration from seconds
    pub fn with_duration(mut self, secs: u64) -> Self {
        self.duration_secs = secs;
        self
    }

    /// Set token usage
    pub fn with_tokens(mut self, input: u64, output: u64) -> Self {
        self.input_tokens = input;
        self.output_tokens = output;
        self
    }

    /// Add a file change
    pub fn add_file(&mut self, filename: impl Into<String>, additions: usize, deletions: usize) {
        self.files_changed.push(FileChange {
            filename: filename.into(),
            additions,
            deletions,
        });
    }

    /// Format duration as human-readable string
    pub fn format_duration(&self) -> String {
        let secs = self.duration_secs;
        if secs >= 60 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}s", secs)
        }
    }

    /// Format token count (abbreviate if large)
    pub fn format_tokens(count: u64) -> String {
        if count >= 1000 {
            format!("{:.1}k", count as f64 / 1000.0)
        } else {
            count.to_string()
        }
    }

    /// Render the turn summary as a Line
    pub fn render(&self, max_width: usize) -> Line<'static> {
        let mut spans = Vec::new();

        // Duration: ⏱ 2m 34s
        spans.push(Span::styled("⏱ ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            self.format_duration(),
            Style::default().fg(Color::Gray),
        ));

        // Separator
        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));

        // Tokens: ⬇1.2k ⬆856
        spans.push(Span::styled("↓", Style::default().fg(Color::Cyan)));
        spans.push(Span::styled(
            Self::format_tokens(self.input_tokens),
            Style::default().fg(Color::Cyan),
        ));
        spans.push(Span::styled(" ↑", Style::default().fg(Color::Magenta)));
        spans.push(Span::styled(
            Self::format_tokens(self.output_tokens),
            Style::default().fg(Color::Magenta),
        ));

        // Files changed (show up to 3, then overflow)
        if !self.files_changed.is_empty() {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));

            let max_files = 3;
            let overflow = self.files_changed.len().saturating_sub(max_files);
            let display_files = &self.files_changed[..self.files_changed.len().min(max_files)];

            // Calculate overflow totals
            let (overflow_add, overflow_del) = if overflow > 0 {
                self.files_changed[max_files..]
                    .iter()
                    .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions))
            } else {
                (0, 0)
            };

            for (i, file) in display_files.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                }

                // Shorten filename if needed
                let filename = Self::shorten_filename(&file.filename, 15);

                spans.push(Span::styled("◉ ", Style::default().fg(Color::Yellow)));
                spans.push(Span::styled(
                    filename,
                    Style::default().fg(Color::White),
                ));
                spans.push(Span::styled(
                    format!(" +{}", file.additions),
                    Style::default().fg(Color::Green),
                ));
                spans.push(Span::styled(
                    format!(" -{}", file.deletions),
                    Style::default().fg(Color::Red),
                ));
            }

            // Overflow indicator
            if overflow > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                spans.push(Span::styled(
                    format!("+{} more", overflow),
                    Style::default().fg(Color::DarkGray),
                ));
                spans.push(Span::styled(
                    format!(" +{}", overflow_add),
                    Style::default().fg(Color::Green),
                ));
                spans.push(Span::styled(
                    format!(" -{}", overflow_del),
                    Style::default().fg(Color::Red),
                ));
            }
        }

        // Respect max_width by truncating if needed
        let _ = max_width; // TODO: implement truncation if needed

        Line::from(spans)
    }

    /// Shorten filename to fit within max_len
    fn shorten_filename(filename: &str, max_len: usize) -> String {
        // Just get the file name, not the full path
        let name = filename
            .rsplit('/')
            .next()
            .unwrap_or(filename);

        if name.len() <= max_len {
            name.to_string()
        } else {
            format!("{}…", &name[..max_len - 1])
        }
    }
}
