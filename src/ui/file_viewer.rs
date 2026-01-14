//! File viewer session for displaying local files in tabs
//!
//! This module provides the FileViewerSession struct which holds the state
//! for viewing a local file in a tab.

use std::path::PathBuf;

use uuid::Uuid;

/// State for a file viewer tab
#[derive(Debug)]
pub struct FileViewerSession {
    /// Unique identifier for this session
    pub id: Uuid,
    /// Path to the file being viewed
    pub file_path: PathBuf,
    /// Raw file content
    content: String,
    /// Lines of the file (for display)
    lines: Vec<String>,
    /// Total number of lines
    pub total_lines: usize,
    /// Current scroll offset (in lines)
    pub scroll_offset: usize,
    /// Whether to show line numbers
    pub show_line_numbers: bool,
    /// Whether this is a markdown file
    pub is_markdown: bool,
}

impl FileViewerSession {
    /// Create a new file viewer session by reading a file
    pub fn new(file_path: PathBuf) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(&file_path)?;
        let is_markdown = file_path
            .extension()
            .map(|ext| ext == "md" || ext == "markdown")
            .unwrap_or(false);

        let lines: Vec<String> = content.lines().map(String::from).collect();
        let total_lines = lines.len();

        Ok(Self {
            id: Uuid::new_v4(),
            file_path,
            content,
            lines,
            total_lines,
            scroll_offset: 0,
            show_line_numbers: true,
            is_markdown,
        })
    }

    /// Get display name for the tab (filename only)
    pub fn tab_name(&self) -> String {
        self.file_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)
            .unwrap_or_else(|| "File".to_string())
    }

    /// Get the full file path as a string
    pub fn file_path_display(&self) -> String {
        self.file_path.display().to_string()
    }

    /// Get the raw content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Scroll up by N lines
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Scroll down by N lines
    /// Note: For proper clamping, use scroll_down_clamped with visible height
    pub fn scroll_down(&mut self, lines: usize) {
        // Don't scroll past end - keep at least one line visible
        let max_scroll = self.total_lines.saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    /// Scroll down by N lines, clamped to visible height
    pub fn scroll_down_clamped(&mut self, lines: usize, visible_height: usize) {
        let max_scroll = self.total_lines.saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    /// Scroll to top of file
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll to bottom of file (uses default visible height approximation)
    pub fn scroll_to_bottom(&mut self) {
        // Default to assuming ~30 lines visible
        let default_visible = 30;
        self.scroll_offset = self.total_lines.saturating_sub(default_visible);
    }

    /// Scroll to bottom with exact visible height
    pub fn scroll_to_bottom_exact(&mut self, visible_height: usize) {
        self.scroll_offset = self.total_lines.saturating_sub(visible_height);
    }

    /// Page up (scroll by default page size)
    pub fn page_up(&mut self) {
        let page_size = 20; // Default page size
        self.scroll_up(page_size);
    }

    /// Page up with exact visible height
    pub fn page_up_exact(&mut self, visible_height: usize) {
        let page_size = visible_height.saturating_sub(2);
        self.scroll_up(page_size);
    }

    /// Page down (scroll by default page size)
    pub fn page_down(&mut self) {
        let page_size = 20; // Default page size
        self.scroll_down(page_size);
    }

    /// Page down with exact visible height
    pub fn page_down_exact(&mut self, visible_height: usize) {
        let page_size = visible_height.saturating_sub(2);
        self.scroll_down_clamped(page_size, visible_height);
    }

    /// Get visible lines for rendering
    pub fn visible_lines(&self, visible_height: usize) -> &[String] {
        let start = self.scroll_offset;
        let end = (start + visible_height).min(self.lines.len());
        &self.lines[start..end]
    }

    /// Get all lines
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Toggle line numbers display
    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
    }

    /// Reload the file from disk
    pub fn reload(&mut self) -> std::io::Result<()> {
        let content = std::fs::read_to_string(&self.file_path)?;
        self.lines = content.lines().map(String::from).collect();
        self.total_lines = self.lines.len();
        self.content = content;

        // Clamp scroll offset if file got shorter
        if self.scroll_offset >= self.total_lines {
            self.scroll_offset = self.total_lines.saturating_sub(1);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_viewer_session_new() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "Line 1").unwrap();
        writeln!(file, "Line 2").unwrap();
        writeln!(file, "Line 3").unwrap();

        let session = FileViewerSession::new(file.path().to_path_buf()).unwrap();
        assert_eq!(session.total_lines, 3);
        assert_eq!(session.scroll_offset, 0);
        assert!(session.show_line_numbers);
    }

    #[test]
    fn test_scroll_operations() {
        let mut file = NamedTempFile::new().unwrap();
        for i in 1..=100 {
            writeln!(file, "Line {}", i).unwrap();
        }

        let mut session = FileViewerSession::new(file.path().to_path_buf()).unwrap();
        let visible_height = 20;

        // Scroll down (using clamped version for exact behavior)
        session.scroll_down_clamped(10, visible_height);
        assert_eq!(session.scroll_offset, 10);

        // Scroll up
        session.scroll_up(5);
        assert_eq!(session.scroll_offset, 5);

        // Scroll to bottom (using exact version)
        session.scroll_to_bottom_exact(visible_height);
        assert_eq!(session.scroll_offset, 80); // 100 - 20

        // Scroll to top
        session.scroll_to_top();
        assert_eq!(session.scroll_offset, 0);
    }

    #[test]
    fn test_markdown_detection() {
        let mut file = NamedTempFile::with_suffix(".md").unwrap();
        writeln!(file, "# Markdown").unwrap();

        let session = FileViewerSession::new(file.path().to_path_buf()).unwrap();
        assert!(session.is_markdown);
    }

    #[test]
    fn test_tab_name() {
        let mut file = NamedTempFile::with_suffix(".txt").unwrap();
        writeln!(file, "content").unwrap();

        let session = FileViewerSession::new(file.path().to_path_buf()).unwrap();
        let name = session.tab_name();
        assert!(name.ends_with(".txt"));
    }
}
