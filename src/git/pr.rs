//! Pull Request management utilities
//!
//! This module provides preflight checks and prompt generation for PR creation.
//! The actual git/gh commands are executed by Claude Sonnet.

use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Clone, Copy)]
pub struct GhStatus {
    pub installed: bool,
    pub authenticated: bool,
}

struct GhStatusCache {
    checked_at: Instant,
    status: GhStatus,
}

const GH_STATUS_TTL: Duration = Duration::from_secs(30);
static GH_STATUS_CACHE: OnceLock<Mutex<GhStatusCache>> = OnceLock::new();

fn refresh_gh_status() -> GhStatus {
    let installed = match Command::new("gh").arg("--version").output() {
        Ok(output) => output.status.success(),
        Err(error) => {
            warn!(error = %error, "Failed to run gh --version");
            false
        }
    };

    let authenticated = if installed {
        match Command::new("gh").args(["auth", "status"]).output() {
            Ok(output) => output.status.success(),
            Err(error) => {
                warn!(error = %error, "Failed to run gh auth status");
                false
            }
        }
    } else {
        false
    };

    GhStatus {
        installed,
        authenticated,
    }
}

fn gh_status_cached() -> GhStatus {
    let now = Instant::now();
    let cache = GH_STATUS_CACHE.get_or_init(|| {
        let checked_at = now.checked_sub(GH_STATUS_TTL).unwrap_or(now);
        Mutex::new(GhStatusCache {
            checked_at,
            status: GhStatus {
                installed: false,
                authenticated: false,
            },
        })
    });

    let mut guard = match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            return guard.status;
        }
    };

    if now.duration_since(guard.checked_at) >= GH_STATUS_TTL {
        guard.status = refresh_gh_status();
        guard.checked_at = now;
    }

    guard.status
}

/// PR state matching GitHub's actual states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrState {
    #[default]
    Unknown,
    Open,
    Merged,
    Closed,
    Draft,
}

/// CI check state summary
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckState {
    #[default]
    None, // No checks configured
    Pending, // Checks in progress
    Passing, // All checks passed
    Failing, // One or more checks failed
}

/// CI check status with counts
#[derive(Debug, Clone, Default)]
pub struct CheckStatus {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
    pub skipped: usize,
}

impl CheckStatus {
    /// Get the overall check state
    pub fn state(&self) -> CheckState {
        if self.total == 0 {
            CheckState::None
        } else if self.failed > 0 {
            CheckState::Failing
        } else if self.pending > 0 {
            CheckState::Pending
        } else if self.passed > 0 {
            CheckState::Passing
        } else {
            CheckState::None
        }
    }

    /// Parse from gh pr view statusCheckRollup
    /// Handles both CheckRun (status/conclusion) and StatusContext (state) entries
    fn from_status_checks(checks: &[GhStatusCheck]) -> Self {
        let mut passed = 0;
        let mut failed = 0;
        let mut pending = 0;
        let mut skipped = 0;

        for check in checks {
            // Check if this is a StatusContext (uses state field) vs CheckRun (uses status/conclusion)
            if !check.state.is_empty() {
                // StatusContext: uses state field directly
                match check.state.to_uppercase().as_str() {
                    "SUCCESS" => passed += 1,
                    "PENDING" | "EXPECTED" => pending += 1,
                    "FAILURE" | "ERROR" => failed += 1,
                    _ => pending += 1,
                }
            } else {
                // CheckRun: uses status/conclusion fields
                match check.status.to_uppercase().as_str() {
                    "COMPLETED" => match check.conclusion.to_uppercase().as_str() {
                        "SUCCESS" | "NEUTRAL" => passed += 1,
                        "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" => failed += 1,
                        "SKIPPED" => skipped += 1,
                        _ => skipped += 1, // Unknown conclusions treated as skipped
                    },
                    _ => pending += 1,
                }
            }
        }

        Self {
            total: checks.len(),
            passed,
            failed,
            pending,
            skipped,
        }
    }
}

/// Merge conflict status from GitHub
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeableStatus {
    #[default]
    Unknown,
    Mergeable,
    Conflicting,
}

impl MergeableStatus {
    fn from_gh_json(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "MERGEABLE" => Self::Mergeable,
            "CONFLICTING" => Self::Conflicting,
            _ => Self::Unknown,
        }
    }
}

/// Review decision status from GitHub
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewDecision {
    #[default]
    None,
    Approved,
    ReviewRequired,
    ChangesRequested,
}

impl ReviewDecision {
    fn from_gh_json(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "APPROVED" => Self::Approved,
            "REVIEW_REQUIRED" => Self::ReviewRequired,
            "CHANGES_REQUESTED" => Self::ChangesRequested,
            _ => Self::None,
        }
    }
}

/// Overall merge readiness
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeReadiness {
    #[default]
    Unknown,
    Ready,        // Checks pass + no conflicts + approved
    Blocked,      // Failing checks or missing approval
    HasConflicts, // Merge conflicts present
}

impl MergeReadiness {
    fn compute(checks: &CheckStatus, mergeable: MergeableStatus, review: ReviewDecision) -> Self {
        // Conflicts take priority
        if mergeable == MergeableStatus::Conflicting {
            return Self::HasConflicts;
        }

        let checks_ok = matches!(checks.state(), CheckState::Passing | CheckState::None);
        let review_ok = matches!(review, ReviewDecision::Approved | ReviewDecision::None);

        if checks_ok && review_ok && mergeable == MergeableStatus::Mergeable {
            Self::Ready
        } else if mergeable == MergeableStatus::Unknown {
            Self::Unknown
        } else {
            Self::Blocked
        }
    }
}

impl PrState {
    /// Parse from gh pr view JSON output
    pub fn from_gh_json(state: &str, is_draft: bool, merged_at: Option<&str>) -> Self {
        if merged_at.is_some() {
            PrState::Merged
        } else if is_draft {
            PrState::Draft
        } else {
            match state.to_uppercase().as_str() {
                "OPEN" => PrState::Open,
                "CLOSED" => PrState::Closed,
                "MERGED" => PrState::Merged,
                _ => PrState::Unknown,
            }
        }
    }
}

/// PR status for a branch
#[derive(Debug, Clone, Default)]
pub struct PrStatus {
    pub exists: bool,
    pub number: Option<u32>,
    pub url: Option<String>,
    pub state: PrState,
    pub title: Option<String>,
    /// CI check status
    pub checks: CheckStatus,
    /// Merge conflict status
    pub mergeable: MergeableStatus,
    /// Review decision
    pub review_decision: ReviewDecision,
    /// Overall merge readiness
    pub merge_readiness: MergeReadiness,
}

/// Result of preflight checks before PR creation
#[derive(Debug, Clone)]
pub struct PrPreflightResult {
    pub gh_installed: bool,
    pub gh_authenticated: bool,
    pub on_main_branch: bool,
    pub branch_name: String,
    pub target_branch: String,
    pub uncommitted_count: usize,
    pub has_upstream: bool,
    pub existing_pr: Option<PrStatus>,
}

/// JSON structure for a single status check from statusCheckRollup
/// Can be either a CheckRun (uses status/conclusion) or a StatusContext (uses state)
///
/// Type discrimination: StatusContext entries have a non-empty `state` field,
/// while CheckRun entries use `status`/`conclusion` with an empty `state`.
#[derive(Debug, Deserialize)]
struct GhStatusCheck {
    #[serde(default)]
    status: String, // "COMPLETED", "IN_PROGRESS", "QUEUED" (for CheckRun)
    #[serde(default)]
    conclusion: String, // "SUCCESS", "FAILURE", "SKIPPED", "" (for CheckRun)
    #[serde(default)]
    state: String, // "SUCCESS", "PENDING", "EXPECTED", "FAILURE", "ERROR" (for StatusContext)
}

/// JSON structure returned by `gh pr view --json`
#[derive(Debug, Deserialize)]
struct GhPrView {
    number: u32,
    url: String,
    state: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    title: String,
    /// CI status checks (can be CheckRun or StatusContext entries)
    #[serde(rename = "statusCheckRollup", default)]
    status_check_rollup: Vec<GhStatusCheck>,
    /// Merge conflict status: "MERGEABLE", "CONFLICTING", "UNKNOWN"
    #[serde(default)]
    mergeable: String,
    /// Review decision: "APPROVED", "REVIEW_REQUIRED", "CHANGES_REQUESTED", ""
    #[serde(rename = "reviewDecision", default)]
    review_decision: String,
}

/// PR Manager for preflight checks and utilities
pub struct PrManager;

impl PrManager {
    /// Cached GitHub CLI status (installed/authenticated).
    pub fn gh_status() -> GhStatus {
        gh_status_cached()
    }

    /// Get repository name from git remote URL or directory name
    pub fn get_repo_name(working_dir: &Path) -> Option<String> {
        // Try git remote origin URL first
        let output = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(working_dir)
            .output()
            .ok()?;

        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(name) = Self::parse_repo_name_from_url(&url) {
                return Some(name);
            }
        }

        // Fallback to directory name
        working_dir.file_name()?.to_str().map(String::from)
    }

    /// Parse repository name from git remote URL
    /// Handles HTTPS (github.com/user/repo.git) and SSH (git@github.com:user/repo.git) formats
    fn parse_repo_name_from_url(url: &str) -> Option<String> {
        // Remove .git suffix if present
        let url = url.strip_suffix(".git").unwrap_or(url);

        // Try HTTPS format: https://github.com/user/repo
        if let Some(path) = url.strip_prefix("https://") {
            return path.split('/').next_back().map(String::from);
        }

        // Try SSH format: git@github.com:user/repo
        if url.starts_with("git@") {
            if let Some(path) = url.split(':').nth(1) {
                return path.split('/').next_back().map(String::from);
            }
        }

        // Fallback: just take the last path component
        url.split('/').next_back().map(String::from)
    }

    /// Check if GitHub CLI (gh) is installed
    pub fn is_gh_installed() -> bool {
        gh_status_cached().installed
    }

    /// Check if gh is authenticated
    pub fn is_gh_authenticated() -> bool {
        gh_status_cached().authenticated
    }

    /// Get current branch name
    pub fn get_current_branch(working_dir: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(working_dir)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if branch.is_empty() {
            // Detached HEAD
            Some("(detached)".to_string())
        } else {
            Some(branch)
        }
    }

    /// Check if branch is a main branch (main, master, develop)
    pub fn is_main_branch(branch: &str) -> bool {
        matches!(branch, "main" | "master" | "develop")
    }

    /// Get default branch from remote
    pub fn get_default_branch(working_dir: &Path) -> String {
        // Try to get the default branch from origin
        let output = Command::new("git")
            .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
            .current_dir(working_dir)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // Remove "origin/" prefix if present
                return branch
                    .strip_prefix("origin/")
                    .unwrap_or(&branch)
                    .to_string();
            }
        }

        // Fallback: check if main or master exists
        let check_main = Command::new("git")
            .args(["rev-parse", "--verify", "origin/main"])
            .current_dir(working_dir)
            .output();

        if check_main.map(|o| o.status.success()).unwrap_or(false) {
            return "main".to_string();
        }

        "master".to_string()
    }

    /// Check if current branch has an upstream
    pub fn has_upstream(working_dir: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "@{u}"])
            .current_dir(working_dir)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Count uncommitted changes (staged + unstaged + untracked)
    pub fn count_uncommitted_changes(working_dir: &Path) -> usize {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(working_dir)
            .output();

        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|line| !line.is_empty())
                .count(),
            _ => 0,
        }
    }

    /// Check if a PR exists for the current branch
    pub fn get_existing_pr(working_dir: &Path) -> Option<PrStatus> {
        let output = Command::new("gh")
            .args([
                "pr",
                "view",
                "--json",
                "number,url,state,isDraft,mergedAt,title,statusCheckRollup,mergeable,reviewDecision",
            ])
            .current_dir(working_dir)
            .output()
            .ok()?;

        if !output.status.success() {
            // No PR exists for this branch
            return Some(PrStatus {
                exists: false,
                number: None,
                url: None,
                state: PrState::Unknown,
                title: None,
                checks: CheckStatus::default(),
                mergeable: MergeableStatus::Unknown,
                review_decision: ReviewDecision::None,
                merge_readiness: MergeReadiness::Unknown,
            });
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        if let Ok(pr) = serde_json::from_str::<GhPrView>(&json_str) {
            let state = PrState::from_gh_json(&pr.state, pr.is_draft, pr.merged_at.as_deref());
            let checks = CheckStatus::from_status_checks(&pr.status_check_rollup);
            let mergeable = MergeableStatus::from_gh_json(&pr.mergeable);
            let review_decision = ReviewDecision::from_gh_json(&pr.review_decision);
            let merge_readiness = MergeReadiness::compute(&checks, mergeable, review_decision);

            tracing::debug!(
                pr_number = pr.number,
                pr_state = ?state,
                checks_total = checks.total,
                checks_passed = checks.passed,
                checks_pending = checks.pending,
                checks_failed = checks.failed,
                check_state = ?checks.state(),
                mergeable = ?mergeable,
                review_decision = ?review_decision,
                merge_readiness = ?merge_readiness,
                "PR status fetched"
            );

            Some(PrStatus {
                exists: true,
                number: Some(pr.number),
                url: Some(pr.url),
                state,
                title: Some(pr.title),
                checks,
                mergeable,
                review_decision,
                merge_readiness,
            })
        } else {
            // JSON parse failed - gh succeeded but returned unexpected format
            tracing::warn!("Failed to parse gh pr view JSON: {}", json_str);
            Some(PrStatus {
                exists: false,
                number: None,
                url: None,
                state: PrState::Unknown,
                title: None,
                checks: CheckStatus::default(),
                mergeable: MergeableStatus::Unknown,
                review_decision: ReviewDecision::None,
                merge_readiness: MergeReadiness::Unknown,
            })
        }
    }

    /// Run all preflight checks
    pub fn preflight_check(working_dir: &Path) -> PrPreflightResult {
        let gh_installed = Self::is_gh_installed();
        let gh_authenticated = if gh_installed {
            Self::is_gh_authenticated()
        } else {
            false
        };

        let branch_name = Self::get_current_branch(working_dir).unwrap_or_default();
        let on_main_branch = Self::is_main_branch(&branch_name);
        let target_branch = format!("origin/{}", Self::get_default_branch(working_dir));
        let uncommitted_count = Self::count_uncommitted_changes(working_dir);
        let has_upstream = Self::has_upstream(working_dir);

        let existing_pr = if gh_installed && gh_authenticated && !on_main_branch {
            Self::get_existing_pr(working_dir)
        } else {
            None
        };

        PrPreflightResult {
            gh_installed,
            gh_authenticated,
            on_main_branch,
            branch_name,
            target_branch,
            uncommitted_count,
            has_upstream,
            existing_pr,
        }
    }

    /// Open existing PR in browser
    pub fn open_pr_in_browser(working_dir: &Path) -> std::io::Result<()> {
        use std::process::Stdio;
        // Suppress stdout/stderr to prevent TUI corruption
        Command::new("gh")
            .args(["pr", "view", "--web"])
            .current_dir(working_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }

    /// Generate the prompt for Claude Sonnet to create a PR
    pub fn generate_pr_prompt(preflight: &PrPreflightResult) -> String {
        let upstream_note = if preflight.has_upstream {
            String::new()
        } else {
            "There is no upstream branch yet.\n".to_string()
        };

        let base_branch = preflight
            .target_branch
            .strip_prefix("origin/")
            .unwrap_or(&preflight.target_branch);

        format!(
            r#"The user likes the state of the code.

There are {} uncommitted changes.
The current branch is {}.
The target branch is {}.

{}The user requested a PR.

Follow these exact steps to create a PR:

1. Run git diff to review uncommitted changes
2. Commit them with a clear, descriptive commit message
3. Push to origin
4. Use git diff {}... to review the PR diff
5. Use gh pr create --base {} to create a PR. Keep the title under 80 characters and the description under five sentences.
6. If any of these steps fail, explain what went wrong."#,
            preflight.uncommitted_count,
            preflight.branch_name,
            preflight.target_branch,
            upstream_note,
            base_branch,
            base_branch,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::Path;
    use std::process::Command;
    use tempfile::tempdir;

    fn run_git(path: &Path, args: &[&str]) -> io::Result<String> {
        let output = Command::new("git").args(args).current_dir(path).output()?;
        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "git {} failed: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn init_git_repo(path: &Path) -> io::Result<()> {
        run_git(path, &["init"])?;
        run_git(path, &["config", "user.email", "test@test.com"])?;
        run_git(path, &["config", "user.name", "Test"])?;

        std::fs::write(path.join("README.md"), "# Test")?;
        run_git(path, &["add", "."])?;
        run_git(path, &["commit", "-m", "Initial commit"])?;

        Ok(())
    }

    #[test]
    fn test_is_main_branch() {
        assert!(PrManager::is_main_branch("main"));
        assert!(PrManager::is_main_branch("master"));
        assert!(PrManager::is_main_branch("develop"));
        assert!(!PrManager::is_main_branch("feature/foo"));
        assert!(!PrManager::is_main_branch("fix/bar"));
    }

    #[test]
    fn test_generate_pr_prompt() {
        let preflight = PrPreflightResult {
            gh_installed: true,
            gh_authenticated: true,
            on_main_branch: false,
            branch_name: "feature/add-pr-support".to_string(),
            target_branch: "origin/main".to_string(),
            uncommitted_count: 5,
            has_upstream: false,
            existing_pr: None,
        };

        let prompt = PrManager::generate_pr_prompt(&preflight);
        assert!(prompt.contains("5 uncommitted changes"));
        assert!(prompt.contains("feature/add-pr-support"));
        assert!(prompt.contains("origin/main"));
        assert!(prompt.contains("no upstream branch"));
        assert!(prompt.contains("gh pr create --base main"));
    }

    #[test]
    fn test_get_current_branch_detached() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path()).unwrap();

        let head_sha = run_git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        run_git(dir.path(), &["checkout", "--detach", &head_sha]).unwrap();

        let branch = PrManager::get_current_branch(dir.path());
        assert_eq!(branch.as_deref(), Some("(detached)"));
    }

    #[test]
    fn test_get_current_branch_on_branch() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path()).unwrap();

        let branch = PrManager::get_current_branch(dir.path());
        assert!(matches!(branch.as_deref(), Some("main") | Some("master")));
    }

    #[test]
    fn test_parse_repo_name_from_url_https() {
        let name = PrManager::parse_repo_name_from_url("https://github.com/user/awesome-repo.git");
        assert_eq!(name, Some("awesome-repo".to_string()));

        let name = PrManager::parse_repo_name_from_url("https://github.com/user/awesome-repo");
        assert_eq!(name, Some("awesome-repo".to_string()));
    }

    #[test]
    fn test_parse_repo_name_from_url_ssh() {
        let name = PrManager::parse_repo_name_from_url("git@github.com:user/awesome-repo.git");
        assert_eq!(name, Some("awesome-repo".to_string()));

        let name = PrManager::parse_repo_name_from_url("git@github.com:user/awesome-repo");
        assert_eq!(name, Some("awesome-repo".to_string()));
    }

    #[test]
    fn test_parse_repo_name_from_url_nested_paths() {
        // GitLab style nested groups
        let name = PrManager::parse_repo_name_from_url("https://gitlab.com/org/subgroup/repo.git");
        assert_eq!(name, Some("repo".to_string()));
    }

    #[test]
    fn test_parse_repo_name_from_url_fallback() {
        // Fallback: plain strings return the string itself (handles local paths)
        let name = PrManager::parse_repo_name_from_url("my-local-repo");
        assert_eq!(name, Some("my-local-repo".to_string()));

        // Paths with directories use the last component
        let name = PrManager::parse_repo_name_from_url("/home/user/projects/my-repo");
        assert_eq!(name, Some("my-repo".to_string()));
    }
}
