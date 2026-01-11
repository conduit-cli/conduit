//! Integration tests for PR creation workflow
//!
//! Tests the PR preflight checks and workflow using temporary git repositories.

use super::common::git_fixtures::TestRepo;
use conduit::{PrManager, PrPreflightResult};

/// Test that preflight detects a clean repository
#[test]
fn test_preflight_detects_clean_repo() {
    let repo = TestRepo::new();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(
        uncommitted, 0,
        "Clean repo should have no uncommitted changes"
    );
}

/// Test that preflight detects uncommitted (untracked) changes
#[test]
fn test_preflight_detects_uncommitted_changes() {
    let repo = TestRepo::with_uncommitted_changes();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 1, "Should detect 1 uncommitted file");
}

/// Test that preflight detects staged changes
#[test]
fn test_preflight_detects_staged_changes() {
    let repo = TestRepo::with_staged_changes();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 1, "Should detect 1 staged file");
}

/// Test that preflight detects mixed staged and unstaged changes
#[test]
fn test_preflight_detects_mixed_changes() {
    let repo = TestRepo::with_mixed_changes();

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 2, "Should detect 2 changed files");
}

/// Test getting current branch
#[test]
fn test_get_current_branch() {
    let repo = TestRepo::new();

    // Default branch after init
    let branch = PrManager::get_current_branch(&repo.path);
    assert!(branch.is_some(), "Should get current branch");

    // Create and checkout a feature branch
    repo.checkout_new_branch("feature/test-branch");

    let branch = PrManager::get_current_branch(&repo.path).unwrap();
    assert_eq!(branch, "feature/test-branch");
}

/// Test main branch detection
#[test]
fn test_is_main_branch() {
    assert!(PrManager::is_main_branch("main"));
    assert!(PrManager::is_main_branch("master"));
    assert!(PrManager::is_main_branch("develop"));
    assert!(!PrManager::is_main_branch("feature/foo"));
    assert!(!PrManager::is_main_branch("fcoury/bold-fox"));
    assert!(!PrManager::is_main_branch("fix/bug-123"));
    assert!(!PrManager::is_main_branch("release/v1.0"));
}

/// Test upstream detection (no remote, so no upstream)
#[test]
fn test_has_no_upstream_initially() {
    let repo = TestRepo::new();
    repo.checkout_new_branch("feature/test");

    // No remote configured, so no upstream
    let has_upstream = PrManager::has_upstream(&repo.path);
    assert!(!has_upstream, "Should have no upstream without remote");
}

/// Test PR prompt generation
#[test]
fn test_pr_prompt_generation() {
    let preflight = PrPreflightResult {
        gh_installed: true,
        gh_authenticated: true,
        on_main_branch: false,
        branch_name: "fcoury/bold-fox".to_string(),
        target_branch: "origin/main".to_string(),
        uncommitted_count: 3,
        has_upstream: false,
        existing_pr: None,
    };

    let prompt = PrManager::generate_pr_prompt(&preflight);

    // Verify key information is in the prompt
    assert!(
        prompt.contains("3 uncommitted changes"),
        "Should mention uncommitted count"
    );
    assert!(
        prompt.contains("fcoury/bold-fox"),
        "Should mention branch name"
    );
    assert!(
        prompt.contains("no upstream branch"),
        "Should mention no upstream"
    );
    assert!(
        prompt.contains("gh pr create --base main"),
        "Should include PR creation command"
    );
    assert!(prompt.contains("git diff"), "Should include diff command");
}

/// Test PR prompt when upstream exists
#[test]
fn test_pr_prompt_with_upstream() {
    let preflight = PrPreflightResult {
        gh_installed: true,
        gh_authenticated: true,
        on_main_branch: false,
        branch_name: "feature/new-feature".to_string(),
        target_branch: "origin/main".to_string(),
        uncommitted_count: 0,
        has_upstream: true,
        existing_pr: None,
    };

    let prompt = PrManager::generate_pr_prompt(&preflight);

    // Should NOT mention "no upstream" when upstream exists
    assert!(
        !prompt.contains("no upstream branch"),
        "Should not mention 'no upstream' when upstream exists"
    );
    assert!(
        prompt.contains("0 uncommitted changes"),
        "Should mention zero uncommitted"
    );
}

/// Test repo name extraction via PrManager::get_repo_name
///
/// Tests the full flow of extracting repo name from a git remote URL.
/// URL parsing logic is tested in unit tests in src/git/pr.rs.
#[test]
fn test_get_repo_name_from_remote() {
    let repo = TestRepo::new();

    // Add a remote with HTTPS URL
    repo.set_remote("origin", "https://github.com/user/awesome-repo.git");

    let name = PrManager::get_repo_name(&repo.path);
    assert_eq!(name, Some("awesome-repo".to_string()));
}

/// Test repo name falls back to directory name when no remote exists
#[test]
fn test_get_repo_name_fallback_to_directory() {
    let repo = TestRepo::new();

    // No remote configured, should fall back to directory name
    let name = PrManager::get_repo_name(&repo.path);

    // The temp directory name is random, but should be Some
    assert!(name.is_some(), "Should fall back to directory name");
}

/// Test that preflight works with a feature branch
#[test]
fn test_preflight_on_feature_branch() {
    let repo = TestRepo::new();
    repo.checkout_new_branch("feature/test-pr");
    repo.create_file("new_feature.rs", "// New feature code");

    let branch = PrManager::get_current_branch(&repo.path);
    assert_eq!(branch, Some("feature/test-pr".to_string()));

    let is_main = PrManager::is_main_branch(branch.as_deref().unwrap_or(""));
    assert!(!is_main, "feature branch should not be detected as main");

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(uncommitted, 1, "Should have one uncommitted file");
}

/// Test workflow: create branch, make changes, check preflight
#[test]
fn test_full_pr_preflight_workflow() {
    let repo = TestRepo::new();

    // 1. Create feature branch
    repo.checkout_new_branch("user/add-feature");

    // 2. Make some changes
    repo.create_file("src/feature.rs", "pub fn new_feature() {}");
    repo.create_file("tests/feature_test.rs", "#[test] fn test_it() {}");

    // 3. Stage one file
    repo.stage_file("src/feature.rs");

    // 4. Check preflight
    let branch = PrManager::get_current_branch(&repo.path).unwrap();
    assert_eq!(branch, "user/add-feature");

    let uncommitted = PrManager::count_uncommitted_changes(&repo.path);
    assert_eq!(
        uncommitted, 2,
        "Should have 2 uncommitted files (1 staged, 1 untracked)"
    );

    let is_main = PrManager::is_main_branch(&branch);
    assert!(!is_main);

    let has_upstream = PrManager::has_upstream(&repo.path);
    assert!(!has_upstream, "No remote configured");

    // 5. Generate prompt
    let preflight = PrPreflightResult {
        gh_installed: true,
        gh_authenticated: true,
        on_main_branch: is_main,
        branch_name: branch,
        target_branch: "origin/main".to_string(),
        uncommitted_count: uncommitted,
        has_upstream,
        existing_pr: None,
    };

    let prompt = PrManager::generate_pr_prompt(&preflight);
    assert!(prompt.contains("2 uncommitted changes"));
    assert!(prompt.contains("user/add-feature"));
}
