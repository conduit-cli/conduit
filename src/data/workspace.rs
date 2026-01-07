//! Workspace data access object

use super::models::Workspace;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Data access object for Workspace operations
#[derive(Clone)]
pub struct WorkspaceStore {
    conn: Arc<Mutex<Connection>>,
}

impl WorkspaceStore {
    /// Create a new WorkspaceDao
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a new workspace
    pub fn create(&self, workspace: &Workspace) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, repository_id, name, branch, path, created_at, last_accessed, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                workspace.id.to_string(),
                workspace.repository_id.to_string(),
                workspace.name,
                workspace.branch,
                workspace.path.to_string_lossy().to_string(),
                workspace.created_at.to_rfc3339(),
                workspace.last_accessed.to_rfc3339(),
                workspace.is_default as i32,
            ],
        )?;
        Ok(())
    }

    /// Get a workspace by ID
    pub fn get_by_id(&self, id: Uuid) -> SqliteResult<Option<Workspace>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repository_id, name, branch, path, created_at, last_accessed, is_default, archived_at, archived_commit_sha
             FROM workspaces WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_workspace(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get all active (non-archived) workspaces for a repository
    pub fn get_by_repository(&self, repository_id: Uuid) -> SqliteResult<Vec<Workspace>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repository_id, name, branch, path, created_at, last_accessed, is_default, archived_at, archived_commit_sha
             FROM workspaces WHERE repository_id = ?1 AND archived_at IS NULL ORDER BY is_default DESC, name",
        )?;

        let workspaces = stmt
            .query_map(params![repository_id.to_string()], |row| {
                Self::row_to_workspace(row)
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workspaces)
    }

    /// Get ALL workspace names for a repository (including archived)
    ///
    /// Used for uniqueness checks to prevent resurrection of old workspace names.
    /// Unlike `get_by_repository`, this includes archived workspaces.
    pub fn get_all_names_by_repository(&self, repository_id: Uuid) -> SqliteResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT name FROM workspaces WHERE repository_id = ?1")?;

        let names = stmt
            .query_map(params![repository_id.to_string()], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(names)
    }

    /// Get all active (non-archived) workspaces
    pub fn get_all(&self) -> SqliteResult<Vec<Workspace>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repository_id, name, branch, path, created_at, last_accessed, is_default, archived_at, archived_commit_sha
             FROM workspaces WHERE archived_at IS NULL ORDER BY repository_id, is_default DESC, name",
        )?;

        let workspaces = stmt
            .query_map([], Self::row_to_workspace)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workspaces)
    }

    /// Update the last accessed timestamp
    pub fn update_last_accessed(&self, id: Uuid) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE workspaces SET last_accessed = ?2 WHERE id = ?1",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Update a workspace
    pub fn update(&self, workspace: &Workspace) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE workspaces SET name = ?2, branch = ?3, path = ?4, last_accessed = ?5, is_default = ?6
             WHERE id = ?1",
            params![
                workspace.id.to_string(),
                workspace.name,
                workspace.branch,
                workspace.path.to_string_lossy().to_string(),
                workspace.last_accessed.to_rfc3339(),
                workspace.is_default as i32,
            ],
        )?;
        Ok(())
    }

    /// Delete a workspace
    pub fn delete(&self, id: Uuid) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM workspaces WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    /// Check if a workspace exists by path
    pub fn exists_by_path(&self, path: &Path) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let path_str = path.to_string_lossy().to_string();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspaces WHERE path = ?1",
            params![path_str],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get the default workspace for a repository
    pub fn get_default_for_repository(
        &self,
        repository_id: Uuid,
    ) -> SqliteResult<Option<Workspace>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repository_id, name, branch, path, created_at, last_accessed, is_default, archived_at, archived_commit_sha
             FROM workspaces WHERE repository_id = ?1 AND is_default = 1 AND archived_at IS NULL",
        )?;

        let mut rows = stmt.query(params![repository_id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_workspace(row)?))
        } else {
            Ok(None)
        }
    }

    /// Archive a workspace (soft delete - marks as archived and stores the branch SHA)
    pub fn archive(&self, id: Uuid, archived_commit_sha: Option<String>) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE workspaces SET archived_at = ?2, archived_commit_sha = ?3 WHERE id = ?1",
            params![id.to_string(), Utc::now().to_rfc3339(), archived_commit_sha],
        )?;
        Ok(())
    }

    /// Convert a database row to a Workspace
    fn row_to_workspace(row: &rusqlite::Row) -> SqliteResult<Workspace> {
        let id_str: String = row.get(0)?;
        let repo_id_str: String = row.get(1)?;
        let path_str: String = row.get(4)?;
        let created_at_str: String = row.get(5)?;
        let last_accessed_str: String = row.get(6)?;
        let is_default: i32 = row.get(7)?;
        let archived_at_str: Option<String> = row.get(8)?;
        let archived_commit_sha: Option<String> = row.get(9)?;

        Ok(Workspace {
            id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
            repository_id: Uuid::parse_str(&repo_id_str).unwrap_or_else(|_| Uuid::new_v4()),
            name: row.get(2)?,
            branch: row.get(3)?,
            path: PathBuf::from(path_str),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_accessed: DateTime::parse_from_rfc3339(&last_accessed_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            is_default: is_default != 0,
            archived_at: archived_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            }),
            archived_commit_sha,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Database, Repository, RepositoryStore};
    use tempfile::tempdir;

    fn setup_db() -> (tempfile::TempDir, Database, RepositoryStore, WorkspaceStore) {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).unwrap();
        let repo_dao = RepositoryStore::new(db.connection());
        let ws_dao = WorkspaceStore::new(db.connection());
        (dir, db, repo_dao, ws_dao)
    }

    #[test]
    fn test_create_and_get() {
        let (_dir, _db, repo_dao, ws_dao) = setup_db();

        // Create a repository first
        let repo = Repository::from_local_path("test-repo", PathBuf::from("/tmp/test"));
        repo_dao.create(&repo).unwrap();

        // Create a workspace
        let ws = Workspace::new(
            repo.id,
            "main",
            "main",
            PathBuf::from("/tmp/test/worktrees/main"),
        );
        ws_dao.create(&ws).unwrap();

        let retrieved = ws_dao.get_by_id(ws.id).unwrap().unwrap();
        assert_eq!(retrieved.name, "main");
        assert_eq!(retrieved.branch, "main");
    }

    #[test]
    fn test_get_by_repository() {
        let (_dir, _db, repo_dao, ws_dao) = setup_db();

        let repo = Repository::from_local_path("test-repo", PathBuf::from("/tmp/test"));
        repo_dao.create(&repo).unwrap();

        let ws1 = Workspace::new_default(repo.id, "main", "main", PathBuf::from("/tmp/main"));
        let ws2 = Workspace::new(
            repo.id,
            "feature",
            "feature-branch",
            PathBuf::from("/tmp/feature"),
        );

        ws_dao.create(&ws1).unwrap();
        ws_dao.create(&ws2).unwrap();

        let workspaces = ws_dao.get_by_repository(repo.id).unwrap();
        assert_eq!(workspaces.len(), 2);
        assert!(workspaces[0].is_default); // Default comes first
    }

    #[test]
    fn test_cascade_delete() {
        let (_dir, _db, repo_dao, ws_dao) = setup_db();

        let repo = Repository::from_local_path("test-repo", PathBuf::from("/tmp/test"));
        repo_dao.create(&repo).unwrap();

        let ws = Workspace::new(repo.id, "main", "main", PathBuf::from("/tmp/main"));
        ws_dao.create(&ws).unwrap();

        // Delete repository should cascade to workspaces
        repo_dao.delete(repo.id).unwrap();

        let workspaces = ws_dao.get_by_repository(repo.id).unwrap();
        assert!(workspaces.is_empty());
    }
}
