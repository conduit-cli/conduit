//! Integration tests for workspace creation flow
//!
//! Tests the full workflow of creating workspaces with git worktrees
//! and persisting them to the database.

use super::common::git_fixtures::TestRepo;
use conduit::{generate_branch_name, generate_workspace_name};
use conduit::{Database, Repository, RepositoryStore, Workspace, WorkspaceStore, WorktreeManager};
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

/// Create a test database in a temporary directory with stores
fn create_test_db() -> (Database, RepositoryStore, WorkspaceStore, TempDir) {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let db = Database::open(db_path).expect("Failed to open database");
    let repo_store = RepositoryStore::new(db.connection());
    let ws_store = WorkspaceStore::new(db.connection());
    (db, repo_store, ws_store, dir)
}

/// Test that workspace name generation avoids collisions
#[test]
fn test_workspace_name_uniqueness() {
    let existing: Vec<String> = Vec::new();
    let name1 = generate_workspace_name(&existing);

    let existing_with_name1: Vec<String> = vec![name1.clone()];

    let name2 = generate_workspace_name(&existing_with_name1);

    assert_ne!(name1, name2, "Generated names should be unique");
}

/// Test that workspace names follow the expected pattern
#[test]
fn test_workspace_name_format() {
    let existing: Vec<String> = Vec::new();
    let name = generate_workspace_name(&existing);

    // Should be adjective-noun format
    assert!(
        name.contains('-'),
        "Workspace name should contain a hyphen: {}",
        name
    );

    let parts: Vec<&str> = name.split('-').collect();
    assert_eq!(parts.len(), 2, "Should have exactly two parts");
    assert!(!parts[0].is_empty(), "Adjective should not be empty");
    assert!(!parts[1].is_empty(), "Noun should not be empty");
}

/// Test branch name generation from username and workspace
#[test]
fn test_branch_name_generation() {
    let branch = generate_branch_name("fcoury", "bold-fox");
    assert_eq!(branch, "fcoury/bold-fox");
}

/// Test branch name sanitizes special characters in username
#[test]
fn test_branch_name_sanitization() {
    let branch = generate_branch_name("user@domain.com", "bold-fox");

    // Username should be sanitized (no @)
    assert!(
        !branch.contains('@'),
        "Branch should not contain @ in username"
    );
    // Should have proper format
    assert!(
        branch.contains('/'),
        "Branch should have username/workspace format"
    );

    // Test with spaces in username
    let branch = generate_branch_name("John Doe", "swift-eagle");
    assert!(
        !branch.contains(' '),
        "Branch should not contain spaces in username"
    );
}

// NOTE: Worktree creation tests have been moved to use WorktreeManager directly
// (see test_worktree_manager_existing_branch, test_worktree_manager_new_branch)
// to ensure production code paths are exercised rather than test fixtures.

/// Test workspace persistence to database
#[test]
fn test_workspace_persistence() {
    let (_db, repo_store, ws_store, _dir) = create_test_db();

    // Create repository first
    let repo = Repository::from_local_path("test-repo", PathBuf::from("/test/path"));
    repo_store.create(&repo).expect("Failed to save repository");

    // Create workspace
    let workspace = Workspace::new(
        repo.id,
        "bold-fox",
        "fcoury/bold-fox",
        PathBuf::from("/test/path/worktrees/bold-fox"),
    );
    ws_store
        .create(&workspace)
        .expect("Failed to save workspace");

    // Verify retrieval
    let loaded = ws_store
        .get_by_id(workspace.id)
        .expect("Failed to query")
        .expect("Workspace should exist");

    assert_eq!(loaded.name, "bold-fox");
    assert_eq!(loaded.branch, "fcoury/bold-fox");
    assert_eq!(loaded.repository_id, repo.id);
}

/// Test getting workspace names for a repository
#[test]
fn test_workspace_names_query() {
    let (_db, repo_store, ws_store, _dir) = create_test_db();

    let repo = Repository::from_local_path("test-repo", PathBuf::from("/test/repo"));
    repo_store.create(&repo).unwrap();

    // Initially no workspaces
    let names = ws_store.get_all_names_by_repository(repo.id).unwrap();
    assert!(names.is_empty());

    // Create some workspaces
    let ws1 = Workspace::new(repo.id, "bold-fox", "branch1", PathBuf::from("/path1"));
    ws_store.create(&ws1).unwrap();

    let ws2 = Workspace::new(repo.id, "swift-eagle", "branch2", PathBuf::from("/path2"));
    ws_store.create(&ws2).unwrap();

    // Query names
    let names = ws_store.get_all_names_by_repository(repo.id).unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"bold-fox".to_string()));
    assert!(names.contains(&"swift-eagle".to_string()));
}

/// Test the full workspace creation flow
#[test]
fn test_full_workspace_creation_flow() {
    let repo = TestRepo::new();
    let (_db, repo_store, ws_store, _db_dir) = create_test_db();

    // 1. Register repository in database
    let db_repo = Repository::from_local_path("test-repo", repo.path.clone());
    repo_store
        .create(&db_repo)
        .expect("Failed to save repository");

    // 2. Generate unique workspace name with UUID suffix to avoid parallel test collisions
    let existing = ws_store.get_all_names_by_repository(db_repo.id).unwrap();
    let base_name = generate_workspace_name(&existing);
    let unique_suffix = Uuid::new_v4().as_simple().to_string();
    let workspace_name = format!("{}-{}", base_name, &unique_suffix[..8]);

    // 3. Generate branch name
    let branch_name = generate_branch_name("testuser", &workspace_name);

    // 4. Create worktree (tracked for automatic cleanup)
    let worktree_path = repo.path.parent().unwrap().join(&workspace_name);
    repo.create_tracked_worktree(&worktree_path, &branch_name);

    // 5. Persist workspace to database
    let workspace = Workspace::new(
        db_repo.id,
        &workspace_name,
        &branch_name,
        worktree_path.clone(),
    );
    ws_store
        .create(&workspace)
        .expect("Failed to save workspace");

    // Verify everything
    assert!(worktree_path.exists(), "Worktree should exist on disk");

    let loaded = ws_store
        .get_by_id(workspace.id)
        .unwrap()
        .expect("Should find workspace in DB");
    assert_eq!(loaded.name, workspace_name);
    assert_eq!(loaded.branch, branch_name);

    // Verify the new name shows up in existing names
    let names = ws_store.get_all_names_by_repository(db_repo.id).unwrap();
    assert!(names.contains(&workspace_name));
}

/// Test creating multiple workspaces for the same repository
#[test]
fn test_multiple_workspaces_per_repo() {
    let repo = TestRepo::new();
    let (_db, repo_store, ws_store, _db_dir) = create_test_db();

    let db_repo = Repository::from_local_path("test-repo", repo.path.clone());
    repo_store.create(&db_repo).unwrap();

    // Create 3 workspaces
    let mut created_names = Vec::new();
    for _ in 0..3 {
        let existing = ws_store.get_all_names_by_repository(db_repo.id).unwrap();
        let base_name = generate_workspace_name(&existing);

        // Ensure no collision with previously created names
        assert!(
            !created_names.contains(&base_name),
            "Generated name {} collides with existing",
            base_name
        );

        // Add UUID suffix to avoid parallel test collisions
        let unique_suffix = Uuid::new_v4().as_simple().to_string();
        let name = format!("{}-{}", base_name, &unique_suffix[..8]);

        let branch = generate_branch_name("testuser", &name);
        let wt_path = repo.path.parent().unwrap().join(&name);

        // Use tracked worktree for automatic cleanup
        repo.create_tracked_worktree(&wt_path, &branch);

        let ws = Workspace::new(db_repo.id, &name, &branch, wt_path);
        ws_store.create(&ws).unwrap();

        created_names.push(base_name);
    }

    // Verify all 3 workspaces exist
    let all_workspaces = ws_store.get_by_repository(db_repo.id).unwrap();
    assert_eq!(all_workspaces.len(), 3);

    let names = ws_store.get_all_names_by_repository(db_repo.id).unwrap();
    assert_eq!(names.len(), 3);
}

/// Test workspace isolation - worktrees don't share state
#[test]
fn test_workspace_isolation() {
    let repo = TestRepo::new();

    // Use unique paths to avoid collisions
    let unique_id = Uuid::new_v4().as_simple().to_string();

    // Create two worktrees (tracked for automatic cleanup)
    let wt1_path = repo
        .path
        .parent()
        .unwrap()
        .join(format!("wt1-{}", &unique_id[..8]));
    let wt2_path = repo
        .path
        .parent()
        .unwrap()
        .join(format!("wt2-{}", &unique_id[..8]));

    repo.create_tracked_worktree(&wt1_path, "branch-1");
    repo.create_tracked_worktree(&wt2_path, "branch-2");

    // Create a file in wt1 only
    std::fs::write(wt1_path.join("wt1_only.txt"), "This is only in wt1").unwrap();

    // Verify file exists in wt1 but not wt2
    assert!(wt1_path.join("wt1_only.txt").exists());
    assert!(!wt2_path.join("wt1_only.txt").exists());

    // Create a file in wt2 only
    std::fs::write(wt2_path.join("wt2_only.txt"), "This is only in wt2").unwrap();

    // Verify isolation both ways
    assert!(!wt1_path.join("wt2_only.txt").exists());
    assert!(wt2_path.join("wt2_only.txt").exists());
}

/// Test WorktreeManager::create_worktree with existing branch
///
/// Exercises the production WorktreeManager to ensure it handles
/// existing branches correctly.
#[test]
fn test_worktree_manager_existing_branch() {
    let repo = TestRepo::with_branches(&["feature-1"]);
    let manager = WorktreeManager::new();

    let unique_id = Uuid::new_v4().as_simple().to_string();
    let worktree_name = format!("wt-manager-{}", &unique_id[..8]);

    let result = manager.create_worktree(&repo.path, "feature-1", &worktree_name);

    assert!(
        result.is_ok(),
        "WorktreeManager should create worktree: {:?}",
        result.err()
    );

    let worktree_path = result.unwrap();
    assert!(worktree_path.exists(), "Worktree should exist");
    assert!(
        worktree_path.join(".git").exists(),
        "Worktree should have .git"
    );

    // Clean up (WorktreeManager doesn't auto-cleanup like TestRepo)
    let _ = manager.remove_worktree(&repo.path, &worktree_path);
}

/// Test WorktreeManager::create_worktree with new branch
///
/// Exercises the production WorktreeManager to ensure it auto-creates
/// branches that don't exist.
#[test]
fn test_worktree_manager_new_branch() {
    let repo = TestRepo::new();
    let manager = WorktreeManager::new();

    let unique_id = Uuid::new_v4().as_simple().to_string();
    let worktree_name = format!("wt-newbranch-{}", &unique_id[..8]);
    let branch_name = format!("feature-new-{}", &unique_id[..8]);

    let result = manager.create_worktree(&repo.path, &branch_name, &worktree_name);

    assert!(
        result.is_ok(),
        "WorktreeManager should create worktree with new branch: {:?}",
        result.err()
    );

    let worktree_path = result.unwrap();
    assert!(worktree_path.exists(), "Worktree should exist");

    // Verify branch was created
    let branches = repo.branches();
    assert!(
        branches.iter().any(|b| b == &branch_name),
        "New branch should exist: {:?}",
        branches
    );

    // Clean up
    let _ = manager.remove_worktree(&repo.path, &worktree_path);
}

/// Test WorktreeManager error handling for already-existing worktree
#[test]
fn test_worktree_manager_already_exists_error() {
    let repo = TestRepo::with_branches(&["feature-1"]);
    let manager = WorktreeManager::new();

    let unique_id = Uuid::new_v4().as_simple().to_string();
    let worktree_name = format!("wt-duplicate-{}", &unique_id[..8]);

    // Create first worktree
    let result1 = manager.create_worktree(&repo.path, "feature-1", &worktree_name);
    assert!(result1.is_ok());
    let worktree_path = result1.unwrap();

    // Try to create another with same name - should fail
    let result2 = manager.create_worktree(&repo.path, "feature-1", &worktree_name);
    assert!(result2.is_err(), "Should fail when worktree already exists");

    // Clean up
    let _ = manager.remove_worktree(&repo.path, &worktree_path);
}
