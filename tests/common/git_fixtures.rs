//! Git repository test fixtures
//!
//! Provides utilities for creating temporary git repositories
//! in various states for testing git operations.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// A temporary git repository for testing
///
/// The repository is automatically cleaned up when the `TestRepo`
/// is dropped, including any worktrees created through this struct.
/// Use the various constructors to create repos in different initial states.
///
/// # Example
/// ```
/// let repo = TestRepo::new();
/// assert!(repo.path.join(".git").exists());
/// ```
pub struct TestRepo {
    /// TempDir handle (keeps directory alive until dropped)
    _dir: TempDir,
    /// Path to the repository root
    pub path: PathBuf,
    /// Tracks worktree paths for cleanup on drop
    worktrees: Arc<Mutex<Vec<PathBuf>>>,
}

impl TestRepo {
    /// Create a new test repository with an initial commit
    ///
    /// The repository will have:
    /// - Git initialized
    /// - User configured (test@example.com)
    /// - GPG signing disabled (for CI compatibility)
    /// - A README.md file
    /// - One initial commit
    pub fn new() -> Self {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = dir.path().to_path_buf();

        Self::git(&path, &["init"]);
        Self::git(&path, &["config", "user.email", "test@example.com"]);
        Self::git(&path, &["config", "user.name", "Test User"]);
        // Disable GPG signing to ensure tests work on machines with global signing enabled
        Self::git(&path, &["config", "commit.gpgsign", "false"]);

        // Create initial commit
        std::fs::write(path.join("README.md"), "# Test Repository\n").unwrap();
        Self::git(&path, &["add", "."]);
        Self::git(&path, &["commit", "-m", "Initial commit"]);

        Self {
            _dir: dir,
            path,
            worktrees: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a repository with multiple branches
    ///
    /// All branches are created pointing at the initial commit.
    pub fn with_branches(branch_names: &[&str]) -> Self {
        let repo = Self::new();
        for branch in branch_names {
            Self::git(&repo.path, &["branch", branch]);
        }
        repo
    }

    /// Create a repository with uncommitted (untracked) changes
    pub fn with_uncommitted_changes() -> Self {
        let repo = Self::new();
        std::fs::write(repo.path.join("dirty.txt"), "uncommitted content").unwrap();
        repo
    }

    /// Create a repository with staged but uncommitted changes
    pub fn with_staged_changes() -> Self {
        let repo = Self::new();
        std::fs::write(repo.path.join("staged.txt"), "staged content").unwrap();
        Self::git(&repo.path, &["add", "staged.txt"]);
        repo
    }

    /// Create a repository with both staged and unstaged changes
    pub fn with_mixed_changes() -> Self {
        let repo = Self::new();
        // Staged change
        std::fs::write(repo.path.join("staged.txt"), "staged content").unwrap();
        Self::git(&repo.path, &["add", "staged.txt"]);
        // Unstaged change
        std::fs::write(repo.path.join("unstaged.txt"), "unstaged content").unwrap();
        repo
    }

    /// Create a repository with a worktree already set up
    ///
    /// Creates the main repo and a worktree. The worktree is tracked
    /// and will be cleaned up when the TestRepo is dropped.
    pub fn with_worktree(worktree_name: &str, branch_name: &str) -> (Self, PathBuf) {
        let repo = Self::new();

        // Create the branch first
        Self::git(&repo.path, &["branch", branch_name]);

        // Create worktree directory path (sibling to main repo, inside the temp dir)
        let worktree_path = repo.path.parent().unwrap().join(worktree_name);

        Self::git(
            &repo.path,
            &[
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                branch_name,
            ],
        );

        // Track the worktree for cleanup
        repo.worktrees.lock().unwrap().push(worktree_path.clone());

        (repo, worktree_path)
    }

    /// Create a worktree and track it for cleanup
    ///
    /// Use this method when creating worktrees in integration tests
    /// to ensure they are properly cleaned up.
    #[allow(dead_code)] // Used in integration tests via #[path] includes
    pub fn create_tracked_worktree(&self, worktree_path: &Path, branch_name: &str) {
        // Check if branch exists
        let branches = self.branches();
        let branch_exists = branches.iter().any(|b| b == branch_name);

        if branch_exists {
            Self::git(
                &self.path,
                &[
                    "worktree",
                    "add",
                    worktree_path.to_str().unwrap(),
                    branch_name,
                ],
            );
        } else {
            // Create new branch with worktree
            Self::git(
                &self.path,
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch_name,
                    worktree_path.to_str().unwrap(),
                ],
            );
        }

        // Track for cleanup
        self.worktrees
            .lock()
            .unwrap()
            .push(worktree_path.to_path_buf());
    }

    /// Create a repository with commit history
    ///
    /// Creates the specified number of commits after the initial one.
    pub fn with_history(commit_count: usize) -> Self {
        let repo = Self::new();
        for i in 0..commit_count {
            let filename = format!("file_{}.txt", i);
            std::fs::write(repo.path.join(&filename), format!("Content {}", i)).unwrap();
            Self::git(&repo.path, &["add", &filename]);
            Self::git(&repo.path, &["commit", "-m", &format!("Commit {}", i + 1)]);
        }
        repo
    }

    /// Add a file and commit it
    pub fn commit_file(&self, filename: &str, content: &str, message: &str) {
        std::fs::write(self.path.join(filename), content).unwrap();
        Self::git(&self.path, &["add", filename]);
        Self::git(&self.path, &["commit", "-m", message]);
    }

    /// Create a file without staging or committing
    /// Creates parent directories if they don't exist.
    pub fn create_file(&self, filename: &str, content: &str) {
        let file_path = self.path.join(filename);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(file_path, content).unwrap();
    }

    /// Stage a file without committing
    pub fn stage_file(&self, filename: &str) {
        Self::git(&self.path, &["add", filename]);
    }

    /// Checkout a branch
    pub fn checkout(&self, branch: &str) {
        Self::git(&self.path, &["checkout", branch]);
    }

    /// Create and checkout a new branch
    pub fn checkout_new_branch(&self, branch: &str) {
        Self::git(&self.path, &["checkout", "-b", branch]);
    }

    /// Get current branch name
    pub fn current_branch(&self) -> String {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to get branch");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Check if the repository has uncommitted changes
    pub fn is_dirty(&self) -> bool {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to get status");
        !output.stdout.is_empty()
    }

    /// Get the number of uncommitted changes
    pub fn uncommitted_count(&self) -> usize {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to get status");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .count()
    }

    /// Get list of all branches
    pub fn branches(&self) -> Vec<String> {
        let output = Command::new("git")
            .args(["branch", "--list", "--format=%(refname:short)"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to list branches");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect()
    }

    /// Get the HEAD commit SHA
    pub fn head_sha(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.path)
            .output()
            .expect("Failed to get HEAD");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Set up a remote URL for the repository
    #[allow(dead_code)] // Used in integration tests via #[path] includes
    pub fn set_remote(&self, name: &str, url: &str) {
        Self::git(&self.path, &["remote", "add", name, url]);
    }

    /// Execute a git command in the repository
    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap_or_else(|e| panic!("Git command failed to execute: {}", e));

        if !output.status.success() {
            panic!(
                "Git command failed: git {}\nstderr: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    /// Execute a git command and return output (for queries)
    pub fn git_output(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("Git command failed");

        if !output.status.success() {
            panic!(
                "Git command failed: git {}\nstderr: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        String::from_utf8_lossy(&output.stdout).to_string()
    }
}

impl Default for TestRepo {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        // Clean up worktrees before the temp dir is removed
        let worktree_paths = {
            let worktrees = self.worktrees.lock().unwrap();
            worktrees.clone()
        };
        for worktree_path in worktree_paths.iter() {
            // First, remove the worktree from git's tracking
            let Some(worktree_str) = worktree_path.to_str() else {
                eprintln!(
                    "Warning: skipping git worktree remove for non-UTF8 path: {}",
                    worktree_path.display()
                );
                continue;
            };
            match Command::new("git")
                .args(["worktree", "remove", "--force", worktree_str])
                .current_dir(&self.path)
                .output()
            {
                Ok(output) => {
                    if !output.status.success() {
                        eprintln!(
                            "Warning: git worktree remove failed for {}: {}",
                            worktree_path.display(),
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Warning: failed to run git worktree remove for {}: {}",
                        worktree_path.display(),
                        e
                    );
                }
            }

            // Then remove the directory if it still exists
            if worktree_path.exists() {
                if let Err(e) = std::fs::remove_dir_all(worktree_path) {
                    eprintln!(
                        "Warning: failed to remove worktree directory {}: {}",
                        worktree_path.display(),
                        e
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_creation() {
        let repo = TestRepo::new();
        assert!(repo.path.join(".git").exists());
        assert!(repo.path.join("README.md").exists());
    }

    #[test]
    fn test_repo_with_branches() {
        let repo = TestRepo::with_branches(&["feature-1", "feature-2"]);

        let branches = repo.branches();
        assert!(branches.iter().any(|b| b.contains("feature-1")));
        assert!(branches.iter().any(|b| b.contains("feature-2")));
    }

    #[test]
    fn test_repo_with_uncommitted() {
        let repo = TestRepo::with_uncommitted_changes();
        assert!(repo.is_dirty());
        assert_eq!(repo.uncommitted_count(), 1);
    }

    #[test]
    fn test_repo_with_staged() {
        let repo = TestRepo::with_staged_changes();
        assert!(repo.is_dirty());
    }

    #[test]
    fn test_commit_file() {
        let repo = TestRepo::new();
        let initial_sha = repo.head_sha();

        repo.commit_file("test.txt", "test content", "Add test file");

        let new_sha = repo.head_sha();
        assert_ne!(initial_sha, new_sha);
        assert!(repo.path.join("test.txt").exists());
    }

    #[test]
    fn test_checkout_new_branch() {
        let repo = TestRepo::new();
        repo.checkout_new_branch("feature/test");

        assert_eq!(repo.current_branch(), "feature/test");
    }

    #[test]
    fn test_with_history() {
        let repo = TestRepo::with_history(3);

        // Should have initial commit + 3 more = 4 total
        let log_output = repo.git_output(&["log", "--oneline"]);
        let commit_count = log_output.lines().count();
        assert_eq!(commit_count, 4);
    }

    #[test]
    fn test_with_mixed_changes() {
        let repo = TestRepo::with_mixed_changes();

        assert!(repo.is_dirty());
        assert_eq!(repo.uncommitted_count(), 2); // 1 staged + 1 unstaged
        assert!(repo.path.join("staged.txt").exists());
        assert!(repo.path.join("unstaged.txt").exists());
    }

    #[test]
    fn test_with_worktree() {
        let unique_id = uuid::Uuid::new_v4().as_simple().to_string();
        let wt_name = format!("test-wt-{}", &unique_id[..8]);

        let (repo, worktree_path) = TestRepo::with_worktree(&wt_name, "feature-branch");

        assert!(worktree_path.exists(), "Worktree should exist");
        assert!(
            worktree_path.join(".git").exists(),
            "Worktree should have .git"
        );

        // Verify branch exists
        let branches = repo.branches();
        assert!(branches.iter().any(|b| b == "feature-branch"));
    }

    #[test]
    fn test_create_file_with_nested_path() {
        let repo = TestRepo::new();

        // Create file in nested directory (should auto-create parent dirs)
        repo.create_file("src/nested/deep/file.rs", "fn main() {}");

        assert!(repo.path.join("src/nested/deep/file.rs").exists());
        assert!(repo.is_dirty()); // File is untracked
    }

    #[test]
    fn test_stage_file() {
        let repo = TestRepo::new();

        // Create and stage a file
        repo.create_file("new_file.txt", "content");
        repo.stage_file("new_file.txt");

        // Check that file is staged (will show as 'A' in porcelain output)
        let output = repo.git_output(&["status", "--porcelain"]);
        assert!(output.contains("A  new_file.txt") || output.contains("A new_file.txt"));
    }

    #[test]
    fn test_checkout_existing_branch() {
        let repo = TestRepo::with_branches(&["develop"]);

        // Save initial branch (whatever git's default is - typically main or master)
        let initial_branch = repo.current_branch();
        assert!(
            !initial_branch.is_empty(),
            "Should have a default branch after init"
        );

        // Checkout existing branch
        repo.checkout("develop");
        assert_eq!(repo.current_branch(), "develop");

        // Can checkout back to initial branch
        repo.checkout(&initial_branch);
        assert_eq!(repo.current_branch(), initial_branch);
    }
}
