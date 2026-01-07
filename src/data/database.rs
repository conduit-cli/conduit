//! SQLite database management

use rusqlite::{Connection, Result as SqliteResult};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// SQL schema for creating tables
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_path TEXT,
    repository_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL,
    name TEXT NOT NULL,
    branch TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_accessed TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    archived_at TEXT,
    archived_commit_sha TEXT,
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workspaces_repository ON workspaces(repository_id);

CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session_tabs (
    id TEXT PRIMARY KEY,
    tab_index INTEGER NOT NULL,
    workspace_id TEXT,
    agent_type TEXT NOT NULL,
    agent_session_id TEXT,
    model TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_session_tabs_order ON session_tabs(tab_index);
"#;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Failed to determine data directory")]
    NoDataDir,
    #[error("Failed to create data directory: {0}")]
    CreateDir(std::io::Error),
    #[error("Lock poisoned")]
    LockPoisoned,
}

/// Database connection wrapper
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    /// Path to the database file
    pub path: PathBuf,
}

impl Database {
    /// Open or create a database at the specified path
    pub fn open(path: PathBuf) -> Result<Self, DatabaseError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(DatabaseError::CreateDir)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        };

        db.initialize()?;
        Ok(db)
    }

    /// Open database in the default location (~/.conduit/conduit.db)
    pub fn open_default() -> Result<Self, DatabaseError> {
        Self::open(crate::util::database_path())
    }

    /// Initialize the database schema
    fn initialize(&self) -> Result<(), DatabaseError> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        conn.execute_batch(SCHEMA)?;
        drop(conn);

        // Apply migrations for existing databases
        self.apply_migrations()?;
        Ok(())
    }

    /// Apply database migrations for existing databases
    fn apply_migrations(&self) -> Result<(), DatabaseError> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;

        // Migration 1: Add archived_at column to workspaces table
        let has_archived_at: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('workspaces') WHERE name='archived_at'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_archived_at {
            conn.execute("ALTER TABLE workspaces ADD COLUMN archived_at TEXT", [])?;
        }

        // Migration 2: Add pr_number column to session_tabs table
        let has_pr_number: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='pr_number'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_pr_number {
            conn.execute("ALTER TABLE session_tabs ADD COLUMN pr_number INTEGER", [])?;
        }

        // Migration 3: Add pending_user_message column to session_tabs table
        let has_pending_user_message: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='pending_user_message'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_pending_user_message {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN pending_user_message TEXT",
                [],
            )?;
        }

        // Migration 4: Add agent_mode column to session_tabs table
        let has_agent_mode: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_tabs') WHERE name='agent_mode'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_agent_mode {
            conn.execute(
                "ALTER TABLE session_tabs ADD COLUMN agent_mode TEXT DEFAULT 'build'",
                [],
            )?;
        }

        // Migration 5: Add archived_commit_sha column to workspaces table
        let has_archived_commit_sha: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('workspaces') WHERE name='archived_commit_sha'",
                [],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if !has_archived_commit_sha {
            conn.execute(
                "ALTER TABLE workspaces ADD COLUMN archived_commit_sha TEXT",
                [],
            )?;
        }

        Ok(())
    }

    /// Get a reference to the connection (for DAOs)
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    /// Execute a closure with the connection
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, DatabaseError>
    where
        F: FnOnce(&Connection) -> SqliteResult<T>,
    {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        f(&conn).map_err(DatabaseError::Sqlite)
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_creation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let _db = Database::open(db_path.clone()).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn test_schema_initialization() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::open(db_path).unwrap();

        // Verify tables exist
        db.with_connection(|conn| {
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
            let tables: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            assert!(tables.contains(&"repositories".to_string()));
            assert!(tables.contains(&"workspaces".to_string()));
            Ok(())
        })
        .unwrap();
    }
}
