//! TUI testing utilities using Ratatui's TestBackend
//!
//! Provides helpers for rendering UI components to a test buffer
//! and converting the output to strings for snapshot testing.

use ratatui::{backend::TestBackend, buffer::Buffer, layout::Rect, Terminal};

/// Create a test terminal with standard dimensions (80x24)
pub fn create_test_terminal() -> Terminal<TestBackend> {
    create_test_terminal_sized(80, 24)
}

/// Create a test terminal with custom dimensions
pub fn create_test_terminal_sized(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to create test terminal")
}

/// Convert a buffer to a string for snapshot testing
///
/// Preserves exact spacing and newlines for accurate comparison.
pub fn buffer_to_string(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut output = String::new();

    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                output.push_str(cell.symbol());
            }
        }
        output.push('\n');
    }

    output
}

/// Convert buffer to string, trimming trailing whitespace per line
///
/// This is more useful for snapshot comparisons where trailing
/// spaces are not meaningful.
pub fn buffer_to_trimmed_string(buffer: &Buffer) -> String {
    buffer_to_string(buffer)
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract a specific region of the buffer as a string
pub fn buffer_region_to_string(buffer: &Buffer, area: Rect) -> String {
    let mut output = String::new();

    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buffer.cell((x, y)) {
                output.push_str(cell.symbol());
            }
        }
        if y < area.y + area.height - 1 {
            output.push('\n');
        }
    }

    output
}

/// Assert that a specific region of the buffer contains expected text
pub fn assert_buffer_contains(buffer: &Buffer, area: Rect, expected: &str) {
    let actual = buffer_region_to_string(buffer, area);

    assert!(
        actual.contains(expected),
        "Buffer region does not contain expected text.\nExpected: {}\nActual:\n{}",
        expected,
        actual
    );
}

/// Assert that the buffer matches an expected string exactly
pub fn assert_buffer_eq(buffer: &Buffer, expected: &str) {
    let actual = buffer_to_trimmed_string(buffer);
    let expected_trimmed = expected
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        actual, expected_trimmed,
        "Buffer does not match expected content"
    );
}

/// Check if the buffer contains a string anywhere
pub fn buffer_contains(buffer: &Buffer, text: &str) -> bool {
    buffer_to_string(buffer).contains(text)
}

/// Get the character at a specific position
pub fn char_at(buffer: &Buffer, x: u16, y: u16) -> Option<&str> {
    buffer.cell((x, y)).map(|cell| cell.symbol())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::Paragraph;

    #[test]
    fn test_create_terminal() {
        let terminal = create_test_terminal();
        let size = terminal.size().unwrap();
        assert_eq!(size.width, 80);
        assert_eq!(size.height, 24);
    }

    #[test]
    fn test_create_terminal_sized() {
        let terminal = create_test_terminal_sized(100, 50);
        let size = terminal.size().unwrap();
        assert_eq!(size.width, 100);
        assert_eq!(size.height, 50);
    }

    #[test]
    fn test_buffer_to_string() {
        let mut terminal = create_test_terminal_sized(10, 3);
        terminal
            .draw(|f| {
                let para = Paragraph::new("Hello");
                f.render_widget(para, f.area());
            })
            .unwrap();

        let output = buffer_to_trimmed_string(terminal.backend().buffer());
        assert!(output.starts_with("Hello"));
    }

    #[test]
    fn test_buffer_contains() {
        let mut terminal = create_test_terminal_sized(20, 5);
        terminal
            .draw(|f| {
                let para = Paragraph::new("Test content here");
                f.render_widget(para, f.area());
            })
            .unwrap();

        assert!(buffer_contains(terminal.backend().buffer(), "content"));
        assert!(!buffer_contains(terminal.backend().buffer(), "missing"));
    }

    #[test]
    fn test_char_at() {
        let mut terminal = create_test_terminal_sized(10, 3);
        terminal
            .draw(|f| {
                let para = Paragraph::new("ABC");
                f.render_widget(para, f.area());
            })
            .unwrap();

        assert_eq!(char_at(terminal.backend().buffer(), 0, 0), Some("A"));
        assert_eq!(char_at(terminal.backend().buffer(), 1, 0), Some("B"));
        assert_eq!(char_at(terminal.backend().buffer(), 2, 0), Some("C"));
    }

    #[test]
    fn test_buffer_region_to_string() {
        let mut terminal = create_test_terminal_sized(20, 5);
        terminal
            .draw(|f| {
                let para = Paragraph::new("Line 1\nLine 2\nLine 3");
                f.render_widget(para, f.area());
            })
            .unwrap();

        // Extract just the first 6 characters of the first 2 lines
        let region = Rect::new(0, 0, 6, 2);
        let output = buffer_region_to_string(terminal.backend().buffer(), region);
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 2"));
        assert!(!output.contains("Line 3"));
    }

    #[test]
    fn test_assert_buffer_contains_success() {
        let mut terminal = create_test_terminal_sized(20, 5);
        terminal
            .draw(|f| {
                let para = Paragraph::new("Hello World");
                f.render_widget(para, f.area());
            })
            .unwrap();

        let region = Rect::new(0, 0, 20, 1);
        // This should not panic
        assert_buffer_contains(terminal.backend().buffer(), region, "Hello");
        assert_buffer_contains(terminal.backend().buffer(), region, "World");
    }

    #[test]
    fn test_assert_buffer_eq_success() {
        let mut terminal = create_test_terminal_sized(5, 1);
        terminal
            .draw(|f| {
                let para = Paragraph::new("Test");
                f.render_widget(para, f.area());
            })
            .unwrap();

        // Buffer will be "Test " (with trailing space to fill width)
        assert_buffer_eq(terminal.backend().buffer(), "Test");
    }
}
