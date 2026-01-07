//! Git status utilities for tracking diff statistics

use std::path::Path;
use std::process::Command;

/// Git diff statistics (additions, deletions, files changed)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitDiffStats {
    pub additions: usize,
    pub deletions: usize,
    pub files_changed: usize,
}

impl GitDiffStats {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.additions > 0 || self.deletions > 0
    }

    /// Get git diff stats for the current working directory
    /// Uses `git diff --shortstat` to get uncommitted changes
    pub fn from_working_dir(working_dir: &Path) -> Self {
        // Get stats for staged and unstaged changes
        let output = Command::new("git")
            .args(["diff", "--shortstat", "HEAD"])
            .current_dir(working_dir)
            .output();

        let stats = match output {
            Ok(o) if o.status.success() => {
                let output_str = String::from_utf8_lossy(&o.stdout);
                Self::parse_shortstat(&output_str)
            }
            _ => GitDiffStats::default(),
        };

        // If no staged changes, also check unstaged
        if stats == GitDiffStats::default() {
            let unstaged = Command::new("git")
                .args(["diff", "--shortstat"])
                .current_dir(working_dir)
                .output();

            if let Ok(o) = unstaged {
                if o.status.success() {
                    let output_str = String::from_utf8_lossy(&o.stdout);
                    return Self::parse_shortstat(&output_str);
                }
            }
        }

        stats
    }

    /// Parse from `git diff --shortstat` output
    /// Format: "1 file changed, 44 insertions(+), 10 deletions(-)"
    fn parse_shortstat(output: &str) -> Self {
        let mut stats = GitDiffStats::default();
        let output = output.trim();

        if output.is_empty() {
            return stats;
        }

        for part in output.split(',') {
            let part = part.trim();
            if part.contains("insertion") {
                stats.additions = part
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            } else if part.contains("deletion") {
                stats.deletions = part
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            } else if part.contains("file") {
                stats.files_changed = part
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            }
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shortstat_full() {
        let output = " 3 files changed, 44 insertions(+), 10 deletions(-)";
        let stats = GitDiffStats::parse_shortstat(output);
        assert_eq!(stats.files_changed, 3);
        assert_eq!(stats.additions, 44);
        assert_eq!(stats.deletions, 10);
    }

    #[test]
    fn test_parse_shortstat_insertions_only() {
        let output = " 1 file changed, 25 insertions(+)";
        let stats = GitDiffStats::parse_shortstat(output);
        assert_eq!(stats.files_changed, 1);
        assert_eq!(stats.additions, 25);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn test_parse_shortstat_deletions_only() {
        let output = " 2 files changed, 15 deletions(-)";
        let stats = GitDiffStats::parse_shortstat(output);
        assert_eq!(stats.files_changed, 2);
        assert_eq!(stats.additions, 0);
        assert_eq!(stats.deletions, 15);
    }

    #[test]
    fn test_parse_shortstat_empty() {
        let output = "";
        let stats = GitDiffStats::parse_shortstat(output);
        assert_eq!(stats.files_changed, 0);
        assert_eq!(stats.additions, 0);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn test_has_changes() {
        let empty = GitDiffStats::default();
        assert!(!empty.has_changes());

        let with_additions = GitDiffStats {
            additions: 10,
            deletions: 0,
            files_changed: 1,
        };
        assert!(with_additions.has_changes());

        let with_deletions = GitDiffStats {
            additions: 0,
            deletions: 5,
            files_changed: 1,
        };
        assert!(with_deletions.has_changes());
    }
}
