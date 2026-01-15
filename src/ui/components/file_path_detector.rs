//! File path detection in text
//!
//! This module provides functionality to detect and validate local file paths
//! in text content, enabling clickable file paths in chat messages.

use std::path::Path;

use regex::Regex;
use std::sync::LazyLock;

/// Represents a detected file path in text
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePathMatch {
    /// The detected file path string
    pub path: String,
    /// Byte offset where the path starts in the original text
    pub start: usize,
    /// Byte offset where the path ends in the original text
    pub end: usize,
}

impl FilePathMatch {
    /// Expand tilde to home directory
    pub fn expanded_path(&self) -> String {
        expand_tilde(&self.path)
    }

    /// Check if the path exists on the filesystem
    pub fn exists(&self) -> bool {
        path_exists(&self.path)
    }
}

// Regex pattern for detecting file paths
// Matches:
// - Absolute paths: /path/to/file
// - Home directory: ~/path/to/file
// - Relative paths: ./path or ../path
//
// Note: Rust's regex crate doesn't support look-behind, so we use a simpler pattern
// and filter false positives in code
static FILE_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // Captures file paths - the pattern allows alphanumeric, dots, underscores, hyphens, and slashes
    // We use word boundaries where possible and handle edge cases in code
    Regex::new(
        r"(/[a-zA-Z0-9._\-]+(?:/[a-zA-Z0-9._\-]+)*|~/[a-zA-Z0-9._\-]*(?:/[a-zA-Z0-9._\-]+)*|\.{1,2}/[a-zA-Z0-9._\-]+(?:/[a-zA-Z0-9._\-]+)*)"
    ).expect("Invalid regex pattern")
});

/// Detect file paths in text that exist on the filesystem
pub fn detect_existing_paths(text: &str) -> Vec<FilePathMatch> {
    detect_file_paths(text)
        .into_iter()
        .filter(|m| m.exists())
        .collect()
}

/// Detect all potential file paths in text (may not exist)
pub fn detect_file_paths(text: &str) -> Vec<FilePathMatch> {
    let mut matches = Vec::new();

    for cap in FILE_PATH_REGEX.find_iter(text) {
        let raw_path = cap.as_str();
        let start = cap.start();

        // Clean up the path - strip trailing punctuation that's likely sentence-ending
        let path = strip_trailing_punctuation(raw_path);
        let end = start + path.len();

        // Skip very short paths that are likely false positives
        if path.len() < 3 {
            continue;
        }

        // Skip common false positives
        if is_false_positive(path) {
            continue;
        }

        matches.push(FilePathMatch {
            path: path.to_string(),
            start,
            end,
        });
    }

    matches
}

/// Strip trailing punctuation from a path
fn strip_trailing_punctuation(path: &str) -> &str {
    path.trim_end_matches(['.', ',', ':', ';', '!', '?', ')', ']', '}', '"', '\''])
}

/// Check if a detected path is likely a false positive
fn is_false_positive(path: &str) -> bool {
    // Skip common protocol-like patterns
    if path.starts_with("//") {
        return true;
    }

    // Skip paths that are just punctuation or very generic
    let stripped = path.trim_start_matches(['.', '/', '~']);
    if stripped.is_empty() || stripped == "." || stripped == ".." {
        return true;
    }

    // Skip common abbreviations that look like paths
    let lower = path.to_lowercase();
    let false_positives = [
        "/etc", // too common
        "/a", "/b", "/c", "/d", "/e", // single letter paths
        "i/o", "/o", // I/O pattern
    ];
    if false_positives.contains(&lower.as_str()) {
        return true;
    }

    false
}

/// Expand tilde (~) to the user's home directory
pub fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return path.replacen('~', &home.display().to_string(), 1);
        }
    }
    path.to_string()
}

/// Check if a path exists on the filesystem
pub fn path_exists(path: &str) -> bool {
    let expanded = expand_tilde(path);
    Path::new(&expanded).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_absolute_path() {
        let text = "Check the file at /Users/test/file.txt for details";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "/Users/test/file.txt");
    }

    #[test]
    fn test_detect_tilde_path() {
        let text = "The config is at ~/.config/app/settings.json";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "~/.config/app/settings.json");
    }

    #[test]
    fn test_detect_relative_path() {
        let text = "Look at ./src/main.rs for the entry point";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "./src/main.rs");
    }

    #[test]
    fn test_detect_parent_relative_path() {
        let text = "The file ../tests/test.rs contains tests";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "../tests/test.rs");
    }

    #[test]
    fn test_multiple_paths() {
        let text = "Compare /path/one.txt with /path/two.txt";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].path, "/path/one.txt");
        assert_eq!(matches[1].path, "/path/two.txt");
    }

    #[test]
    fn test_strip_trailing_punctuation() {
        let text = "See /path/to/file.txt.";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "/path/to/file.txt");
    }

    #[test]
    fn test_path_with_dots_in_filename() {
        let text = "Check /home/user/.bashrc";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "/home/user/.bashrc");
    }

    #[test]
    fn test_skip_io_pattern() {
        let text = "The I/O performance is good";
        let matches = detect_file_paths(text);

        // Should not detect "/O" as a path
        assert!(matches.is_empty() || matches.iter().all(|m| m.path != "/O"));
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde("~/test");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.contains("test"));
    }

    #[test]
    fn test_no_tilde_expansion_for_absolute() {
        let path = "/absolute/path";
        let expanded = expand_tilde(path);
        assert_eq!(path, expanded);
    }

    #[test]
    fn test_path_exists_for_nonexistent() {
        assert!(!path_exists("/nonexistent/path/xyz123"));
    }

    #[test]
    #[cfg(unix)]
    fn test_path_exists_for_root() {
        // Root directory should exist on Unix systems
        assert!(path_exists("/"));
    }

    #[test]
    fn test_detect_claude_plan_path() {
        let text = "Check the plan at /Users/fcoury/.claude/plans/my-plan.md";
        let matches = detect_file_paths(text);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "/Users/fcoury/.claude/plans/my-plan.md");
    }
}
